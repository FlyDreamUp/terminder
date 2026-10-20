import type { Terminal } from '@xterm/xterm';
import { WebglAddon } from '@xterm/addon-webgl';

/** One active GPU context per window. Buffer and selection ownership always remain with xterm. */
export class RendererCoordinator {
  private active: Terminal | null = null;
  private addon: WebglAddon | null = null;
  private lost: (() => void) | null = null;
  activate(terminal: Terminal, enabled: boolean, changed: (renderer: 'webgl' | 'dom') => void): void {
    this.dispose(); this.active = terminal; changed('dom');
    if (!enabled) return;
    try {
      const addon = new WebglAddon(); this.addon = addon;
      this.lost = () => { this.dispose(); changed('dom'); };
      addon.onContextLoss(this.lost);
      terminal.loadAddon(addon); changed('webgl');
    } catch { this.dispose(); changed('dom'); }
  }
  release(terminal: Terminal): void { if (terminal === this.active) this.dispose(); }
  dispose(): void { this.addon?.dispose(); this.addon = null; this.active = null; this.lost = null; }
}
