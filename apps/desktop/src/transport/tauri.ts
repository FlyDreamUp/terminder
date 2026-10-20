import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import type { Binding, Bootstrap, ConfigDocument, SessionEvent, Size, Snapshot, TerminalConfig, Workspace } from '../contracts/models';
import { validateBinding } from '../contracts/models';
import { InputQueue } from './input';
import { AckQueue } from './output';

export const host = {
  available: isTauri,
  bootstrap: () => invoke<Bootstrap>('bootstrap'),
  config: () => invoke<ConfigDocument>('load_config'),
  saveConfig: (text: string, revision: string) => invoke<ConfigDocument>('save_config', { text, revision }),
  saveTerminal: (terminal: TerminalConfig, revision: string) => invoke<ConfigDocument>('save_terminal', { terminal, revision }),
  prepare: (profileId: string, cwd: string | null, inheritSession: string | null) => invoke<Binding>('prepare_session', { profileId, cwd, inheritSession }),
  list: () => invoke<Snapshot[]>('list_sessions'),
  close: (binding: Binding) => invoke<void>('close_session', { binding }),
  forget: (binding: Binding) => invoke<void>('forget_session', { binding }),
  saveWorkspace: (workspace: Workspace) => invoke<void>('save_workspace', { workspace }),
  clipboardRead: () => invoke<string>('read_clipboard'),
  clipboardWrite: (text: string) => invoke<void>('write_clipboard', { text }),
  openLink: (url: string) => invoke<void>('open_link', { url }),
  diagnostics: () => invoke<string>('diagnostics'),
};
export interface Transport {
  readonly binding: Binding;
  start(size: Size, output: (bytes: Uint8Array) => void, event: (event: SessionEvent) => void): Promise<void>;
  input(bytes: Uint8Array): void;
  paste(bytes: Uint8Array): void;
  canInput(bytes: number): boolean;
  ack(sequence: string, bytes: string): void;
  resize(size: Size): Promise<void>;
  viewFailed(): Promise<void>;
  dispose(): void;
}
export class TauriTransport implements Transport {
  readonly binding: Binding;
  private inputQueue: InputQueue;
  private acks: AckQueue;
  private revision = 0n;
  private disposed = false;
  private outputs: Channel<ArrayBuffer> | null = null;
  private events: Channel<SessionEvent> | null = null;
  constructor(binding: Binding, private failed: (error: unknown) => void) {
    validateBinding(binding); this.binding = binding;
    this.inputQueue = new InputQueue((sequence, bytes, reservation) => invoke('input', bytes, { headers: {
      'x-session-id': binding.sessionId, 'x-view-epoch': binding.viewEpoch,
      'x-protocol-version': String(binding.protocolVersion), 'x-input-sequence': sequence,
      ...(reservation ? { 'x-input-reservation': reservation } : {}),
    } }), failed, bytes => invoke<string>('reserve_input', { binding, bytes }));
    this.acks = new AckQueue((sequence, bytes) => invoke('output_consumed', { binding, sequence, bytes }), failed);
  }
  async start(size: Size, output: (bytes: Uint8Array) => void, event: (event: SessionEvent) => void): Promise<void> {
    this.outputs = new Channel<ArrayBuffer>(); this.events = new Channel<SessionEvent>();
    this.outputs.onmessage = buffer => {
      if (this.disposed) return;
      if (!(buffer instanceof ArrayBuffer)) {
        this.failed(new Error('输出不是原始二进制'));
        void this.viewFailed().catch(this.failed); return;
      }
      output(new Uint8Array(buffer));
    };
    this.events.onmessage = message => { if (!this.disposed) event(message); };
    await invoke('start_session', { binding: this.binding, size, output: this.outputs, events: this.events });
  }
  input(bytes: Uint8Array): void { this.inputQueue.enqueue(bytes); }
  paste(bytes: Uint8Array): void { this.inputQueue.enqueue(bytes, true); }
  canInput(bytes: number): boolean { return this.inputQueue.canAccept(bytes); }
  ack(sequence: string, bytes: string): void { this.acks.update(sequence, bytes); }
  resize(size: Size): Promise<void> { return invoke('resize_session', { binding: this.binding, size, revision: (++this.revision).toString() }); }
  viewFailed(): Promise<void> { return invoke('view_failed', { binding: this.binding }); }
  dispose(): void { this.disposed = true; this.inputQueue.dispose(); this.acks.dispose(); this.outputs = null; this.events = null; }
}
