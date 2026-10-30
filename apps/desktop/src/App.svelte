<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { Bootstrap, ConfigDocument, Snapshot } from './contracts/models';
  import { errorMessage } from './contracts/models';
  import { host } from './transport/tauri';
  import { TerminalView, fontKey, type FontMetrics } from './terminal/view';
  import { RendererCoordinator } from './terminal/renderer';
  import { shortcutAction } from './state/shortcuts';
  import TerminalPane from './ui/TerminalPane.svelte';
  import PaneDivider from './ui/PaneDivider.svelte';
  import SplitMenu from './ui/SplitMenu.svelte';
  import { createPane, createTab, workspaceSnapshot, type Pane, type Tab, type SplitDirection } from './state/workspace';
  import { geometry, fitsSplit, splitLeaf, removeLeaf, replaceLeaf, closestPane, focusNeighbor, minimumPane, rectStyle, MAX_TAB_PANES, type FocusDirection } from './state/layout';
  import ConfirmDialog from './ui/ConfirmDialog.svelte';
  import Settings from './ui/Settings.svelte';
  import WindowControls from './ui/WindowControls.svelte';
  import appIcon from './assets/terminder.svg';
  interface Confirmation { title: string; message: string; preview?: string; accept?: string; dangerous?: boolean; resolve: (answer: boolean) => void }
  let boot = $state<Bootstrap | null>(null); let doc = $state<ConfigDocument | null>(null);
  let tabs = $state<Tab[]>([]); let activeId = $state<string | null>(null); let busy = $state(false); let loading = $state(true);
  let globalError = $state(''); let notice = $state(''); let showProfiles = $state(false); let settings = $state(false);
  let searching = $state(false); let searchText = $state(''); let searchResult = $state(''); let searchInput = $state<HTMLInputElement>();
  let confirm = $state<Confirmation | null>(null); let renaming = $state<string | null>(null); let renameText = $state('');
  // Terminal instances are imperative resources and intentionally outside Svelte state.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const views = new Map<string, TerminalView>(); const renderer = new RendererCoordinator();
  // In-flight commands are imperative guards, not rendered application state.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const removing = new Set<string>();
  let profileTrigger = $state<HTMLButtonElement>();
  let profileMenu = $state<HTMLDivElement>();
  let profilePosition = $state({ left: 0, top: 0 });
  let workspaceTimer: ReturnType<typeof setTimeout> | null = null;
  const phaseLabels: Record<string, string> = { preparing: '准备中', starting: '启动中', running: '运行中', draining: '排空输出', closing: '关闭中', finalizing: '回收资源', finished: '已结束' };
  let activeTab = $derived(tabs.find(t => t.id === activeId) ?? null);
  let active = $derived(activeTab?.panes.find(p => p.id === activeTab.activePaneId) ?? null);
  let contentSize = $state({ width: 0, height: 0 });
  let paneCount = $derived(tabs.reduce((count, tab) => count + tab.panes.length, 0));
  let atPaneLimit = $derived(paneCount >= (doc?.config.session.max_sessions ?? 0));
  let fontMetrics = $state<FontMetrics | null>(null);
  let minimum = $derived(minimumPane(fontMetrics ?? { width: 9, height: 16 }));
  let layouts = $derived(Object.fromEntries(tabs.map(tab => [tab.id, geometry(tab.layout, contentSize, minimum)])));
  function splitReason(tab: Tab | null, pane: Pane | null, direction: SplitDirection): string {
    if (!tab || !pane) return '请先打开终端';
    if (tab.panes.length >= MAX_TAB_PANES) return `每个标签最多 ${MAX_TAB_PANES} 个窗格`;
    if (atPaneLimit) return '已达到会话数量上限，请先关闭不需要的窗格';
    if (busy || tab.closing || tab.pending || pane.closing) return '请等待当前操作完成';
    if (!pane.snapshot || pane.forgotten || !doc || fontMetrics?.font !== fontKey(doc.config)) return '正在准备终端尺寸';
    if (!fitsSplit(layouts[tab.id]?.panes[pane.id], direction, minimum)) return `${direction === 'columns' ? '向右' : '向下'}空间不足，每格至少保留 40 列 × 10 行`;
    return '';
  }
  function moveFocus(direction: FocusDirection) {
    if (!activeTab || !active) return;
    const id = focusNeighbor(layouts[activeTab.id].panes, active.id, direction);
    const pane = activeTab.panes.find(p => p.id === id);
    if (pane) selectPane(activeTab, pane);
  }
  let appTheme = $derived(doc?.config.terminal.theme ?? 'dark');
  function paneTitle(pane: Pane): string { return pane.programTitle || pane.snapshot?.profileName || doc?.config.profiles.find(p => p.id === pane.profile)?.name || '终端'; }
  function title(tab: Tab): string { return tab.userTitle ?? paneTitle(tab.panes.find(p => p.id === tab.activePaneId) ?? tab.panes[0]); }
  function measureContent(node: HTMLElement) {
    const measure = () => { contentSize = { width: node.clientWidth, height: node.clientHeight }; };
    const observer = new ResizeObserver(measure); measure();
    observer.observe(node); return { destroy: () => observer.disconnect() };
  }
  function focusActive() { if (active && !settings && !confirm && !searching && !renaming) views.get(active.id)?.focus(); }
  function resetSearch() { if (active) views.get(active.id)?.clearSearch(); searching = false; searchResult = ''; }
  function selectPane(tab: Tab, pane: Pane, focus = true) {
    if (tab.id !== activeId || tab.activePaneId !== pane.id) {
      resetSearch(); activeId = tab.id; tab.activePaneId = pane.id; pane.activity = false; saveWorkspaceSoon();
    }
    if (focus) void tick().then(focusActive);
  }
  function zoomPane(tab: Tab, pane: Pane) {
    selectPane(tab, pane, false); tab.zoomed = !tab.zoomed; void tick().then(focusActive);
  }
  function ask(question: Omit<Confirmation, 'resolve'>): Promise<boolean> {
    if (confirm) return Promise.resolve(false);
    return new Promise(resolve => { confirm = { ...question, resolve }; });
  }
  function answer(value: boolean) { const pending = confirm; confirm = null; pending?.resolve(value); void tick().then(focusActive); }
  function select(id: string, focus = true) { resetSearch(); activeId = id; if (active) active.activity = false; saveWorkspaceSoon(); if (focus) void tick().then(focusActive); }
  function closeProfiles(restoreFocus = false) {
    if (!showProfiles) return;
    showProfiles = false;
    if (restoreFocus) profileTrigger?.focus();
  }
  async function toggleProfiles(last = false) {
    if (showProfiles) { closeProfiles(true); return; }
    if (busy || !doc) return;
    showProfiles = true;
    await tick();
    if (!showProfiles || !profileMenu || !profileTrigger) return;
    const anchor = profileTrigger.getBoundingClientRect();
    profilePosition = { left: Math.max(12, Math.min(anchor.left, window.innerWidth - profileMenu.offsetWidth - 12)), top: anchor.bottom + 8 };
    const items = profileMenu.querySelectorAll<HTMLButtonElement>('button:not(:disabled)');
    items[last ? items.length - 1 : 0]?.focus();
  }
  function profileKeys(event: KeyboardEvent) {
    if (event.key === 'Tab') { closeProfiles(true); return; }
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); closeProfiles(true); return; }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key) || !profileMenu) return;
    event.preventDefault(); event.stopPropagation();
    const items = [...profileMenu.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length;
    items[next]?.focus();
  }
  function dismissProfiles(event: PointerEvent) {
    if (event.target instanceof Node && !profileMenu?.contains(event.target) && !profileTrigger?.contains(event.target)) closeProfiles();
  }
  async function newTab(profile = 'default', cwd: string | null = null, customTitle: string | null = null) {
    if (busy || !doc || !host.available()) return;
    // A failed restart may retain a finished view after its native session was forgotten.
    if (atPaneLimit) { globalError = '已达到会话数量上限，请先关闭不需要的窗格。'; return; }
    busy = true; showProfiles = false; globalError = '';
    try {
      const binding = await host.prepare(profile, cwd, active?.binding.sessionId ?? null);
      const tab = createTab(createPane(binding, profile), customTitle);
      resetSearch(); tabs.push(tab); activeId = tab.id; return tab.id;
    } catch (error) { globalError = errorMessage(error); } finally { busy = false; }
  }
  async function split(direction: SplitDirection, tab = activeTab, source = active) {
    if (!tab || !source) return;
    const reason = splitReason(tab, source, direction);
    if (reason) { notice = reason; return; }
    busy = true; tab.pending = true; showProfiles = false; globalError = '';
    try {
      const binding = await host.prepare(source.profile, null, source.binding.sessionId);
      const pane = createPane(binding, source.profile);
      if (activeId === tab.id) resetSearch();
      tab.layout = splitLeaf(tab.layout, source.id, direction, pane.id, crypto.randomUUID());
      tab.zoomed = false; tab.panes.push(pane); tab.activePaneId = pane.id;
    } catch (error) { globalError = errorMessage(error); }
    finally { busy = false; tab.pending = false; }
  }
  function updateState(tab: Tab, pane: Pane, snapshot: Snapshot) {
    pane.snapshot = snapshot; pane.profile = snapshot.profileId;
    if (snapshot.cleanupError) pane.error = snapshot.cleanupError.message;
    if (snapshot.phase === 'running') {
      if (snapshot.startupCwd === null) notice = '启动目录不能无损保存，此窗格不会写入恢复快照。';
      saveWorkspaceSoon();
    }
    if (pane.closing && snapshot.phase === 'finished' && pane.outputComplete) void removePane(tab, pane);
  }
  async function removePane(tab: Tab, pane: Pane): Promise<boolean> {
    if (removing.has(pane.id) || !tab.panes.includes(pane)) return false;
    removing.add(pane.id);
    try {
      if (!pane.forgotten) { await host.forget(pane.binding); pane.forgotten = true; }
      const index = tab.panes.indexOf(pane);
      if (index < 0) return false;
      const focused = active?.id === pane.id;
      if (focused) resetSearch();
      const removed = removeLeaf(tab.layout, pane.id);
      const next = closestPane(layouts[tab.id].panes, pane.id, removed.neighbors);
      tab.panes.splice(index, 1);
      if (removed.layout) tab.layout = removed.layout;
      if (tab.activePaneId === pane.id) { tab.activePaneId = next ?? tab.panes[0]?.id ?? ''; tab.zoomed = false; }
      if (tab.panes.length < 2) tab.zoomed = false;
      if (!tab.panes.length) {
        const tabIndex = tabs.indexOf(tab); tabs.splice(tabIndex, 1);
        if (activeId === tab.id) activeId = tabs[Math.min(tabIndex, tabs.length - 1)]?.id ?? null;
      }
      saveWorkspaceSoon(); if (focused) void tick().then(focusActive);
      return true;
    } catch (error) {
      pane.closing = false; tab.closing = false; pane.error = errorMessage(error); return false;
    } finally { removing.delete(pane.id); }
  }
  async function stopPane(tab: Tab, pane: Pane) {
    if (pane.closing) return;
    pane.closing = true;
    try {
      if (pane.snapshot?.phase === 'finished') { if (pane.outputComplete) await removePane(tab, pane); return; }
      await host.close(pane.binding);
    } catch (error) { pane.closing = false; tab.closing = false; pane.error = errorMessage(error); }
  }
  async function closePane(tab: Tab, pane: Pane) {
    if (pane.closing || tab.closing || tab.pending) return;
    if (pane.snapshot?.phase !== 'finished' && doc?.config.session.confirm_close_running
      && !await ask({ title: '关闭这个窗格？', message: `“${paneTitle(pane)}”及其受管理程序将结束，其他窗格继续运行。`, accept: '关闭窗格', dangerous: true })) return;
    await stopPane(tab, pane);
  }
  async function closeTab(tab: Tab) {
    if (tab.closing || tab.pending) return;
    if (tab.panes.some(p => p.snapshot?.phase !== 'finished') && doc?.config.session.confirm_close_running) {
      const multiple = tab.panes.length > 1;
      if (!await ask({ title: multiple ? '关闭这个标签页？' : '关闭这个终端？',
        message: multiple ? `“${title(tab)}”内的 ${tab.panes.length} 个窗格及其受管理程序将全部结束，未保存的工作可能丢失。` : `“${title(tab)}”及其受管理程序将结束，未保存的工作可能丢失。`,
        accept: multiple ? '关闭标签页' : '关闭终端', dangerous: true })) return;
    }
    tab.closing = true;
    await Promise.all([...tab.panes].map(pane => stopPane(tab, pane)));
  }
  async function restart(tab: Tab, pane: Pane) {
    if (busy || tab.pending || tab.closing || pane.closing || !pane.outputComplete) return;
    busy = true; tab.pending = true; globalError = '';
    try {
      // Keep the finished pane if prepare fails; a retry must not forget the old binding twice.
      if (!pane.forgotten) { await host.forget(pane.binding); pane.forgotten = true; saveWorkspaceSoon(); }
      const binding = await host.prepare(pane.profile, pane.snapshot?.startupCwd ?? null, null);
      const next = createPane(binding, pane.profile);
      if (active?.id === pane.id) resetSearch();
      tab.layout = replaceLeaf(tab.layout, pane.id, { kind: 'pane', id: next.id });
      tab.panes[tab.panes.indexOf(pane)] = next;
      if (tab.activePaneId === pane.id) tab.activePaneId = next.id;
    } catch (error) { pane.error = errorMessage(error); }
    finally { busy = false; tab.pending = false; }
  }
  function saveWorkspaceSoon() {
    if (!host.available()) return;
    if (workspaceTimer) clearTimeout(workspaceTimer);
    workspaceTimer = setTimeout(() => {
      void host.saveWorkspace(workspaceSnapshot(tabs, activeId)).catch(error => notice = `工作区未保存：${errorMessage(error)}`);
    }, 300);
  }
  async function openSettings() {
    showProfiles = false;
    if (!doc) return;
    if (host.available()) { try { doc = await host.config(); } catch (error) { globalError = errorMessage(error); } }
    settings = true;
  }
  function saved(next: ConfigDocument) { doc = next; if (boot) boot.document = next; }
  async function copy() { const selection = active ? views.get(active.id)?.selection() : ''; if (selection) { try { await host.clipboardWrite(selection); } catch (error) { globalError = errorMessage(error); } } }
  async function paste() {
    const id = active?.id;
    if (!id) return;
    const view = views.get(id);
    if (!view) return;
    try {
      const text = await host.clipboardRead();
      // Reading the OS clipboard may wait. Never redirect a pending paste to another Shell.
      if (active?.id === id && views.get(id) === view) await view.paste(text);
    } catch (error) { globalError = errorMessage(error); }
  }
  async function openSearch() { searching = true; await tick(); searchInput?.focus(); searchInput?.select(); }
  function find(previous = false) { const result = active && views.get(active.id)?.find(searchText, previous); searchResult = searchText ? (result ? '已定位' : '未找到') : ''; }
  function closeSearch() { resetSearch(); focusActive(); }
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
      case 'close_tab': if (activeTab) void closeTab(activeTab); break;
      case 'close_pane': if (activeTab && active) void closePane(activeTab, active); break;
      case 'split_right': void split('columns'); break;
      case 'split_down': void split('rows'); break;
      case 'focus_left': case 'focus_right': case 'focus_up': case 'focus_down': moveFocus(action.slice(6) as FocusDirection); break;
      case 'toggle_pane_zoom': if (activeTab && active && activeTab.panes.length > 1) zoomPane(activeTab, active); break;
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
      if (!host.available() || !tabs.some(t => t.panes.some(p => !p.snapshot || p.closing))) return;
      void host.list().then(snapshots => {
        if (stopped) return;
        for (const tab of tabs) {
          for (const pane of tab.panes) {
            const snapshot = snapshots.find(s => s.binding.sessionId === pane.binding.sessionId);
            if (snapshot) {
              if (!pane.snapshot && snapshot.phase === 'finished') pane.outputComplete = true;
              updateState(tab, pane, snapshot);
            }
          }
        }
      }).catch(error => notice = errorMessage(error));
    }, 1000);
    return () => { stopped = true; clearInterval(timer); if (workspaceTimer) clearTimeout(workspaceTimer); renderer.dispose(); confirm?.resolve(false); };
  });
</script>

<svelte:window onkeydown={event => { if (!event.defaultPrevented) shortcut(event, false); }} onpointerdown={dismissProfiles} onresize={() => closeProfiles(true)} />
<div class="app" data-theme={appTheme}>
  <header class="topbar" data-tauri-drag-region="deep">
    <div class="brand" aria-label="Terminder"><img src={appIcon} width="26" height="26" alt="" draggable="false" /><span>terminder</span></div>
    <div class="topbar-center">LOCAL WORKSPACE</div>
    <div class="topbar-actions" data-tauri-drag-region="false"><button class="icon-button" disabled={!active} title="搜索终端 · Ctrl+Shift+F" aria-label="搜索终端" onclick={() => void openSearch()}><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 4 4"/></svg></button><button class="icon-button" disabled={!doc} title="设置 · Ctrl+," aria-label="设置" onclick={() => void openSettings()}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16M4 17h16"/><circle cx="8" cy="7" r="2"/><circle cx="16" cy="17" r="2"/></svg></button><WindowControls failed={message => globalError = message} /></div>
  </header>
  <div class="tabbar">
    <div class="tabs" role="tablist" aria-label="终端标签">
      {#each tabs as tab (tab.id)}
        <div class="tab" class:active={activeId === tab.id}>
          {#if renaming === tab.id}<input class="tab-rename" aria-label="标签名称" bind:value={renameText} maxlength="80" onblur={() => commitRename(tab)} onkeydown={event => { if (event.key === 'Enter') commitRename(tab); if (event.key === 'Escape') renaming = null; }} use:focusInput />
          {:else}<button role="tab" id={`tab-${tab.id}`} aria-selected={activeId === tab.id} aria-controls={`pane-${tab.id}`} tabindex={activeId === tab.id ? 0 : -1} class="tab-select" onclick={() => select(tab.id)} ondblclick={() => rename(tab)} onkeydown={event => { if (event.key === 'F2') rename(tab); if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') { event.preventDefault(); const i = tabs.findIndex(t => t.id === tab.id); const next = tabs[(i + (event.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length]; select(next.id, false); document.getElementById(`tab-${next.id}`)?.focus(); } }}><span class="tab-symbol" aria-hidden="true">❯</span><span class="tab-label">{title(tab)}</span>{#if tab.panes.some(p => p.activity)}<span class="activity-dot" aria-label="有新输出"></span>{/if}</button>{/if}
          <button class="tab-close" aria-label={`关闭 ${title(tab)}`} disabled={tab.closing || tab.pending} onclick={() => void closeTab(tab)}>×</button>
        </div>
      {/each}
    </div>
    <div class="new-tab-group">
      <button bind:this={profileTrigger} class="new-tab" aria-label="新建终端" title="选择终端类型 · Ctrl+Shift+T 直接新建默认终端" aria-haspopup="menu" aria-expanded={showProfiles} aria-controls="terminal-profile-menu" disabled={busy || !doc} onclick={() => void toggleProfiles()} onkeydown={event => { if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); void toggleProfiles(event.key === 'ArrowUp'); } }}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 4v16M4 12h16"/></svg></button>
    </div>
    <SplitMenu id="global-split-menu" targetId={active?.id ?? ''} disabled={!activeTab || busy || activeTab.closing || activeTab.pending} columnsReason={splitReason(activeTab, active, 'columns')} rowsReason={splitReason(activeTab, active, 'rows')} shortcuts={doc?.config.shortcuts} split={direction => void split(direction)} />
    {#if showProfiles && doc}
      <div bind:this={profileMenu} id="terminal-profile-menu" class="profile-menu" role="menu" aria-label="选择终端类型" tabindex="-1" style={`left: ${profilePosition.left}px; top: ${profilePosition.top}px; max-height: calc(100dvh - ${profilePosition.top + 12}px)`} onkeydown={profileKeys}>
        <div class="eyebrow">选择终端类型</div>
        {#each doc.config.profiles as profile (profile.id)}
          <button role="menuitem" tabindex="-1" aria-label={profile.name} disabled={busy} onclick={() => void newTab(profile.id)}><span>{profile.name}</span><small>{boot?.profiles.find(p => p.id === profile.id)?.available ? '本机' : '未发现 / 自定义'}</small></button>
        {/each}
        <button role="menuitem" tabindex="-1" class="profile-settings" onclick={() => void openSettings()}>管理终端配置 <span aria-hidden="true">↗</span></button>
      </div>
    {/if}
  </div>
  {#if globalError}<div class="banner error" role="alert"><span>{globalError}</span>{#if doc}<button onclick={() => void openSettings()}>打开设置</button>{/if}<button class="icon-button" aria-label="关闭提示" onclick={() => globalError = ''}>×</button></div>{/if}
  <main use:measureContent>
    {#if tabs.length && doc}
      {#each tabs as tab (tab.id)}
        <div id={`pane-${tab.id}`} role="tabpanel" tabindex="-1" aria-labelledby={`tab-${tab.id}`} class="tab-content" class:split={tab.panes.length > 1} class:zoomed={tab.zoomed} class:inactive={tab.id !== activeId} aria-hidden={tab.id !== activeId} inert={tab.id !== activeId}>
          {#each tab.panes as pane, index (pane.id)}
            {@const visible = tab.id === activeId && (!tab.zoomed || pane.id === tab.activePaneId)}
            <section id={`session-${pane.id}`} class="session-pane" class:focused={pane.id === tab.activePaneId} class:zoom-hidden={tab.zoomed && pane.id !== tab.activePaneId} aria-label={`窗格 ${index + 1}：${paneTitle(pane)}`} aria-hidden={!visible} inert={!visible} onfocusin={() => selectPane(tab, pane, false)} onpointerdown={() => selectPane(tab, pane, false)} style={rectStyle(tab.zoomed && pane.id === tab.activePaneId ? { x: 0, y: 0, ...contentSize } : layouts[tab.id].panes[pane.id])}>
              {#if tab.panes.length > 1}
                <div class="pane-toolbar">
                  <button class="pane-name" title={paneTitle(pane)} aria-label={`聚焦窗格 ${index + 1}`} aria-pressed={pane.id === tab.activePaneId} onclick={() => selectPane(tab, pane)}><span class="pane-number">{index + 1}</span><span>{paneTitle(pane)}</span>{#if pane.id === tab.activePaneId}<span class="current-pane">当前</span>{/if}{#if pane.activity}<span class="activity-dot" aria-label="有新输出"></span>{/if}</button>
                  <SplitMenu id={`split-menu-${pane.id}`} targetId={pane.id} label={`分屏窗格 ${index + 1}`} disabled={tab.id !== activeId || busy || tab.closing || tab.pending || pane.closing} columnsReason={splitReason(tab, pane, 'columns')} rowsReason={splitReason(tab, pane, 'rows')} shortcuts={doc.config.shortcuts} split={direction => void split(direction, tab, pane)} />
                  <button class="icon-button" aria-label={tab.zoomed ? '还原分屏' : `最大化窗格 ${index + 1}`} title={tab.zoomed ? '还原分屏' : '最大化窗格'} onclick={() => zoomPane(tab, pane)}><svg viewBox="0 0 24 24" aria-hidden="true">{#if tab.zoomed}<path d="M8 8V4h12v12h-4"/><rect x="4" y="8" width="12" height="12" rx="1"/>{:else}<rect x="4" y="4" width="16" height="16" rx="1"/>{/if}</svg></button>
                  <button class="icon-button" aria-label={`关闭窗格 ${index + 1}`} title="关闭窗格" disabled={pane.closing || tab.closing || tab.pending} onclick={() => void closePane(tab, pane)}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m4 4 16 16M4 20 20 4"/></svg></button>
                </div>
              {/if}
              <TerminalPane binding={pane.binding} {visible} active={visible && pane.id === tab.activePaneId} config={doc.config} {renderer} ready={view => { if (view) { views.set(pane.id, view); if (active?.id === pane.id) void tick().then(focusActive); } else views.delete(pane.id); }} callbacks={{
                state: snapshot => updateState(tab, pane, snapshot), error: message => pane.error = message,
                activity: () => { if (tab.id !== activeId || (tab.zoomed && pane.id !== tab.activePaneId)) pane.activity = true; }, title: name => pane.programTitle = name,
                size: size => pane.size = size, metrics: metrics => { if (metrics.font === fontKey(doc!.config)) fontMetrics = metrics; }, renderer: mode => pane.renderer = mode,
                complete: truncated => { pane.outputComplete = true; pane.tailTruncated = truncated; if (pane.closing && pane.snapshot?.phase === 'finished') void removePane(tab, pane); },
                shortcut: event => shortcut(event), confirmPaste: (text, reason) => ask({ title: '确认粘贴', message: reason, preview: text.length > 4000 ? `${text.slice(0, 4000)}\n…（预览已截短）` : text, accept: '粘贴', dangerous: true }), link: url => void openLink(url),
              }} />
              {#if pane.error}<div class="session-message" role="alert"><span>{pane.error}</span><button aria-label="关闭会话提示" onclick={() => pane.error = ''}>×</button></div>{/if}
              {#if pane.snapshot?.phase === 'finished'}<div class="finished-bar"><span><span class="status-dot"></span> 会话已结束{pane.snapshot.exitCode !== null ? ` · 退出码 ${pane.snapshot.exitCode}` : ''}{pane.tailTruncated || !pane.snapshot.tailComplete ? ' · 尾部输出不完整' : ''}</span><button disabled={busy || pane.closing || tab.closing || !pane.outputComplete} onclick={() => void restart(tab, pane)}>重新启动 <span>↗</span></button></div>{/if}
            </section>
          {/each}
          {#if !tab.zoomed}
            {#each layouts[tab.id].dividers as divider (divider.node.id)}
              <PaneDivider {divider} label={`调整窗格 ${divider.controls.map(id => tab.panes.findIndex(p => p.id === id) + 1).join('、')} 大小`} resize={ratio => divider.node.ratio = ratio} />
            {/each}
          {/if}
        </div>
      {/each}
    {:else}
      <div class="welcome"><div class="welcome-glyph" aria-hidden="true">❯<span>_</span></div><div class="eyebrow">YOUR NEXT COMMAND STARTS HERE</div><h1>留一处空间，专注眼前。</h1><p>{loading ? '正在准备本机环境…' : '打开一个终端，继续你的工作。'}</p><button class="primary start-button" disabled={loading || busy || !doc} onclick={() => void newTab()}><span>+</span> 新建终端 <kbd>Ctrl Shift T</kbd></button><div class="welcome-details"><span>本机 Shell</span><i>·</i><span>独立会话</span><i>·</i><span>你的工作方式</span></div></div>
    {/if}
    {#if searching}<div class="search-bar"><label class="sr-only" for="terminal-search">搜索当前终端</label><input id="terminal-search" placeholder="搜索当前终端…" bind:this={searchInput} bind:value={searchText} oninput={() => find()} onkeydown={event => { if (event.key === 'Enter') find(event.shiftKey); if (event.key === 'Escape') closeSearch(); }} /><span>{searchResult}</span><button aria-label="上一个匹配" onclick={() => find(true)}>↑</button><button aria-label="下一个匹配" onclick={() => find()}>↓</button><button aria-label="关闭搜索" onclick={closeSearch}>×</button></div>{/if}
  </main>
  <footer class="statusbar"><div class="status-left"><span class="connection-dot" class:connected={active?.snapshot?.phase === 'running'}></span><span>{active?.snapshot?.profileName ?? (host.available() ? '本机工作区' : '桌面未连接')}</span>{#if active?.snapshot?.startupCwd}<span class="status-divider">/</span><span class="cwd" title={active.snapshot.startupCwd}>启动目录 · {active.snapshot.startupCwd}</span>{/if}</div><div class="status-right">{#if active?.size}<span>{active.size.cols} × {active.size.rows}</span><span class="status-divider">/</span>{/if}{#if active}<span>{active.renderer === 'webgl' ? 'WebGL' : 'DOM'}</span><span class="status-divider">/</span><span>{active.snapshot?.congested ? '输出等待消费' : phaseLabels[active.snapshot?.phase ?? 'preparing']}</span>{:else}<span>TERMINDER 0.1.0</span>{/if}</div></footer>
  {#if notice}<div class="toast" role="status"><span>{notice}</span><button aria-label="关闭通知" onclick={() => notice = ''}>×</button></div>{/if}
  {#if settings && doc}<Settings document={doc} {saved} close={() => { settings = false; focusActive(); }} />{/if}
  {#if confirm}<ConfirmDialog title={confirm.title} message={confirm.message} preview={confirm.preview} accept={confirm.accept} dangerous={confirm.dangerous} {answer} />{/if}
</div>

<script lang="ts" module>
  function focusInput(node: HTMLInputElement) { node.focus(); node.select(); }
</script>
