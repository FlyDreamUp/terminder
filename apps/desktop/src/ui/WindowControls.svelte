<script lang="ts">
  import { onMount } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { errorMessage } from '../contracts/models';

  let { failed }: { failed: (message: string) => void } = $props();
  const desktop = isTauri();
  let maximized = $state(false);
  let busy = $state(false);
  let stopped = false;
  let revision = 0;

  async function syncMaximized() {
    const current = ++revision;
    try {
      const value = await getCurrentWindow().isMaximized();
      if (!stopped && current === revision) maximized = value;
    } catch (error) { if (!stopped) failed(errorMessage(error)); }
  }

  async function act(action: 'minimize' | 'maximize' | 'close') {
    if (busy || stopped) return;
    busy = true;
    try {
      const window = getCurrentWindow();
      if (action === 'minimize') await window.minimize();
      else if (action === 'maximize') { await window.toggleMaximize(); await syncMaximized(); }
      // close() emits CloseRequested so the native confirmation and session cleanup still run.
      else await window.close();
    } catch (error) { if (!stopped) failed(errorMessage(error)); }
    finally { if (!stopped) busy = false; }
  }

  onMount(() => {
    if (!desktop) return;
    let unlisten: (() => void) | undefined;
    void getCurrentWindow().onResized(() => { void syncMaximized(); })
      .then(stop => { if (stopped) stop(); else unlisten = stop; })
      .catch(error => { if (!stopped) failed(errorMessage(error)); });
    void syncMaximized();
    return () => { stopped = true; unlisten?.(); };
  });
</script>

{#if desktop}
  <div class="window-controls" data-tauri-drag-region="false" role="group" aria-label="窗口控制">
    <button class="window-button" aria-label="最小化窗口" title="最小化" disabled={busy} onclick={() => void act('minimize')}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 8.5h10" /></svg>
    </button>
    <button class="window-button" aria-label={maximized ? '还原窗口' : '最大化窗口'} title={maximized ? '还原' : '最大化'} disabled={busy} onclick={() => void act('maximize')}>
      <svg viewBox="0 0 16 16" aria-hidden="true">
        {#if maximized}<path d="M5.5 5.5v-3h8v8h-3m-8-5h8v8h-8z" />
        {:else}<path d="M3.5 3.5h9v9h-9z" />{/if}
      </svg>
    </button>
    <button class="window-button window-close" aria-label="关闭窗口" title="关闭" disabled={busy} onclick={() => void act('close')}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="m3.5 3.5 9 9m0-9-9 9" /></svg>
    </button>
  </div>
{/if}

<style>
  .window-controls { display: flex; align-self: stretch; margin-left: 12px; }
  .window-button { display: grid; place-items: center; width: 46px; padding: 0; border: 0; border-radius: 0; background: transparent; color: var(--muted); }
  .window-button:hover:enabled { background: var(--hover); color: var(--text); }
  .window-button:focus-visible { outline-offset: -4px; }
  .window-button svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 1; }
  .window-close:hover:enabled { background: #c9404c; color: #fff; }
</style>
