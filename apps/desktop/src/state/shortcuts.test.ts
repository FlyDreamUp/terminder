import { expect, it } from 'vitest';
import { shortcutAction } from './shortcuts';
const shortcuts = { new_tab: 'Ctrl+Shift+T', copy: 'Ctrl+Shift+C', settings: 'Ctrl+,' };
function event(key: string, options: Partial<KeyboardEvent> = {}): KeyboardEvent { return { key, ctrlKey: true, shiftKey: false, altKey: false, metaKey: false, isComposing: false, getModifierState: () => false, ...options } as KeyboardEvent; }
it('leaves terminal Ctrl+C, editors and IME composition alone', () => {
  expect(shortcutAction(event('c'), shortcuts, 'terminal')).toBeNull();
  expect(shortcutAction(event('C', { shiftKey: true }), shortcuts, 'terminal')).toBe('copy');
  expect(shortcutAction(event('C', { shiftKey: true }), shortcuts, 'editor')).toBeNull();
  expect(shortcutAction(event('T', { shiftKey: true, isComposing: true }), shortcuts, 'terminal')).toBeNull();
  expect(shortcutAction(event('T', { shiftKey: true, getModifierState: () => true }), shortcuts, 'terminal')).toBeNull();
  expect(shortcutAction(event(','), shortcuts, 'editor')).toBe('settings');
});
