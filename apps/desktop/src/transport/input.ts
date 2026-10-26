import { INPUT_BUDGET, INPUT_CHUNK, errorMessage } from '../contracts/models';

interface PasteReservation { bytes: number; token?: string }
interface InputChunk { bytes: Uint8Array; reservation?: PasteReservation }
export class InputQueue {
  private queue: InputChunk[] = [];
  private pendingBytes = 0;
  private sequence = 0n;
  private sending = false;
  private stopped = false;
  constructor(
    private send: (sequence: string, bytes: Uint8Array, reservation?: string) => Promise<void>,
    private failed: (error: unknown) => void,
    private reservePaste?: (bytes: number) => Promise<string>,
  ) {}
  canAccept(bytes: number): boolean { return !this.stopped && bytes > 0 && bytes + this.pendingBytes <= INPUT_BUDGET && this.queue.length + Math.ceil(bytes / INPUT_CHUNK) <= 128; }
  enqueue(bytes: Uint8Array, paste = false): void {
    if (!bytes.length) return;
    if (!this.canAccept(bytes.length)) throw new Error('输入队列已满，此次输入未发送');
    this.pendingBytes += bytes.length;
    const reservation: PasteReservation | undefined = paste ? { bytes: bytes.length } : undefined;
    for (let i = 0; i < bytes.length; i += INPUT_CHUNK) this.queue.push({ bytes: bytes.slice(i, i + INPUT_CHUNK), reservation });
    void this.flush();
  }
  private async flush(): Promise<void> {
    if (this.sending || this.stopped) return;
    this.sending = true;
    try {
      while (this.queue.length && !this.stopped) {
        const { bytes, reservation } = this.queue.shift()!;
        if (reservation && !reservation.token) {
          if (!this.reservePaste) throw new Error('传输不支持原子粘贴额度预约');
          // No paste bytes are sent until the backend reserves this complete, already wrapped payload.
          reservation.token = await this.reservePaste(reservation.bytes);
        }
        if (this.stopped) break;
        const next = this.sequence + 1n;
        await this.send(next.toString(), bytes, reservation?.token);
        if (this.stopped) return;
        this.sequence = next;
        this.pendingBytes -= bytes.length;
      }
    } catch (error) {
      if (this.stopped) return;
      const code = typeof error === 'object' && error !== null && 'code' in error ? String(error.code) : '';
      // These rejections explicitly guarantee non-acceptance. An uncertain invoke result is terminal.
      this.stopped = !['input_full', 'control_busy', 'request_rate'].includes(code);
      this.queue = []; this.pendingBytes = 0;
      this.failed(new Error(`${errorMessage(error)}。后续排队输入未发送；已接受的输入可能继续执行。${this.stopped ? '请关闭并重新启动会话后继续。' : '可稍后重新输入。'}`));
    } finally { this.sending = false; }
  }
  dispose(): void { this.stopped = true; this.queue = []; this.pendingBytes = 0; }
}
export function binaryBytes(data: string): Uint8Array {
  const bytes = new Uint8Array(data.length);
  for (let i = 0; i < data.length; i++) { const value = data.charCodeAt(i); if (value > 255) throw new Error('二进制输入包含非字节字符'); bytes[i] = value; }
  return bytes;
}
