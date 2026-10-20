<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { Binding, Bootstrap, ConfigDocument, Size, Snapshot } from './contracts/models';
  import { errorMessage } from './contracts/models';
  import { host } from './transport/tauri';
  import { TerminalView } from './terminal/view';
  import { RendererCoordinator } from './terminal/renderer';
  import { shortcutAction } from './state/shortcuts';
  import TerminalPane from './ui/TerminalPane.svelte';
  import ConfirmDialog from './ui/ConfirmDialog.svelte';
  import Settings from './ui/Settings.svelte';
  interface Tab { id: string; binding: Binding; profile: string; userTitle: string | null; programTitle: string; snapshot: Snapshot | null; activity: boolean; error: string; size: Size | null; renderer: 'webgl' | 'dom'; outputComplete: boolean; closing: boolean; tailTruncated: boolean }
  interface Confirmation { title: string; message: string; preview?: string; accept?: string; dangerous?: boolean; resolve: (answer: boolean) => void }
  let boot = $state<Bootstrap | null>(null); let doc = $state<ConfigDocument | null>(null);
  let tabs = $state<Tab[]>([]); let activeId = $state<string | null>(null); let busy = $state(false); let loading = $state(true);
  let globalError = $state(''); let notice = $state(''); let showProfiles = $state(false); let settings = $state(false);
  let searching = $state(false); let searchText = $state(''); let searchResult = $state(''); let searchInput = $state<HTMLInputElement>();
  let confirm = $state<Confirmation | null>(null); let renaming = $state<string | null>(null); let renameText = $state('');
  // Terminal instances are imperative resources and intentionally outside Svelte state.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const views = new Map<string, TerminalView>(); const renderer = new RendererCoordinator();
  let workspaceTimer: ReturnType<typeof setTimeout> | null = null;
  const phaseLabels: Record<string, string> = { preparing: '准备中', starting: '启动中', running: '运行中', draining: '排空输出', closing: '关闭中', finalizing: '回收资源', finished: '已结束' };
  let active = $derived(tabs.find(t => t.id === activeId) ?? null);
  let appTheme = $derived(doc?.config.terminal.theme ?? 'dark');
  function title(tab: Tab): string { return tab.userTitle ?? (tab.programTitle || tab.snapshot?.profileName || doc?.config.profiles.find(p => p.id === tab.profile)?.name || '终端'); }
  function ask(question: Omit<Confirmation, 'resolve'>): Promise<boolean> {
    if (confirm) return Promise.resolve(false);
    return new Promise(resolve => { confirm = { ...question, resolve }; });
  }
  function answer(value: boolean) { const pending = confirm; confirm = null; pending?.resolve(value); void tick().then(() => activeId && views.get(activeId)?.focus()); }
  function select(id: string) { activeId = id; const tab = tabs.find(t => t.id === id); if (tab) tab.activity = false; searching = false; saveWorkspaceSoon(); }
  async function newTab(profile = 'default', cwd: string | null = null, customTitle: string | null = null) {
    if (busy || !doc || !host.available()) return; busy = true; showProfiles = false; globalError = '';
    try {
      const binding = await host.prepare(profile, cwd, active?.binding.sessionId ?? null);
      const tab: Tab = { id: crypto.randomUUID(), binding, profile, userTitle: customTitle, programTitle: '', snapshot: null, activity: false, error: '', size: null, renderer: 'dom', outputComplete: false, closing: false, tailTruncated: false };
      tabs.push(tab); activeId = tab.id; return tab.id;
    } catch (error) { globalError = errorMessage(error); } finally { busy = false; }
  }
  function updateState(tab: Tab, snapshot: Snapshot) {
    tab.snapshot = snapshot; tab.profile = snapshot.profileId;
    if (snapshot.cleanupError) tab.error = snapshot.cleanupError.message;
    if (snapshot.phase === 'running') {
      if (snapshot.startupCwd === null) notice = '启动目录不能无损保存，此标签不会写入恢复快照。';
      saveWorkspaceSoon();
    }
    if (tab.closing && snapshot.phase === 'finished' && tab.outputComplete) void remove(tab);
  }
  async function remove(tab: Tab) {
    if (!tabs.some(t => t.id === tab.id)) return;
    try {
      await host.forget(tab.binding);
      const index = tabs.findIndex(t => t.id === tab.id); tabs.splice(index, 1);
      if (activeId === tab.id) activeId = tabs[Math.min(index, tabs.length - 1)]?.id ?? null;
      saveWorkspaceSoon();
    } catch (error) { tab.error = errorMessage(error); }
  }
  async function closeTab(tab: Tab) {
    if (tab.closing) return;
    if (tab.snapshot?.phase !== 'finished' && doc?.config.session.confirm_close_running) {
      if (!await ask({ title: '关闭这个终端？', message: `“${title(tab)}”及其受管理程序将结束，未保存的工作可能丢失。`, accept: '关闭终端', dangerous: true })) return;
    }
    tab.closing = true;
    try {
      if (tab.snapshot?.phase === 'finished') { await remove(tab); return; }
      await host.close(tab.binding);
    } catch (error) { tab.closing = false; tab.error = errorMessage(error); }
  }
  async function restart(tab: Tab) {
    const profile = tab.profile; const cwd = tab.snapshot?.startupCwd ?? null; const name = tab.userTitle;
    await remove(tab); await newTab(profile, cwd, name);
  }
  function saveWorkspaceSoon() {
    if (!host.available()) return;
    if (workspaceTimer) clearTimeout(workspaceTimer);
    workspaceTimer = setTimeout(() => {
      const ready = tabs.filter(t => t.snapshot !== null && t.snapshot.startupCwd !== null);
      void host.saveWorkspace({ schemaVersion: 1, geometry: null, tabs: ready.map(t => ({ id: t.id, profileId: t.snapshot!.profileId, startupCwd: t.snapshot!.startupCwd, title: t.userTitle })), activeTab: ready.some(t => t.id === activeId) ? activeId : ready[0]?.id ?? null }).catch(error => notice = `工作区未保存：${errorMessage(error)}`);
    }, 300);
  }
  async function openSettings() {
    showProfiles = false;
    if (!doc) return;
    if (host.available()) { try { doc = await host.config(); } catch (error) { globalError = errorMessage(error); } }
    settings = true;
  }
  function saved(next: ConfigDocument) { doc = next; if (boot) boot.document = next; }
  async function copy() { const selection = activeId ? views.get(activeId)?.selection() : ''; if (selection) { try { await host.clipboardWrite(selection); } catch (error) { globalError = errorMessage(error); } } }
  async function paste() { if (!activeId) return; try { const text = await host.clipboardRead(); await views.get(activeId)?.paste(text); } catch (error) { globalError = errorMessage(error); } }
  async function openSearch() { searching = true; await tick(); searchInput?.focus(); searchInput?.select(); }
  function find(previous = false) { const result = activeId && views.get(activeId)?.find(searchText, previous); searchResult = searchText ? (result ? '已定位' : '未找到') : ''; }
  function closeSearch() { searching = false; if (activeId) { views.get(activeId)?.clearSearch(); views.get(activeId)?.focus(); } }
  function rename(tab: Tab) { renaming = tab.id; renameText = title(tab); }
  function commitRename(tab: Tab) { tab.userTitle = [...renameText.trim()].slice(0, 80).join('') || null; renaming = null; saveWorkspaceSoon(); }
  async function openLink(url: string) {
    try {
      const parsed = new URL(url);
      if (!['https:', 'http:'].includes(parsed.protocol) || parsed.username || parsed.password) throw new Error('仅允许不含凭据的 HTTP/HTTPS 链接');
      if (await ask({ title: '在浏览器中打开链接？', message: '链接由终端程序提供，请核对目标地址。', preview: url, accept: '打开链接' })) await host.openLink(url);
    } catch (error) { globalError = errorMessage(error); }
  }
  function shortcut(event: KeyboardEvent, terminal = true): boolean {
    if (!doc || settings || confirm || renaming) return false;
    const target = event.target as HTMLElement;
    const editor = !terminal && (target.matches('input, textarea, select') || target.isContentEditable);
    const action = shortcutAction(event, doc.config.shortcuts, editor ? 'editor' : terminal ? 'terminal' : 'app');
    if (!action) return false;
    event.preventDefault(); event.stopPropagation();
    switch (action) {
      case 'new_tab': void newTab(); break;
      case 'close_tab': if (active) void closeTab(active); break;
      case 'next_tab': case 'previous_tab': if (tabs.length) { const index = tabs.findIndex(t => t.id === activeId); select(tabs[(index + (action === 'next_tab' ? 1 : tabs.length - 1)) % tabs.length].id); } break;
      case 'copy': void copy(); break; case 'paste': void paste(); break; case 'find': void openSearch(); break; case 'settings': void openSettings(); break;
      case 'zoom_in': case 'zoom_out': case 'zoom_reset': {
        const size = action === 'zoom_reset' ? 14 : doc.config.terminal.font_size_px + (action === 'zoom_in' ? 1 : -1);
        if (size >= 8 && size <= 48) { doc = { ...doc, config: { ...doc.config, terminal: { ...doc.config.terminal, font_size_px: size } } }; } break;
      }
    }
    return true;
  }
  onMount(() => {
    let stopped = false;
    void (async () => {
      if (!host.available()) { loading = false; globalError = '桌面连接未建立。请通过 Terminder 桌面应用运行本机终端。'; return; }
      try {
        boot = await host.bootstrap(); doc = boot.document;
        if (boot.workspaceError) notice = boot.workspaceError.message;
        if (doc.error && !await ask({ title: '配置需要修复', message: `${doc.error.message}。原文件已保留，是否本次使用临时默认配置？`, accept: '使用临时默认配置' })) { settings = true; return; }
        if (doc.config.window.restore_tabs && boot.workspace.tabs.length) {
          if (await ask({ title: '重建上次的标签？', message: '将按保存的 Profile 和启动目录创建新会话，不恢复原进程、命令或终端历史。Shell 启动脚本可能执行。', accept: '重建标签' })) {
            let restoredActive: string | undefined; const unavailable: string[] = [];
            for (const tab of boot.workspace.tabs.slice(0, doc.config.session.max_sessions)) {
              if (stopped) break;
              const id = await newTab(tab.profileId, tab.startupCwd, tab.title);
              if (!id) unavailable.push(`${tab.title ?? tab.profileId}：${globalError}`);
              if (tab.id === boot.workspace.activeTab) restoredActive = id;
            }
            if (restoredActive) select(restoredActive);
            if (unavailable.length) globalError = `部分标签未恢复。${unavailable.join('；')}`;
          } else await newTab();
        } else await newTab();
      } catch (error) { globalError = errorMessage(error); } finally { loading = false; }
    })();
    // Polling covers prepare/start command failures before a status channel was bound.
    const timer = setInterval(() => {
      if (!host.available() || !tabs.some(t => !t.snapshot || t.closing)) return;
      void host.list().then(snapshots => {
        if (stopped) return;
        for (const tab of tabs) {
          const snapshot = snapshots.find(s => s.binding.sessionId === tab.binding.sessionId);
          if (snapshot) {
            if (!tab.snapshot && snapshot.phase === 'finished') tab.outputComplete = true;
            updateState(tab, snapshot);
          }
        }
      }).catch(error => notice = errorMessage(error));
    }, 1000);
    return () => { stopped = true; clearInterval(timer); if (workspaceTimer) clearTimeout(workspaceTimer); renderer.dispose(); confirm?.resolve(false); };
  });
</script>

<svelte:window onkeydown={event => { if (!event.defaultPrevented) shortcut(event, false); }} />
<div class="app" data-theme={appTheme}>
  <header class="topbar">
    <div class="brand" aria-label="Terminder"><svg width="22" height="22" viewBox="0 0 24 24" aria-hidden="true"><path d="m5 6 6 6-6 6m9 0h6" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" /></svg><span>terminder</span></div>
    <div class="topbar-center">LOCAL WORKSPACE</div>
    <div class="topbar-actions"><button class="icon-button" disabled={!active} title="搜索终端 · Ctrl+Shift+F" aria-label="搜索终端" onclick={() => void openSearch()}><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 4 4"/></svg></button><button class="icon-button" disabled={!doc} title="设置 · Ctrl+," aria-label="设置" onclick={() => void openSettings()}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16M4 17h16"/><circle cx="8" cy="7" r="2"/><circle cx="16" cy="17" r="2"/></svg></button></div>
  </header>
  <div class="tabbar">
    <div class="tabs" role="tablist" aria-label="终端标签">
      {#each tabs as tab (tab.id)}
        <div class="tab" class:active={activeId === tab.id}>
          {#if renaming === tab.id}<input class="tab-rename" aria-label="标签名称" bind:value={renameText} maxlength="80" onblur={() => commitRename(tab)} onkeydown={event => { if (event.key === 'Enter') commitRename(tab); if (event.key === 'Escape') renaming = null; }} use:focusInput />
          {:else}<button role="tab" id={`tab-${tab.id}`} aria-selected={activeId === tab.id} aria-controls={`pane-${tab.id}`} tabindex={activeId === tab.id ? 0 : -1} class="tab-select" onclick={() => select(tab.id)} ondblclick={() => rename(tab)} onkeydown={event => { if (event.key === 'F2') rename(tab); if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') { event.preventDefault(); const i = tabs.findIndex(t => t.id === tab.id); const next = tabs[(i + (event.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length]; select(next.id); document.getElementById(`tab-${next.id}`)?.focus(); } }}><span class="tab-symbol" aria-hidden="true">❯</span><span class="tab-label">{title(tab)}</span>{#if tab.activity}<span class="activity-dot" aria-label="有新输出"></span>{/if}</button>{/if}
          <button class="tab-close" aria-label={`关闭 ${title(tab)}`} disabled={tab.closing} onclick={() => void closeTab(tab)}>×</button>
        </div>
      {/each}
    </div>
    <div class="new-tab-group"><button class="new-tab" aria-label="新建终端" title="新建终端 · Ctrl+Shift+T" disabled={busy || !doc} onclick={() => void newTab()}>+</button><button class="profile-toggle" aria-label="选择 Profile" aria-expanded={showProfiles} disabled={!doc} onclick={() => showProfiles = !showProfiles}>⌄</button></div>
    {#if showProfiles && doc}<div class="profile-menu"><div class="eyebrow">SHELL PROFILES</div>{#each doc.config.profiles as profile (profile.id)}<button onclick={() => void newTab(profile.id)}><span>{profile.name}</span><small>{boot?.profiles.find(p => p.id === profile.id)?.available ? '本机' : '未发现 / 自定义'}</small></button>{/each}<button class="profile-settings" onclick={() => void openSettings()}>管理 Profiles <span>↗</span></button></div>{/if}
  </div>
  {#if globalError}<div class="banner error" role="alert"><span>{globalError}</span>{#if doc}<button onclick={() => void openSettings()}>打开设置</button>{/if}<button class="icon-button" aria-label="关闭提示" onclick={() => globalError = ''}>×</button></div>{/if}
  <main>
    {#if tabs.length && doc}
      {#each tabs as tab (tab.id)}
        <div id={`pane-${tab.id}`} role="tabpanel" tabindex="-1" aria-labelledby={`tab-${tab.id}`} class="session-pane" class:inactive={tab.id !== activeId} aria-hidden={tab.id !== activeId} inert={tab.id !== activeId}>
          <TerminalPane binding={tab.binding} active={tab.id === activeId} config={doc.config} {renderer} ready={view => { if (view) views.set(tab.id, view); else views.delete(tab.id); }} callbacks={{
            state: snapshot => updateState(tab, snapshot), error: message => tab.error = message,
            activity: () => { if (tab.id !== activeId) tab.activity = true; }, title: name => tab.programTitle = name,
            size: size => tab.size = size, renderer: mode => tab.renderer = mode,
            complete: truncated => { tab.outputComplete = true; tab.tailTruncated = truncated; if (tab.closing && tab.snapshot?.phase === 'finished') void remove(tab); },
            shortcut: event => shortcut(event), confirmPaste: (text, reason) => ask({ title: '确认粘贴', message: reason, preview: text.length > 4000 ? `${text.slice(0, 4000)}\n…（预览已截短）` : text, accept: '粘贴', dangerous: true }), link: url => void openLink(url),
          }} />
          {#if tab.error}<div class="session-message" role="alert"><span>{tab.error}</span><button aria-label="关闭会话提示" onclick={() => tab.error = ''}>×</button></div>{/if}
          {#if tab.snapshot?.phase === 'finished'}<div class="finished-bar"><span><span class="status-dot"></span> 会话已结束{tab.snapshot.exitCode !== null ? ` · 退出码 ${tab.snapshot.exitCode}` : ''}{tab.tailTruncated || !tab.snapshot.tailComplete ? ' · 尾部输出不完整' : ''}</span><button disabled={busy} onclick={() => void restart(tab)}>重新启动 <span>↗</span></button></div>{/if}
        </div>
      {/each}
    {:else}
      <div class="welcome"><div class="welcome-glyph" aria-hidden="true">❯<span>_</span></div><div class="eyebrow">YOUR NEXT COMMAND STARTS HERE</div><h1>留一处空间，专注眼前。</h1><p>{loading ? '正在准备本机环境…' : '打开一个终端，继续你的工作。'}</p><button class="primary start-button" disabled={loading || busy || !doc} onclick={() => void newTab()}><span>+</span> 新建终端 <kbd>Ctrl Shift T</kbd></button><div class="welcome-details"><span>本机 Shell</span><i>·</i><span>独立会话</span><i>·</i><span>你的工作方式</span></div></div>
    {/if}
    {#if searching}<div class="search-bar"><label class="sr-only" for="terminal-search">搜索当前终端</label><input id="terminal-search" placeholder="搜索当前终端…" bind:this={searchInput} bind:value={searchText} oninput={() => find()} onkeydown={event => { if (event.key === 'Enter') find(event.shiftKey); if (event.key === 'Escape') closeSearch(); }} /><span>{searchResult}</span><button aria-label="上一个匹配" onclick={() => find(true)}>↑</button><button aria-label="下一个匹配" onclick={() => find()}>↓</button><button aria-label="关闭搜索" onclick={closeSearch}>×</button></div>{/if}
  </main>
  <footer class="statusbar"><div class="status-left"><span class="connection-dot" class:connected={active?.snapshot?.phase === 'running'}></span><span>{active?.snapshot?.profileName ?? (host.available() ? '本机工作区' : '桌面未连接')}</span>{#if active?.snapshot?.startupCwd}<span class="status-divider">/</span><span class="cwd" title={active.snapshot.startupCwd}>启动目录 · {active.snapshot.startupCwd}</span>{/if}</div><div class="status-right">{#if active?.size}<span>{active.size.cols} × {active.size.rows}</span><span class="status-divider">/</span>{/if}{#if active}<span>{active.renderer === 'webgl' ? 'WebGL' : 'DOM'}</span><span class="status-divider">/</span><span>{active.snapshot?.congested ? '输出等待消费' : phaseLabels[active.snapshot?.phase ?? 'preparing']}</span>{:else}<span>TERMINDER 0.1.0</span>{/if}</div></footer>
  {#if notice}<div class="toast" role="status"><span>{notice}</span><button aria-label="关闭通知" onclick={() => notice = ''}>×</button></div>{/if}
  {#if settings && doc}<Settings document={doc} {saved} close={() => { settings = false; if (activeId) views.get(activeId)?.focus(); }} />{/if}
  {#if confirm}<ConfirmDialog title={confirm.title} message={confirm.message} preview={confirm.preview} accept={confirm.accept} dangerous={confirm.dangerous} {answer} />{/if}
</div>

<script lang="ts" module>
  function focusInput(node: HTMLInputElement) { node.focus(); node.select(); }
</script>
