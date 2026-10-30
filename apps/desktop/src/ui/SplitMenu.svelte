<script lang="ts">
  import { tick } from 'svelte';
  import type { SplitDirection } from '../state/workspace';
  let { disabled, columnsReason, rowsReason, split, id, label = '分屏', targetId, shortcuts = {} }: {
    disabled: boolean; columnsReason: string; rowsReason: string; id: string; label?: string; targetId: string;
    shortcuts?: Record<string, string>; split: (direction: SplitDirection) => void;
  } = $props();
  let open = $state(false);
  let trigger: HTMLButtonElement;
  let menu = $state<HTMLDivElement>();
  let position = $state({ left: 0, top: 0 });
  $effect(() => { if (disabled) open = false; });
  $effect(() => { void targetId; open = false; });
  let explanation = $derived([...new Set([columnsReason, rowsReason].filter(Boolean))].join('；'));
  function close(focus = false) { open = false; if (focus) trigger.focus(); }
  async function toggle() {
    if (open) { close(true); return; }
    open = true; await tick();
    if (!open || !menu) return;
    const rect = trigger.getBoundingClientRect();
    position = { left: Math.max(12, Math.min(rect.left, innerWidth - menu.offsetWidth - 12)), top: Math.max(12, Math.min(rect.bottom + 8, innerHeight - menu.offsetHeight - 12)) };
    (menu.querySelector<HTMLButtonElement>('button:not(:disabled)') ?? menu).focus();
  }
  function key(event: KeyboardEvent) {
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(true); return; }
    if (event.key === 'Tab') { close(true); return; }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault(); event.stopPropagation();
    if (!menu) return;
    const items = [...menu.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
    if (!items.length) return;
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (current + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length;
    items[next].focus();
  }
  function choose(direction: SplitDirection) { close(); split(direction); }
</script>

<svelte:window onpointerdown={event => { if (open && event.target instanceof Node && !trigger.contains(event.target) && !menu?.contains(event.target)) close(); }} onresize={() => { if (open) close(true); }} />
<button bind:this={trigger} class="icon-button split-trigger" aria-label={label} title={label} aria-haspopup="menu" aria-expanded={open} aria-controls={id} {disabled}
  onclick={() => void toggle()} onkeydown={event => { if (event.key === 'ArrowDown') { event.preventDefault(); void toggle(); } }}>
  <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/></svg>
</button>
{#if open}
  <div bind:this={menu} id={id} class="profile-menu split-menu" role="menu" aria-label="分屏布局" tabindex="-1" style={`left: ${position.left}px; top: ${position.top}px`} onkeydown={key}>
    <div class="eyebrow">{label === '分屏' ? '拆分当前窗格' : label}</div>
    <button role="menuitem" tabindex="-1" disabled={!!columnsReason} aria-label="向右分屏" title={columnsReason || shortcuts.split_right} aria-describedby={`${id}-reason`} onclick={() => choose('columns')}><span><svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/></svg>向右分屏</span>{#if shortcuts.split_right}<kbd>{shortcuts.split_right}</kbd>{/if}</button>
    <button role="menuitem" tabindex="-1" disabled={!!rowsReason} aria-label="向下分屏" title={rowsReason || shortcuts.split_down} aria-describedby={`${id}-reason`} onclick={() => choose('rows')}><span><svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 12h18"/></svg>向下分屏</span>{#if shortcuts.split_down}<kbd>{shortcuts.split_down}</kbd>{/if}</button>
    <p id={`${id}-reason`}>{explanation || '当前窗格均分，新窗格获得焦点'}</p>
  </div>
{/if}
