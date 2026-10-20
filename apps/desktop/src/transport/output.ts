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
    if (this.end && (this.end.sequence !== next.sequence || this.end.bytes !== next.bytes)) throw new Error('重复结束屏障不一致');
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
  constructor(private send: (sequence: string, bytes: string) => Promise<void>, private failed: (error: unknown) => void) {}
  update(sequence: string, bytes: string): void { this.latest = [sequence, bytes]; if (!this.timer && !this.sending && !this.stopped) this.timer = setTimeout(() => { this.timer = null; void this.flush(); }, 8); }
  private async flush(): Promise<void> {
    if (this.sending || this.stopped) return;
    this.sending = true;
    try { while (this.latest && !this.stopped) { const value = this.latest; this.latest = null; await this.send(...value); } }
    catch (error) { this.stopped = true; this.failed(error); }
    finally { this.sending = false; }
  }
  dispose(): void { this.stopped = true; if (this.timer) clearTimeout(this.timer); this.latest = null; }
}
