import type { IDisposable, Terminal } from '@xterm/xterm';
import { WebglAddon } from '@xterm/addon-webgl';

type Renderer = 'webgl' | 'dom';

/** One active GPU context per window. Buffer and selection ownership remain with xterm. */
export class RendererCoordinator {
  private active: Terminal | null = null;
  private addon: WebglAddon | null = null;
  private contextLoss: IDisposable | null = null;
  private enabled = false;
  private changed: ((renderer: Renderer) => void) | null = null;

  activate(terminal: Terminal, enabled: boolean, changed: (renderer: Renderer) => void): void {
    if (this.active === terminal && this.enabled === enabled) {
      this.changed = changed;
      changed(this.addon ? 'webgl' : 'dom');
      return;
    }
    this.dispose();
    this.active = terminal;
    this.enabled = enabled;
    this.changed = changed;
    changed('dom');
    if (!enabled) return;
    try {
      const addon = new WebglAddon();
      this.addon = addon;
      this.contextLoss = addon.onContextLoss(() => this.fallback());
      terminal.loadAddon(addon);
      changed('webgl');
    } catch { this.fallback(); }
  }

  private fallback(): void {
    // Keep the active identity after failure so reapplying colors/fonts stays on DOM.
    this.contextLoss?.dispose();
    this.contextLoss = null;
    const addon = this.addon;
    this.addon = null;
    addon?.dispose();
    this.changed?.('dom');
  }

  release(terminal: Terminal): void {
    if (terminal === this.active) this.dispose();
  }

  dispose(): void {
    this.fallback();
    this.active = null;
    this.changed = null;
    this.enabled = false;
  }
}
