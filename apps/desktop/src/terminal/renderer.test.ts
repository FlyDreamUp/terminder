import { beforeEach, expect, it, vi } from 'vitest';
import type { Terminal } from '@xterm/xterm';

const gpu = vi.hoisted(() => ({ contexts: [] as { lose(): void; disposed: boolean }[] }));
vi.mock('@xterm/addon-webgl', () => ({
  WebglAddon: class {
    disposed = false;
    private loss = () => {};
    constructor() { gpu.contexts.push(this); }
    onContextLoss(callback: () => void) {
      this.loss = callback;
      return { dispose: () => { this.loss = () => {}; } };
    }
    lose() { this.loss(); }
    dispose() { this.disposed = true; }
  },
}));
import { RendererCoordinator } from './renderer';

function terminal(): Terminal { return { loadAddon: vi.fn() } as unknown as Terminal; }
beforeEach(() => { gpu.contexts.length = 0; });

it('keeps the active GPU context when only terminal appearance is reapplied', () => {
  const coordinator = new RendererCoordinator();
  const view = terminal();
  const changed = vi.fn();
  coordinator.activate(view, true, changed);
  coordinator.activate(view, true, changed);
  expect(gpu.contexts).toHaveLength(1);
  expect(gpu.contexts[0].disposed).toBe(false);
  expect(changed).toHaveBeenLastCalledWith('webgl');
  coordinator.dispose();
  expect(gpu.contexts[0].disposed).toBe(true);
});

it('retains DOM fallback after context loss until the active terminal changes', () => {
  const coordinator = new RendererCoordinator();
  const first = terminal();
  const changed = vi.fn();
  coordinator.activate(first, true, changed);
  gpu.contexts[0].lose();
  expect(changed).toHaveBeenLastCalledWith('dom');
  expect(gpu.contexts[0].disposed).toBe(true);
  coordinator.activate(first, true, changed);
  expect(gpu.contexts).toHaveLength(1);
  coordinator.activate(terminal(), true, vi.fn());
  expect(gpu.contexts.filter(context => !context.disposed)).toHaveLength(1);
  coordinator.release(first);
  expect(gpu.contexts.filter(context => !context.disposed)).toHaveLength(1);
  coordinator.dispose();
  expect(gpu.contexts.every(context => context.disposed)).toBe(true);
});
