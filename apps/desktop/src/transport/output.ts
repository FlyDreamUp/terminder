import { counter } from '../contracts/models';

/** Control and data channels have no shared order. Completion waits for the consumed prefix. */
export class OutputProgress {
  private received = 0n;
  private receivedBytes = 0n;
  private consumed = 0n;
  private consumedBytes = 0n;
  private end: { sequence: bigint; bytes: bigint; truncated: boolean } | null = null;
  private completed = false;
  constructor(private ack: (sequence: string, bytes: string) => void, private complete: (truncated: boolean) => void) {}
  receive(bytes: Uint8Array, write: (bytes: Uint8Array, done: () => void) => void): void {
    if (!bytes.length || bytes.length > 32768 || this.completed) throw new Error('无效或迟到的输出块');
    const sequence = ++this.received; const total = this.receivedBytes += BigInt(bytes.length);
    if (this.end && (sequence > this.end.sequence || total > this.end.bytes)) throw new Error('输出超过结束屏障');
    write(bytes, () => {
      if (sequence !== this.consumed + 1n) throw new Error('终端消费回调乱序');
      this.consumed = sequence; this.consumedBytes = total;
      this.ack(sequence.toString(), total.toString()); this.check();
    });
  }
  finish(sequence: string, bytes: string, truncated: boolean): void {
    const next = { sequence: counter(sequence), bytes: counter(bytes), truncated };
    if (next.bytes < next.sequence || next.bytes > next.sequence * 32768n) throw new Error('结束屏障的块数和字节数不匹配');
    if (this.end && (this.end.sequence !== next.sequence || this.end.bytes !== next.bytes || this.end.truncated !== next.truncated)) throw new Error('重复结束屏障不一致');
    if (next.sequence < this.received || next.bytes < this.receivedBytes) throw new Error('结束屏障早于已收到的输出');
    this.end = next; this.check();
  }
  private check(): void {
    if (!this.completed && this.end && this.consumed === this.end.sequence && this.consumedBytes === this.end.bytes) { this.completed = true; this.complete(this.end.truncated); }
  }
  isComplete(): boolean { return this.completed; }
}

export class AckQueue {
  private latest: [string, string] | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private sending = false;
  private stopped = false;
  private attempts = 0;
  constructor(private send: (sequence: string, bytes: string) => Promise<void>, private failed: (error: unknown) => void) {}
  update(sequence: string, bytes: string): void {
    if (this.stopped) return;
    this.latest = [sequence, bytes];
    this.schedule(8);
  }
  private schedule(delay: number): void {
    if (this.timer || this.sending || this.stopped) return;
    this.timer = setTimeout(() => { this.timer = null; void this.flush(); }, delay);
  }
  private async flush(): Promise<void> {
    if (this.sending || this.stopped || !this.latest) return;
    this.sending = true;
    const value = this.latest;
    this.latest = null;
    let delay = 8;
    try {
      await this.send(...value);
      this.attempts = 0;
    } catch (error) {
      if (this.stopped) return;
      const code = typeof error === 'object' && error !== null && 'code' in error ? error.code : '';
      // ACKs are idempotent. Retry explicit congestion, keeping any newer prefix,
      // but bound attempts so a broken transport is reported to its owner.
      if (['control_busy', 'request_rate', 'control_timeout'].includes(String(code)) && ++this.attempts < 3) {
        this.latest ??= value;
        // The host rate limit resets once per second; an earlier retry cannot reliably recover.
        delay = code === 'request_rate' ? 1000 : 25 * 2 ** (this.attempts - 1);
      } else {
        this.stopped = true;
        this.latest = null;
        this.failed(error);
      }
    } finally {
      this.sending = false;
      if (this.latest) this.schedule(delay);
    }
  }
  dispose(): void {
    this.stopped = true;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.latest = null;
  }
}
