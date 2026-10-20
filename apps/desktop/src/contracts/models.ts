export const PROTOCOL_VERSION = 1;
export const INPUT_CHUNK = 16 * 1024;
export const INPUT_BUDGET = 512 * 1024;
export const MAX_PASTE = 256 * 1024;
export interface Binding { sessionId: string; viewEpoch: string; protocolVersion: number }
export interface Size { cols: number; rows: number }
export interface AppError { code: string; message: string; stage: string; osCode?: number | null }
export type Phase = 'preparing' | 'starting' | 'running' | 'draining' | 'closing' | 'finalizing' | 'finished';
export type FinishReason = { kind: 'exited'; code: number } | { kind: 'closed_by_user' | 'spawn_failed' | 'io_failed' | 'view_lost' | 'timed_out' };
export interface Snapshot {
  binding: Binding; profileId: string; profileName: string; startupCwd: string | null;
  cwdSource: string; phase: Phase; reason: FinishReason | null; exitCode: number | null;
  tailComplete: boolean; congested: boolean; cleanupError: AppError | null; size: Size;
}
export type SessionEvent = { type: 'state'; snapshot: Snapshot } | { type: 'output_end'; sequence: string; bytes: string; truncated: boolean } | { type: 'error'; error: AppError };
export interface Profile { id: string; name: string; platforms: string[]; resolver: string | null; program: string | null; args: string[]; cwd_policy: string; cwd: string | null; env: Record<string, string | null> }
export interface TerminalConfig { font_family: string[]; font_size_px: number; theme: 'system' | 'dark' | 'light'; scrollback_lines: number; renderer: 'auto' | 'dom' }
export interface Config {
  schema_version: number; default_profile: string; profiles: Profile[]; terminal: TerminalConfig;
  window: { restore_geometry: boolean; restore_tabs: boolean };
  session: { max_sessions: number; confirm_close_running: boolean };
  clipboard: { confirm_multiline: boolean; max_paste_bytes: number };
  shortcuts: Record<string, string>;
}
export interface ConfigDocument { config: Config; text: string; revision: string; error: AppError | null; writable: boolean }
export interface SavedTab { id: string; profileId: string; startupCwd: string | null; title: string | null }
export interface Workspace { schemaVersion: number; geometry: { x: number; y: number; width: number; height: number; maximized: boolean } | null; tabs: SavedTab[]; activeTab: string | null }
export interface ProfileStatus { id: string; available: boolean; error: AppError | null }
export interface Bootstrap { document: ConfigDocument; profiles: ProfileStatus[]; workspace: Workspace; workspaceError: AppError | null; version: string; platform: string }
export function errorMessage(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) return String(error.message);
  return typeof error === 'string' ? error : '操作失败，请查看诊断信息';
}
export function counter(value: string): bigint {
  if (!/^(0|[1-9]\d{0,19})$/.test(value)) throw new Error('无效的协议计数');
  const n = BigInt(value); if (n > 18446744073709551615n) throw new Error('协议计数越界'); return n;
}
export function validateBinding(binding: Binding): void {
  const uuid = /^[\da-f]{8}(?:-[\da-f]{4}){3}-[\da-f]{12}$/i;
  if (binding.protocolVersion !== PROTOCOL_VERSION || !uuid.test(binding.sessionId) || !uuid.test(binding.viewEpoch)) throw new Error('宿主协议版本或会话绑定不匹配');
}

export function validateSessionEvent(event: SessionEvent, expected: Binding): void {
  if (!event || typeof event !== 'object') throw new Error('无效的会话消息');
  if (event.type === 'state') {
    const snapshot = event.snapshot;
    if (!snapshot || typeof snapshot !== 'object') throw new Error('无效的会话状态');
    validateBinding(snapshot.binding);
    if (snapshot.binding.sessionId !== expected.sessionId || snapshot.binding.viewEpoch !== expected.viewEpoch) throw new Error('收到过期会话状态');
    if (!['preparing', 'starting', 'running', 'draining', 'closing', 'finalizing', 'finished'].includes(snapshot.phase)
      || typeof snapshot.tailComplete !== 'boolean' || typeof snapshot.congested !== 'boolean'
      || typeof snapshot.profileId !== 'string' || typeof snapshot.profileName !== 'string'
      || (snapshot.startupCwd !== null && typeof snapshot.startupCwd !== 'string')
      || !snapshot.size || !Number.isInteger(snapshot.size.cols) || !Number.isInteger(snapshot.size.rows)
      || snapshot.size.cols < 1 || snapshot.size.rows < 1 || snapshot.size.cols > 1000 || snapshot.size.rows > 1000
      || snapshot.size.cols * snapshot.size.rows > 250000) throw new Error('会话状态载荷无效');
  } else if (event.type === 'output_end') {
    if (typeof event.sequence !== 'string' || typeof event.bytes !== 'string' || typeof event.truncated !== 'boolean') throw new Error('输出屏障载荷无效');
    counter(event.sequence); counter(event.bytes);
  } else if (event.type === 'error') {
    if (!event.error || typeof event.error.code !== 'string' || typeof event.error.message !== 'string' || typeof event.error.stage !== 'string') throw new Error('错误消息载荷无效');
  } else throw new Error('未知的会话消息类型');
}
