export type Scope = 'terminal' | 'editor' | 'app';
export function shortcutAction(event: KeyboardEvent, shortcuts: Record<string, string>, scope: Scope): string | null {
  if (event.isComposing || event.keyCode === 229 || event.getModifierState('AltGraph')) return null;
  for (const [action, value] of Object.entries(shortcuts)) {
    const parts = value.toLowerCase().split('+'); const key = parts.pop();
    if (event.ctrlKey !== parts.includes('ctrl') || event.shiftKey !== parts.includes('shift') || event.altKey !== parts.includes('alt') || event.metaKey !== parts.includes('meta')) continue;
    if (event.key.toLowerCase() !== key) continue;
    if (scope === 'editor' && action !== 'settings') return null;
    if (scope !== 'terminal' && ['copy', 'paste', 'find'].includes(action)) return null;
    return action;
  }
  return null;
}
