<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { ConfigDocument, TerminalConfig } from '../contracts/models';
  import { errorMessage } from '../contracts/models';
  import { host } from '../transport/tauri';
  let { document, saved, close }: { document: ConfigDocument; saved: (document: ConfigDocument) => void; close: () => void } = $props();
  let dialog: HTMLDialogElement;
  let tab = $state<'appearance' | 'configuration' | 'diagnostics'>('appearance');
  // Editing owns a snapshot: incoming projections must not overwrite an unsaved draft.
  const initial = untrack(() => $state.snapshot(document));
  let terminal = $state<TerminalConfig>(initial.config.terminal);
  let fonts = $state(initial.config.terminal.font_family.join(', '));
  let text = $state(initial.text);
  let revision = $state(initial.revision);
  let busy = $state(false); let message = $state(''); let diagnostic = $state('');
  let dirty = $derived(text !== document.text || JSON.stringify({ ...terminal, font_family: fonts.split(',').map(s => s.trim()).filter(Boolean) }) !== JSON.stringify(document.config.terminal));
  onMount(() => { dialog.showModal(); });
  async function save() {
    if (busy) return; busy = true; message = '';
    try {
      const next = tab === 'configuration' ? await host.saveConfig(text, revision) : await host.saveTerminal({ ...terminal, font_family: fonts.split(',').map(s => s.trim()).filter(Boolean) }, revision);
      saved(next); revision = next.revision; text = next.text; terminal = structuredClone(next.config.terminal); fonts = terminal.font_family.join(', '); message = '已保存';
    } catch (error) { message = errorMessage(error); } finally { busy = false; }
  }
  async function reload() {
    busy = true;
    try { const next = await host.config(); saved(next); text = next.text; revision = next.revision; terminal = structuredClone(next.config.terminal); fonts = terminal.font_family.join(', '); message = '已重新加载磁盘配置'; }
    catch (error) { message = errorMessage(error); } finally { busy = false; }
  }
  async function loadDiagnostics() { tab = 'diagnostics'; try { diagnostic = await host.diagnostics(); } catch (error) { diagnostic = errorMessage(error); } }
  function requestClose() { if (!dirty || window.confirm('放弃尚未保存的设置？')) close(); }
</script>
<dialog class="settings-dialog" bind:this={dialog} oncancel={event => { event.preventDefault(); requestClose(); }}>
  <header class="settings-header"><div><div class="eyebrow">PREFERENCES</div><h2>设置</h2></div><button class="icon-button" aria-label="关闭设置" onclick={requestClose}>×</button></header>
  <div class="settings-layout">
    <nav aria-label="设置分类"><button class:selected={tab === 'appearance'} onclick={() => tab = 'appearance'}>外观</button><button class:selected={tab === 'configuration'} onclick={() => tab = 'configuration'}>Profiles 与配置</button><button class:selected={tab === 'diagnostics'} onclick={() => void loadDiagnostics()}>诊断</button><div class="settings-nav-note">TERMINDER<br /><span>0.1.0 · Windows</span></div></nav>
    <section class="settings-content">
      {#if tab === 'appearance'}
        <h3>让终端适合你</h3><p class="muted">外观修改在保存后应用到已打开的终端。</p>
        <label>主题<select bind:value={terminal.theme}><option value="system">跟随系统</option><option value="dark">深色 · Graphite</option><option value="light">浅色 · Paper</option></select></label>
        <label>字体<span class="field-help">按顺序使用本机字体，逗号分隔</span><input bind:value={fonts} spellcheck="false" /></label>
        <div class="field-pair"><label>字号<input type="number" min="8" max="48" step="1" bind:value={terminal.font_size_px} /></label><label>历史行数<input type="number" min="0" max="20000" step="1000" bind:value={terminal.scrollback_lines} /></label></div>
        <label>渲染器<select bind:value={terminal.renderer}><option value="auto">自动 · WebGL 可用时启用</option><option value="dom">DOM · 兼容模式</option></select></label>
        <div class="font-preview" style:font-family={fonts} style:font-size={`${terminal.font_size_px}px`}><span>❯</span> echo "Hello, 世界"<br /><span class="muted">每一个想法，从这里开始。</span></div>
      {:else if tab === 'configuration'}
        <h3>Profiles 与配置</h3><p class="muted">修改程序、参数和目录只影响新会话。开启 restore_tabs 后，恢复会创建新进程，Shell 启动脚本仍可能执行。</p>
        <label class="sr-only" for="config-source">TOML 配置</label><textarea id="config-source" class="config-editor" bind:value={text} spellcheck="false" aria-describedby="config-note"></textarea>
        <p id="config-note" class="field-help">保存时校验 TOML、版本、Profile、资源预算和快捷键冲突。未知版本不会被覆盖。</p>
      {:else}
        <h3>本机诊断</h3><p class="muted">以下内容只包含版本、会话状态和错误类别，不含终端输入输出、目录、剪贴板或环境变量。不会上传。</p>
        <pre class="diagnostic-output">{diagnostic || '正在读取…'}</pre>
        <button onclick={() => void host.clipboardWrite(diagnostic).then(() => message = '已复制诊断').catch(error => message = errorMessage(error))}>复制诊断</button>
      {/if}
    </section>
  </div>
  <footer class="settings-footer"><span role="status" class:error={message !== '已保存' && message !== ''}>{message || (dirty ? '有未保存的更改' : '设置已同步')}</span><div><button disabled={busy} onclick={() => void reload()}>重新加载</button>{#if tab !== 'diagnostics'}<button class="primary" disabled={busy || !document.writable || !host.available()} onclick={() => void save()}>{busy ? '保存中…' : '保存设置'}</button>{/if}</div></footer>
</dialog>
