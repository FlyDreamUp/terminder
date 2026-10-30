<script lang="ts">
  import { rectStyle, type Divider } from '../state/layout';
  let { divider: data, label, resize }: { divider: Divider; label: string; resize: (ratio: number) => void } = $props();
  let element: HTMLDivElement;
  let drag: { pointer: number; position: number; ratio: number } | null = null;
  function clamp(value: number) { return Math.max(data.minimum, Math.min(data.maximum, value)); }
  function start(event: PointerEvent) {
    if (event.button !== 0) return;
    drag = { pointer: event.pointerId, position: data.node.direction === 'columns' ? event.clientX : event.clientY, ratio: data.ratio };
    element.setPointerCapture(event.pointerId); element.focus(); event.preventDefault();
  }
  function move(event: PointerEvent) {
    if (!drag || drag.pointer !== event.pointerId || !data.extent) return;
    const position = data.node.direction === 'columns' ? event.clientX : event.clientY;
    resize(clamp(drag.ratio + (position - drag.position) / data.extent));
  }
  function stop(event: PointerEvent) {
    if (drag?.pointer !== event.pointerId) return;
    drag = null;
    if (element.hasPointerCapture(event.pointerId)) element.releasePointerCapture(event.pointerId);
  }
  function key(event: KeyboardEvent) {
    // Modified arrows belong to configured application actions, including spatial focus.
    if (event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) return;
    const decrease = data.node.direction === 'columns' ? 'ArrowLeft' : 'ArrowUp';
    const increase = data.node.direction === 'columns' ? 'ArrowRight' : 'ArrowDown';
    if (![decrease, increase, 'Home', 'End', 'Enter'].includes(event.key)) return;
    event.preventDefault(); event.stopPropagation();
    resize(clamp(event.key === 'Home' ? data.minimum : event.key === 'End' ? data.maximum : event.key === 'Enter' ? 0.5 : data.ratio + (event.key === decrease ? -0.05 : 0.05)));
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (The WAI-ARIA window splitter pattern requires a focusable separator with keyboard resizing.) -->
<div bind:this={element} class="pane-divider" class:stacked={data.node.direction === 'rows'} role="separator" tabindex="0" style={rectStyle(data.rect)}
  aria-label={label} aria-controls={data.controls.map(id => `session-${id}`).join(' ')} aria-orientation={data.node.direction === 'columns' ? 'vertical' : 'horizontal'}
  aria-valuenow={Math.round(data.ratio * 100)} aria-valuemin={Math.round(data.minimum * 100)} aria-valuemax={Math.round(data.maximum * 100)}
  title="拖动或使用方向键调整大小；Enter 恢复均分" onpointerdown={start} onpointermove={move} onpointerup={stop} onpointercancel={stop} onlostpointercapture={() => drag = null} onkeydown={key}></div>
