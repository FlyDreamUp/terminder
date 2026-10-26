<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { Binding, Config } from '../contracts/models';
  import { errorMessage } from '../contracts/models';
  import { TauriTransport } from '../transport/tauri';
  import { TerminalView, type ViewCallbacks } from '../terminal/view';
  import type { RendererCoordinator } from '../terminal/renderer';
  let { binding, active, config, renderer, callbacks, ready }: {
    binding: Binding; active: boolean; config: Config; renderer: RendererCoordinator;
    callbacks: ViewCallbacks; ready: (view: TerminalView | null) => void;
  } = $props();
  let container: HTMLDivElement;
  let view = $state<TerminalView | null>(null);
  onMount(() => {
    const transport = new TauriTransport(binding, error => callbacks.error(errorMessage(error)));
    view = new TerminalView(container, transport, config, renderer, callbacks); ready(view);
    void view.start().catch(error => { callbacks.error(errorMessage(error)); void transport.viewFailed().catch(e => callbacks.error(errorMessage(e))); });
    return () => { view?.dispose(); ready(null); };
  });
  $effect(() => {
    const current = view; const selected = active;
    untrack(() => current?.setActive(selected));
  });
  $effect(() => {
    const current = view; const options = $state.snapshot(config);
    untrack(() => current?.configure(options));
  });
</script>
<div class="terminal-pane" class:inactive={!active} aria-hidden={!active} inert={!active} bind:this={container}></div>
