import { describe, it, expect, vi } from 'vitest';
import { counter, INPUT_BUDGET, INPUT_CHUNK, validateBinding } from '../contracts/models';
import { InputQueue, binaryBytes } from './input';
import { OutputProgress, AckQueue } from './output';

describe('raw byte and counter contract', () => {
  it('keeps non-UTF8 input bytes and 64-bit counters intact', () => {
    expect([...binaryBytes('\x00\x7f\x80\xff')]).toEqual([0, 127, 128, 255]);
    expect(() => binaryBytes('中')).toThrow(); expect(counter('18446744073709551615')).toBe(18446744073709551615n);
    for (const invalid of ['01', '-1', '1.1', '18446744073709551616']) expect(() => counter(invalid)).toThrow();
    expect(() => validateBinding({ sessionId: 'a', viewEpoch: 'b', protocolVersion: 2 })).toThrow();
  });
  it('waits for parsed output even when OutputEnd arrives on the other channel first', () => {
    const ack = vi.fn(); const done = vi.fn(); const callbacks: (() => void)[] = [];
    const progress = new OutputProgress(ack, done);
    progress.finish('2', '4', false);
    progress.receive(Uint8Array.of(0xf0, 0x9f), (_, callback) => callbacks.push(callback));
    progress.receive(Uint8Array.of(0x98, 0x80), (_, callback) => callbacks.push(callback));
    expect(done).not.toHaveBeenCalled(); callbacks[0](); expect(done).not.toHaveBeenCalled(); callbacks[1]();
    expect(ack.mock.calls).toEqual([['1', '2'], ['2', '4']]); expect(done).toHaveBeenCalledExactlyOnceWith(false);
  });
  it('rejects end counters that contradict received bytes', () => {
    const progress = new OutputProgress(vi.fn(), vi.fn()); progress.receive(Uint8Array.of(1, 2), (_, done) => done());
    expect(() => progress.finish('1', '1', false)).toThrow();
  });
  it('rejects contradictory truncation and impossible output barriers', () => {
    const progress = new OutputProgress(vi.fn(), vi.fn());
    progress.finish('1', '1', false);
    expect(() => progress.finish('1', '1', true)).toThrow();
    for (const [sequence, bytes] of [['0', '1'], ['1', '0'], ['2', '1'], ['1', '32769']]) {
      expect(() => new OutputProgress(vi.fn(), vi.fn()).finish(sequence, bytes, false)).toThrow();
    }
  });
});
describe('bounded input', () => {
  it('serializes concurrent sources and reserves an entire paste before sending', async () => {
    let unblock!: () => void; const accepted: { sequence: string; data: Uint8Array }[] = [];
    const queue = new InputQueue(async (sequence, bytes) => { accepted.push({ sequence, data: bytes }); if (sequence === '1') await new Promise<void>(resolve => unblock = resolve); }, vi.fn());
    queue.enqueue(new Uint8Array(INPUT_BUDGET).fill(42));
    expect(accepted).toHaveLength(1); expect(queue.canAccept(1)).toBe(false); expect(() => queue.enqueue(Uint8Array.of(7))).toThrow();
    unblock(); await vi.waitFor(() => expect(accepted).toHaveLength(INPUT_BUDGET / INPUT_CHUNK));
    expect(accepted.map(p => p.sequence)).toEqual(Array.from({ length: 32 }, (_, i) => String(i + 1)));
    expect(accepted.every(p => p.data.length === INPUT_CHUNK && p.data[0] === 42)).toBe(true);
  });
  it('never retries an input with an uncertain result', async () => {
    const send = vi.fn().mockRejectedValue(new Error('connection lost')); const failed = vi.fn(); const queue = new InputQueue(send, failed);
    queue.enqueue(Uint8Array.of(1)); await vi.waitFor(() => expect(failed).toHaveBeenCalledOnce());
    expect(send).toHaveBeenCalledOnce(); expect(queue.canAccept(1)).toBe(false);
  });
  it('cannot revive a disposed input queue when an in-flight request is rejected', async () => {
    let reject!: (error: unknown) => void;
    const failed = vi.fn();
    const queue = new InputQueue(() => new Promise<void>((_, fail) => { reject = fail; }), failed);
    queue.enqueue(Uint8Array.of(1));
    queue.dispose();
    reject({ code: 'control_busy' });
    await Promise.resolve();
    expect(queue.canAccept(1)).toBe(false);
    expect(failed).not.toHaveBeenCalled();
  });
  it('coalesces ACKs without making concurrent invokes', async () => {
    vi.useFakeTimers(); let resolve!: () => void;
    const send = vi.fn().mockImplementationOnce(() => new Promise<void>(r => resolve = r)).mockResolvedValue(undefined);
    const acks = new AckQueue(send, vi.fn()); acks.update('1', '10'); acks.update('2', '20');
    await vi.advanceTimersByTimeAsync(8); expect(send).toHaveBeenCalledExactlyOnceWith('2', '20');
    acks.update('3', '30'); expect(send).toHaveBeenCalledOnce(); resolve(); await vi.advanceTimersByTimeAsync(8);
    expect(send.mock.calls).toEqual([['2', '20'], ['3', '30']]); acks.dispose(); vi.useRealTimers();
  });
  it('retries a rejected ACK without losing a newer consumed prefix', async () => {
    vi.useFakeTimers();
    try {
      const failed = vi.fn();
      const send = vi.fn().mockRejectedValueOnce({ code: 'control_busy' }).mockResolvedValue(undefined);
      const acks = new AckQueue(send, failed);
      acks.update('1', '10');
      await vi.advanceTimersByTimeAsync(8);
      acks.update('2', '20');
      await vi.runAllTimersAsync();
      expect(send).toHaveBeenLastCalledWith('2', '20');
      expect(failed).not.toHaveBeenCalled();
      acks.dispose();
    } finally { vi.useRealTimers(); }
  });
  it('bounds rejected ACK retries and cancels them on disposal', async () => {
    vi.useFakeTimers();
    try {
      const send = vi.fn().mockRejectedValue({ code: 'request_rate' });
      const failed = vi.fn();
      const acks = new AckQueue(send, failed);
      acks.update('1', '10');
      await vi.runAllTimersAsync();
      expect(send).toHaveBeenCalledTimes(3);
      expect(failed).toHaveBeenCalledOnce();
      const disposed = new AckQueue(send, failed);
      disposed.update('1', '10');
      await vi.advanceTimersByTimeAsync(8);
      disposed.dispose();
      const attempts = send.mock.calls.length;
      await vi.runAllTimersAsync();
      expect(send).toHaveBeenCalledTimes(attempts);
      expect(failed).toHaveBeenCalledOnce();
    } finally { vi.useRealTimers(); }
  });
});

it('consumes the shared Rust wire fixture without losing cross-chunk UTF-8', async () => {
  const fixture = (await import('../../../../tests/fixtures/protocol.json')).default;
  validateBinding(fixture.binding);
  const decoder = new TextDecoder(); let text = ''; const ack = vi.fn(); const complete = vi.fn();
  const progress = new OutputProgress(ack, complete);
  for (const chunk of fixture.splitUtf8) progress.receive(Uint8Array.from(chunk), (bytes, done) => { text += decoder.decode(bytes, { stream: true }); done(); });
  progress.finish(fixture.outputEnd.sequence, fixture.outputEnd.bytes, fixture.outputEnd.truncated);
  expect(text).toBe('中😀'); expect(ack).toHaveBeenLastCalledWith(fixture.ack.sequence, fixture.ack.bytes); expect(complete).toHaveBeenCalledExactlyOnceWith(false);
});

it('reserves the full wrapped paste at the backend before sending its first byte', async () => {
  const operations: string[] = []; const sent: Uint8Array[] = [];
  const queue = new InputQueue(async (sequence, bytes, token) => { operations.push(`input:${sequence}:${token}`); sent.push(bytes); }, vi.fn(), async bytes => { operations.push(`reserve:${bytes}`); return 'reservation'; });
  queue.enqueue(new Uint8Array(20000).fill(65), true);
  await vi.waitFor(() => expect(sent).toHaveLength(2));
  expect(operations).toEqual(['reserve:20000', 'input:1:reservation', 'input:2:reservation']);
});
it('a rejected paste reservation sends no partial text and permits later deliberate input', async () => {
  const send = vi.fn().mockResolvedValue(undefined); const failed = vi.fn();
  const queue = new InputQueue(send, failed, async () => { throw { code: 'input_full', message: 'busy' }; });
  queue.enqueue(new Uint8Array(20000), true); await vi.waitFor(() => expect(failed).toHaveBeenCalledOnce());
  expect(send).not.toHaveBeenCalled(); expect(queue.canAccept(1)).toBe(true); queue.enqueue(Uint8Array.of(7));
  await vi.waitFor(() => expect(send).toHaveBeenCalledExactlyOnceWith('1', Uint8Array.of(7), undefined));
});
