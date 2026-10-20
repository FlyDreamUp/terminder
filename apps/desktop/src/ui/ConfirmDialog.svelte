<script lang="ts">
  import { onMount } from 'svelte';
  let { title, message, preview = '', accept = '继续', dangerous = false, answer }: { title: string; message: string; preview?: string; accept?: string; dangerous?: boolean; answer: (value: boolean) => void } = $props();
  let dialog: HTMLDialogElement;
  onMount(() => { dialog.showModal(); });
</script>
<dialog class="confirm-dialog" bind:this={dialog} oncancel={event => { event.preventDefault(); answer(false); }}>
  <div class="eyebrow">TERMINDER</div>
  <h2>{title}</h2><p>{message}</p>
  {#if preview}<pre class="preview">{preview}</pre>{/if}
  <div class="dialog-actions"><button onclick={() => answer(false)}>取消</button><button class:danger={dangerous} class:primary={!dangerous} onclick={() => answer(true)}>{accept}</button></div>
</dialog>
