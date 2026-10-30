// Browser component harness only. This file is outside the production Vite entry graph.
import type { Workspace } from '../../apps/desktop/src/contracts/models';
import type { Page } from '../../apps/desktop/tests/harness';
export async function installHost(page: Page, options: { repeatFinishedState?: boolean; forgetFailures?: number; clipboardDelayMs?: number; finishBeforeRunning?: boolean; prepareFailureAt?: number; closeFailureAt?: number; outputEndDelayMs?: number; maxSessions?: number; workspace?: Workspace } = {}) {
  await page.addInitScript(options => {
    const profile = { id: 'pwsh', name: 'PowerShell 7', platforms: ['windows'], resolver: 'pwsh', program: null, args: ['-NoLogo'], cwd_policy: 'inherit_known', cwd: null, env: {} };
    const profiles = [profile, { ...profile, id: 'cmd', name: '命令提示符', resolver: 'cmd', args: [] }];
    const document = { config: { schema_version: 1, default_profile: 'auto', profiles, terminal: { font_family: ['Consolas', 'monospace'], font_size_px: 14, theme: 'dark', scrollback_lines: 5000, renderer: 'dom' }, window: { restore_geometry: true, restore_tabs: !!options.workspace }, session: { max_sessions: options.maxSessions ?? 16, confirm_close_running: true }, clipboard: { confirm_multiline: true, max_paste_bytes: 262144 }, shortcuts: { new_tab: 'Ctrl+Shift+T', close_tab: 'Ctrl+Shift+W', close_pane: 'Ctrl+Alt+W', split_right: 'Ctrl+Alt+R', split_down: 'Ctrl+Alt+D', focus_left: 'Alt+ArrowLeft', focus_right: 'Alt+ArrowRight', focus_up: 'Alt+ArrowUp', focus_down: 'Alt+ArrowDown', toggle_pane_zoom: 'Ctrl+Alt+Z', next_tab: 'Ctrl+Tab', previous_tab: 'Ctrl+Shift+Tab', copy: 'Ctrl+Shift+C', paste: 'Ctrl+Shift+V', find: 'Ctrl+Shift+F', settings: 'Ctrl+,', zoom_in: 'Ctrl+=', zoom_out: 'Ctrl+-', zoom_reset: 'Ctrl+0' } }, text: 'schema_version = 1\ndefault_profile = "auto"\n', revision: 'fixture', error: null, writable: true };
    const sessions = new Map<string, any>(); const callbacks = new Map<number, any>(); let callbackId = 0;
    const listeners = new Map<number, { event: string; handler: number }>(); let listenerId = 0;
    const windowState = { maximized: false, minimized: false, closeRequests: 0 };
    let forgetFailures = options.forgetFailures ?? 0;
    let preparations = 0; let closes = 0;
    const evidence = { starts: 0, preparations: [] as string[], input: [] as number[][], inputBySession: [] as { id: string; bytes: number[] }[], prepared: [] as any[], closed: [] as string[], forgotten: [] as string[], resize: [] as any[], acknowledgments: [] as any[], clipboard: '', clipboardReads: 0, workspace: null as any, events: [] as string[], windowState };
    (window as any).__testEvidence = evidence;
    (window as any).__emitOutput = (id: string, text: string) => {
      const item = sessions.get(id); const bytes = new TextEncoder().encode(text);
      item.sequence++; item.bytes += bytes.length; item.output.onmessage(bytes.buffer);
    };
    (window as any).__finishSession = (id: string) => {
      const item = sessions.get(id);
      item.snapshot = { ...item.snapshot, phase: 'finished', reason: { kind: 'exited', code: 0 }, exitCode: 0 };
      item.events.onmessage({ type: 'state', snapshot: item.snapshot });
      item.events.onmessage({ type: 'output_end', sequence: String(item.sequence), bytes: String(item.bytes), truncated: false });
    };
    (window as any).isTauri = true;
    (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_event: string, id: number) => listeners.delete(id) };
    (window as any).__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
      transformCallback: (callback: any) => { const id = ++callbackId; callbacks.set(id, callback); return id; },
      unregisterCallback: (id: number) => callbacks.delete(id),
      invoke: async (command: string, args: any, invokeOptions: any) => {
        evidence.events.push(command);
        switch (command) {
          case 'plugin:event|listen': { const id = ++listenerId; listeners.set(id, { event: args.event, handler: args.handler }); return id; }
          case 'plugin:event|unlisten': listeners.delete(args.eventId); return;
          case 'plugin:window|is_maximized': return windowState.maximized;
          case 'plugin:window|minimize': windowState.minimized = true; return;
          case 'plugin:window|toggle_maximize':
            windowState.maximized = !windowState.maximized;
            for (const [id, listener] of listeners) {
              if (listener.event === 'tauri://resize') callbacks.get(listener.handler)?.({ id, event: listener.event, payload: { width: 1100, height: 720 } });
            }
            return;
          case 'plugin:window|close': windowState.closeRequests++; return;
          case 'bootstrap': return { document, profiles: profiles.map(profile => ({ id: profile.id, available: true, error: null })), workspace: options.workspace ?? { schemaVersion: 1, geometry: null, tabs: [], activeTab: null }, workspaceError: null, version: '0.1.0', platform: 'windows' };
          case 'prepare_session': {
            evidence.preparations.push(args.profileId);
            if (++preparations === options.prepareFailureAt) throw { code: 'spawn_failed', message: '测试启动失败，可重试' };
            if (sessions.size >= document.config.session.max_sessions) throw { code: 'session_limit', message: '已达到会话数量上限' };
            const selected = args.profileId === 'default' ? profile : profiles.find(profile => profile.id === args.profileId);
            if (!selected) throw new Error(`Unknown fixture profile: ${args.profileId}`);
            const binding = { sessionId: crypto.randomUUID(), viewEpoch: crypto.randomUUID(), protocolVersion: 1 };
            evidence.prepared.push({ ...args, binding });
            sessions.set(binding.sessionId, { binding, profile: selected }); return binding;
          }
          case 'start_session': {
            evidence.starts++; const item = sessions.get(args.binding.sessionId); item.events = args.events; item.output = args.output;
            item.snapshot = { binding: args.binding, profileId: item.profile.id, profileName: item.profile.name, startupCwd: evidence.prepared.find(p => p.binding.sessionId === args.binding.sessionId)?.cwd ?? 'C:\\Users\\Developer', cwdSource: 'home', phase: 'running', reason: null, exitCode: null, tailComplete: true, congested: false, cleanupError: null, size: args.size };
            const bytes = new TextEncoder().encode(`\x1b[38;2;138;215;193m${item.profile.name}\x1b[0m\r\nBrowser component fixture — no native Shell process\r\n\r\nC:\\Users\\Developer> \x1b[6n`);
            item.sequence = 1; item.bytes = bytes.length; args.output.onmessage(bytes.buffer);
            setTimeout(() => {
              if (options.finishBeforeRunning) {
                item.snapshot = { ...item.snapshot, phase: 'finished', reason: { kind: 'closed_by_user' } };
                args.events.onmessage({ type: 'output_end', sequence: '1', bytes: String(item.bytes), truncated: false });
              }
              args.events.onmessage({ type: 'state', snapshot: item.snapshot });
            }, 50); return;
          }
          case 'reserve_input': return crypto.randomUUID();
          case 'input': evidence.input.push([...args]); evidence.inputBySession.push({ id: invokeOptions.headers['x-session-id'], bytes: [...args] }); return;
          case 'output_consumed': evidence.acknowledgments.push(args); return;
          case 'resize_session': evidence.resize.push(args); return;
          case 'list_sessions': return [...sessions.values()].filter(s => s.snapshot).map(s => s.snapshot);
          case 'close_session': case 'view_failed': {
            if (command === 'close_session' && ++closes === options.closeFailureAt) throw { code: 'control_busy', message: '测试关闭失败，可重试' };
            evidence.closed.push(args.binding.sessionId);
            const item = sessions.get(args.binding.sessionId); if (!item?.snapshot) return;
            item.snapshot = { ...item.snapshot, phase: 'finished', reason: { kind: 'closed_by_user' } };
            // Deliberately send state before the output barrier to exercise cross-channel ordering.
            item.events.onmessage({ type: 'state', snapshot: item.snapshot });
            const end = () => item.events.onmessage({ type: 'output_end', sequence: String(item.sequence), bytes: String(item.bytes), truncated: false });
            if (options.outputEndDelayMs) setTimeout(end, options.outputEndDelayMs); else end();
            if (options.repeatFinishedState) item.events.onmessage({ type: 'state', snapshot: item.snapshot });
            return;
          }
          case 'forget_session':
            if (forgetFailures-- > 0) throw { code: 'control_busy', message: '宿主繁忙，请重试关闭' };
            evidence.forgotten.push(args.binding.sessionId); sessions.delete(args.binding.sessionId); return;
          case 'load_config': return document;
          case 'save_terminal': document.config.terminal = args.terminal; document.revision += '1'; return document;
          case 'save_workspace': evidence.workspace = args.workspace; return;
          case 'read_clipboard':
            if (options.clipboardDelayMs) await new Promise(resolve => setTimeout(resolve, options.clipboardDelayMs));
            evidence.clipboardReads++; return evidence.clipboard;
          case 'write_clipboard': evidence.clipboard = args.text; return;
          case 'diagnostics': return 'Component harness. Native behavior is covered separately.';
          default: throw new Error(`Unexpected harness command: ${command}`);
        }
      },
    };
  }, options);
}
