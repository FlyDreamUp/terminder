import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { SearchAddon } from '@xterm/addon-search';
import { WebLinksAddon } from '@xterm/addon-web-links';
import '@xterm/xterm/css/xterm.css';
import type { Config, SessionEvent, Size, Snapshot } from '../contracts/models';
import { errorMessage, validateSessionEvent } from '../contracts/models';
import type { Transport } from '../transport/tauri';
import { binaryBytes } from '../transport/input';
import { OutputProgress } from '../transport/output';
import { RendererCoordinator } from './renderer';
import { dark, light, isDark } from './themes';
import { inspectPaste } from './paste';

export interface ViewCallbacks {
  state(snapshot: Snapshot): void;
  error(message: string): void;
  activity(): void;
  title(title: string): void;
  size(size: Size): void;
  renderer(renderer: 'webgl' | 'dom'): void;
  complete(truncated: boolean): void;
  shortcut(event: KeyboardEvent): boolean;
  confirmPaste(text: string, reason: string): Promise<boolean>;
  link(url: string): void;
}
export class TerminalView {
  readonly terminal: Terminal;
  private fit = new FitAddon();
  private search = new SearchAddon();
  private disposables: { dispose(): void }[] = [];
  private observer: ResizeObserver;
  private resizeTimer: ReturnType<typeof setTimeout> | null = null;
  private endTimer: ReturnType<typeof setTimeout> | null = null;
  private progress: OutputProgress;
  private active = false;
  private disposed = false;
  private running = false;
  private outputReady = false;
  private outputComplete = false;
  private broken = false;
  private startupOutput: Uint8Array[] = [];
  private startupBytes = 0;
  private finished = false;
  private resizing = false;
  private resizeAgain = false;
  private lastSize: Size | null = null;
  private lastTitle = 0;
  private pasteCapture: Uint8Array[] | null = null;
  private media = matchMedia('(prefers-color-scheme: dark)');
  private mediaChanged = () => this.configure(this.config);
  private pasteEvent = (event: ClipboardEvent) => { event.preventDefault(); event.stopPropagation(); void this.paste(event.clipboardData?.getData('text/plain') ?? ''); };
  private dropEvent = (event: DragEvent) => { event.preventDefault(); };

  constructor(private container: HTMLElement, private transport: Transport, private config: Config, private renderer: RendererCoordinator, private callbacks: ViewCallbacks) {
    this.terminal = new Terminal({
      cursorBlink: true, cursorStyle: 'bar', fontFamily: config.terminal.font_family.map(f => `"${f.replace(/["\\]/g, '')}"`).join(', '),
      fontSize: config.terminal.font_size_px, lineHeight: 1.12, scrollback: config.terminal.scrollback_lines,
      theme: isDark(config.terminal.theme) ? dark : light, screenReaderMode: true,
      allowProposedApi: false, allowTransparency: false, disableStdin: true,
      linkHandler: { activate: (_event, uri) => this.callbacks.link(uri) },
    });
    this.terminal.loadAddon(this.fit); this.terminal.loadAddon(this.search);
    this.terminal.loadAddon(new WebLinksAddon((_event, uri) => this.callbacks.link(uri)));
    // The custom handler runs before xterm's built-in handler, and consumes OSC 52 unconditionally.
    this.disposables.push(this.terminal.parser.registerOscHandler(52, () => true));
    this.terminal.attachCustomKeyEventHandler(event => {
      if (event.type !== 'keydown' || event.isComposing || event.keyCode === 229 || event.getModifierState('AltGraph')) return true;
      return !this.callbacks.shortcut(event);
    });
    this.disposables.push(this.terminal.onData(text => this.input(new TextEncoder().encode(text))), this.terminal.onBinary(data => this.input(binaryBytes(data))));
    this.disposables.push(this.terminal.onTitleChange(title => {
      if (performance.now() - this.lastTitle < 250) return;
      this.lastTitle = performance.now(); this.callbacks.title([...title].filter(c => c >= ' ' && c !== '\u007f').slice(0, 80).join(''));
    }));
    this.progress = new OutputProgress(
      (sequence, bytes) => this.transport.ack(sequence, bytes),
      truncated => this.completeOutput(truncated),
    );
    // Capture paste before xterm so context-menu paste follows the same policy as the shortcut.
    container.addEventListener('paste', this.pasteEvent, true); container.addEventListener('drop', this.dropEvent, true);
    this.terminal.open(container);
    this.observer = new ResizeObserver(() => this.scheduleResize()); this.observer.observe(container);
    this.media.addEventListener('change', this.mediaChanged);
  }
  async start(): Promise<void> {
    await document.fonts.ready;
    if (this.disposed) return;
    const size = this.measure(); if (!size) throw new Error('终端容器尚不可测量，未启动 Shell');
    this.lastSize = size; this.callbacks.size(size);
    await this.transport.start(size, bytes => {
      if (this.disposed || this.broken) return;
      try {
        // Running and output travel on separate channels. Do not lose early device-query replies.
        if (!this.outputReady) {
          if (this.startupOutput.length >= 64 || this.startupBytes + bytes.length > 256 * 1024) throw new Error('启动输出超过协议窗口');
          this.startupOutput.push(bytes); this.startupBytes += bytes.length; return;
        }
        this.write(bytes);
      } catch (error) { this.fail(error); }
    }, event => this.event(event));
  }
  private write(bytes: Uint8Array): void {
    this.callbacks.activity();
    this.progress.receive(bytes, (data, done) => this.terminal.write(data, () => {
      if (!this.disposed) { try { done(); } catch (error) { this.fail(error); } }
    }));
  }
  private event(event: SessionEvent): void {
    try {
      validateSessionEvent(event, this.transport.binding);
      if (event.type === 'state') {
        this.running = !this.broken && event.snapshot.phase === 'running'; this.finished = event.snapshot.phase === 'finished';
        this.terminal.options.disableStdin = !this.running;
        this.callbacks.state(event.snapshot);
        if (!this.broken && !['preparing', 'starting'].includes(event.snapshot.phase)) {
          // A close during process creation may deliver tail bytes without ever reaching Running.
          this.outputReady = true;
          for (const bytes of this.startupOutput) this.write(bytes);
          this.startupOutput = []; this.startupBytes = 0;
        }
        if (this.running) this.scheduleResize();
      } else if (event.type === 'error') this.callbacks.error(event.error.message);
      else if (!this.broken) {
        this.progress.finish(event.sequence, event.bytes, event.truncated);
        if (!this.progress.isComplete() && !this.endTimer) {
          this.endTimer = setTimeout(() => this.fail(new Error('尾部输出未在期限内完成显示')), 5000);
        }
      }
    } catch (error) { this.fail(error); }
  }
  private input(bytes: Uint8Array): void {
    if (!this.running || this.disposed) return;
    if (this.pasteCapture) { this.pasteCapture.push(bytes); return; }
    try { this.transport.input(bytes); } catch (error) { this.callbacks.error(errorMessage(error)); }
  }
  private completeOutput(truncated: boolean): void {
    if (this.outputComplete) return;
    this.outputComplete = true;
    if (this.endTimer) clearTimeout(this.endTimer);
    this.endTimer = null;
    this.callbacks.complete(truncated);
  }
  private fail(error: unknown): void {
    if (this.broken || this.disposed) return;
    this.broken = true;
    this.callbacks.error(errorMessage(error));
    this.running = false;
    this.terminal.options.disableStdin = true;
    this.startupOutput = []; this.startupBytes = 0;
    this.completeOutput(true);
    void this.transport.viewFailed().catch(e => this.callbacks.error(errorMessage(e)));
  }
  private measure(): Size | null {
    if (this.container.clientWidth < 40 || this.container.clientHeight < 20) return null;
    const proposed = this.fit.proposeDimensions(); if (!proposed || proposed.cols < 1 || proposed.rows < 1) return null;
    const cols = Math.min(1000, proposed.cols); const rows = Math.min(1000, proposed.rows, Math.floor(250000 / cols));
    this.terminal.resize(cols, rows); return { cols, rows };
  }
  private scheduleResize(): void {
    if (this.resizeTimer) clearTimeout(this.resizeTimer);
    this.resizeTimer = setTimeout(() => { this.resizeTimer = null; void this.resize(); }, 40);
  }
  private async resize(): Promise<void> {
    if (this.disposed || !this.active) return;
    if (this.resizing) { this.resizeAgain = true; return; }
    const size = this.measure(); if (!size) return;
    if (!this.running || (size.cols === this.lastSize?.cols && size.rows === this.lastSize?.rows)) return;
    this.resizing = true;
    try { await this.transport.resize(size); this.lastSize = size; this.callbacks.size(size); }
    catch (error) { this.callbacks.error(`终端尺寸同步失败：${errorMessage(error)}`); }
    finally { this.resizing = false; if (this.resizeAgain) { this.resizeAgain = false; void this.resize(); } }
  }
  setActive(active: boolean): void {
    if (this.disposed) return;
    this.active = active;
    if (active) { this.renderer.activate(this.terminal, this.config.terminal.renderer === 'auto', r => this.callbacks.renderer(r)); this.scheduleResize(); requestAnimationFrame(() => { if (!this.disposed && this.active) this.terminal.focus(); }); }
    else this.renderer.release(this.terminal);
  }
  configure(config: Config): void {
    this.config = config; this.terminal.options.fontFamily = config.terminal.font_family.map(f => `"${f.replace(/["\\]/g, '')}"`).join(', ');
    this.terminal.options.fontSize = config.terminal.font_size_px; this.terminal.options.scrollback = config.terminal.scrollback_lines;
    this.terminal.options.theme = isDark(config.terminal.theme) ? dark : light;
    if (this.active) this.renderer.activate(this.terminal, config.terminal.renderer === 'auto', r => this.callbacks.renderer(r)); this.scheduleResize();
  }
  selection(): string { return this.terminal.getSelection(); }
  focus(): void { this.terminal.focus(); }
  find(text: string, previous = false): boolean { return text.length > 0 && (previous ? this.search.findPrevious(text) : this.search.findNext(text)); }
  clearSearch(): void { this.search.clearDecorations(); }
  async paste(text: string): Promise<void> {
    try {
      if (!this.running || !text) return;
      const inspection = inspectPaste(text, this.config.clipboard.max_paste_bytes, this.config.clipboard.confirm_multiline);
      if (inspection.confirm && !await this.callbacks.confirmPaste(text, inspection.reason)) return;
      if (!this.running || this.disposed) return;
      if (!this.transport.canInput(inspection.bytes + 12)) throw new Error('输入繁忙，此次粘贴未发送');
      // xterm adds the appropriate bracketed-paste wrapper once according to current terminal mode.
      const chunks: Uint8Array[] = []; this.pasteCapture = chunks;
      try { this.terminal.paste(text); } finally { this.pasteCapture = null; }
      const payload = new Uint8Array(chunks.reduce((total, chunk) => total + chunk.length, 0));
      let offset = 0; for (const chunk of chunks) { payload.set(chunk, offset); offset += chunk.length; }
      this.transport.paste(payload); this.focus();
    } catch (error) { this.callbacks.error(errorMessage(error)); }
  }
  dispose(): void {
    if (this.disposed) return; this.disposed = true;
    if (!this.finished) void this.transport.viewFailed().catch(() => { /* The native window coordinator also handles a destroyed view. */ });
    this.startupOutput = []; this.startupBytes = 0;
    this.observer.disconnect(); this.media.removeEventListener('change', this.mediaChanged);
    this.container.removeEventListener('paste', this.pasteEvent, true); this.container.removeEventListener('drop', this.dropEvent, true);
    if (this.resizeTimer) clearTimeout(this.resizeTimer); if (this.endTimer) clearTimeout(this.endTimer);
    this.renderer.release(this.terminal); this.disposables.forEach(d => d.dispose()); this.transport.dispose(); this.terminal.dispose();
  }
}
