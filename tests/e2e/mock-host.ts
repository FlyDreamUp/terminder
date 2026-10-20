// Browser component harness only. This file is outside the production Vite entry graph.
import type { Page } from '../../apps/desktop/tests/harness';
export async function installHost(page: Page) {
  await page.addInitScript(() => {
    const profile = { id: 'pwsh', name: 'PowerShell', platforms: ['windows'], resolver: 'pwsh', program: null, args: ['-NoLogo'], cwd_policy: 'inherit_known', cwd: null, env: {} };
    const document = { config: { schema_version: 1, default_profile: 'auto', profiles: [profile], terminal: { font_family: ['Consolas', 'monospace'], font_size_px: 14, theme: 'dark', scrollback_lines: 5000, renderer: 'dom' }, window: { restore_geometry: true, restore_tabs: false }, session: { max_sessions: 16, confirm_close_running: true }, clipboard: { confirm_multiline: true, max_paste_bytes: 262144 }, shortcuts: { new_tab: 'Ctrl+Shift+T', close_tab: 'Ctrl+Shift+W', next_tab: 'Ctrl+Tab', previous_tab: 'Ctrl+Shift+Tab', copy: 'Ctrl+Shift+C', paste: 'Ctrl+Shift+V', find: 'Ctrl+Shift+F', settings: 'Ctrl+,', zoom_in: 'Ctrl+=', zoom_out: 'Ctrl+-', zoom_reset: 'Ctrl+0' } }, text: 'schema_version = 1\ndefault_profile = "auto"\n', revision: 'fixture', error: null, writable: true };
    const sessions = new Map<string, any>(); const callbacks = new Map<number, any>(); let callbackId = 0;
    const evidence = { starts: 0, input: [] as number[][], resize: [] as any[], acknowledgments: [] as any[], clipboard: '', workspace: null as any, events: [] as string[] };
    (window as any).__testEvidence = evidence;
    (window as any).isTauri = true;
    (window as any).__TAURI_INTERNALS__ = {
      transformCallback: (callback: any) => { const id = ++callbackId; callbacks.set(id, callback); return id; },
      unregisterCallback: (id: number) => callbacks.delete(id),
      invoke: async (command: string, args: any) => {
        evidence.events.push(command);
        switch (command) {
          case 'bootstrap': return { document, profiles: [{ id: 'pwsh', available: true, error: null }], workspace: { schemaVersion: 1, geometry: null, tabs: [], activeTab: null }, workspaceError: null, version: '0.1.0', platform: 'windows' };
          case 'prepare_session': { const binding = { sessionId: crypto.randomUUID(), viewEpoch: crypto.randomUUID(), protocolVersion: 1 }; sessions.set(binding.sessionId, { binding }); return binding; }
          case 'start_session': {
            evidence.starts++; const item = sessions.get(args.binding.sessionId); item.events = args.events; item.output = args.output;
            item.snapshot = { binding: args.binding, profileId: 'pwsh', profileName: 'PowerShell', startupCwd: 'C:\\Users\\Developer', cwdSource: 'home', phase: 'running', reason: null, exitCode: null, tailComplete: true, congested: false, cleanupError: null, size: args.size };
            const bytes = new TextEncoder().encode('\x1b[38;2;138;215;193mPowerShell\x1b[0m\r\nBrowser component fixture — no native Shell process\r\n\r\nPS C:\\Users\\Developer> \x1b[6n');
            item.sequence = 1; item.bytes = bytes.length; args.output.onmessage(bytes.buffer); setTimeout(() => args.events.onmessage({ type: 'state', snapshot: item.snapshot }), 50); return;
          }
          case 'reserve_input': return crypto.randomUUID();
          case 'input': evidence.input.push([...args]); return;
          case 'output_consumed': evidence.acknowledgments.push(args); return;
          case 'resize_session': evidence.resize.push(args); return;
          case 'list_sessions': return [...sessions.values()].filter(s => s.snapshot).map(s => s.snapshot);
          case 'close_session': case 'view_failed': {
            const item = sessions.get(args.binding.sessionId); if (!item?.snapshot) return;
            item.snapshot = { ...item.snapshot, phase: 'finished', reason: { kind: 'closed_by_user' } };
            // Deliberately send state before the output barrier to exercise cross-channel ordering.
            item.events.onmessage({ type: 'state', snapshot: item.snapshot });
            item.events.onmessage({ type: 'output_end', sequence: String(item.sequence), bytes: String(item.bytes), truncated: false }); return;
          }
          case 'forget_session': sessions.delete(args.binding.sessionId); return;
          case 'load_config': return document;
          case 'save_terminal': document.config.terminal = args.terminal; document.revision += '1'; return document;
          case 'save_workspace': evidence.workspace = args.workspace; return;
          case 'read_clipboard': return evidence.clipboard;
          case 'write_clipboard': evidence.clipboard = args.text; return;
          case 'diagnostics': return 'Component harness. Native behavior is covered separately.';
          default: throw new Error(`Unexpected harness command: ${command}`);
        }
      },
    };
  });
}
