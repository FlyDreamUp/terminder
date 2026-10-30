export type SplitDirection = 'columns' | 'rows';
export type FocusDirection = 'left' | 'right' | 'up' | 'down';
export interface PixelSize { width: number; height: number }
export interface Rect extends PixelSize { x: number; y: number }
export type PaneLayout = { kind: 'pane'; id: string } | SplitLayout;
export interface SplitLayout {
  kind: 'split'; id: string; direction: SplitDirection; ratio: number; first: PaneLayout; second: PaneLayout;
}
export interface Divider {
  node: SplitLayout; rect: Rect; extent: number; ratio: number; minimum: number; maximum: number; controls: string[];
}
export interface LayoutGeometry { panes: Record<string, Rect>; dividers: Divider[] }
export const DIVIDER_SIZE = 7;
export const MAX_TAB_PANES = 8;
export const MIN_COLUMNS = 40;
export const MIN_ROWS = 10;
// Keep these in sync with the terminal padding, xterm gutter and pane toolbar in styles.css.
export function minimumPane(cell: PixelSize): PixelSize {
  return { width: Math.ceil(MIN_COLUMNS * cell.width + 50), height: Math.ceil(MIN_ROWS * cell.height + 56) };
}
export function leafIds(node: PaneLayout): string[] {
  return node.kind === 'pane' ? [node.id] : [...leafIds(node.first), ...leafIds(node.second)];
}
export function replaceLeaf(node: PaneLayout, id: string, replacement: PaneLayout): PaneLayout {
  if (node.kind === 'pane') return node.id === id ? replacement : node;
  const first = replaceLeaf(node.first, id, replacement); const second = replaceLeaf(node.second, id, replacement);
  return first === node.first && second === node.second ? node : { ...node, first, second };
}
export function splitLeaf(node: PaneLayout, id: string, direction: SplitDirection, nextId: string, splitId: string): PaneLayout {
  return replaceLeaf(node, id, { kind: 'split', id: splitId, direction, ratio: 0.5,
    first: { kind: 'pane', id }, second: { kind: 'pane', id: nextId } });
}
export function removeLeaf(node: PaneLayout, id: string): { layout: PaneLayout | null; neighbors: string[] } {
  if (node.kind === 'pane') return { layout: node.id === id ? null : node, neighbors: [] };
  const first = removeLeaf(node.first, id);
  if (!first.layout) return { layout: node.second, neighbors: leafIds(node.second) };
  if (first.layout !== node.first) return { layout: { ...node, first: first.layout }, neighbors: first.neighbors };
  const second = removeLeaf(node.second, id);
  if (!second.layout) return { layout: node.first, neighbors: leafIds(node.first) };
  return { layout: second.layout === node.second ? node : { ...node, second: second.layout }, neighbors: second.neighbors };
}
export function subtreeMinimum(node: PaneLayout, minimum: PixelSize): PixelSize {
  if (node.kind === 'pane') return minimum;
  const first = subtreeMinimum(node.first, minimum); const second = subtreeMinimum(node.second, minimum);
  return node.direction === 'columns'
    ? { width: first.width + DIVIDER_SIZE + second.width, height: Math.max(first.height, second.height) }
    : { width: Math.max(first.width, second.width), height: first.height + DIVIDER_SIZE + second.height };
}
export function ratioBounds(extent: number, first: number, second: number) {
  // If the window shrinks below the combined minimum, keep every session and share the shortage.
  // The stored ratio is untouched, so enlarging the window restores the user's arrangement.
  if (extent < first + second) { const ratio = first / (first + second); return { minimum: ratio, maximum: ratio }; }
  return { minimum: first / extent, maximum: 1 - second / extent };
}
export function geometry(node: PaneLayout, size: PixelSize, minimum: PixelSize): LayoutGeometry {
  const result: LayoutGeometry = { panes: {}, dividers: [] };
  function visit(node: PaneLayout, rect: Rect) {
    if (node.kind === 'pane') { result.panes[node.id] = rect; return; }
    const columns = node.direction === 'columns'; const dimension = columns ? 'width' : 'height';
    const gutter = Math.min(DIVIDER_SIZE, rect[dimension]); const extent = Math.max(0, rect[dimension] - gutter);
    const bounds = ratioBounds(extent, subtreeMinimum(node.first, minimum)[dimension], subtreeMinimum(node.second, minimum)[dimension]);
    const ratio = Math.max(bounds.minimum, Math.min(bounds.maximum, node.ratio));
    const first = extent * ratio; const second = extent - first;
    result.dividers.push({ node, extent, ratio, ...bounds, controls: leafIds(node.first), rect: columns
      ? { x: rect.x + first, y: rect.y, width: gutter, height: rect.height }
      : { x: rect.x, y: rect.y + first, width: rect.width, height: gutter } });
    visit(node.first, { ...rect, [dimension]: first });
    visit(node.second, { ...rect, [columns ? 'x' : 'y']: rect[columns ? 'x' : 'y'] + first + gutter, [dimension]: second });
  }
  visit(node, { x: 0, y: 0, width: Math.max(0, size.width), height: Math.max(0, size.height) });
  return result;
}
export function fitsSplit(rect: PixelSize | undefined, direction: SplitDirection, minimum: PixelSize): boolean {
  if (!rect) return false;
  return direction === 'columns'
    ? rect.width >= minimum.width * 2 + DIVIDER_SIZE && rect.height >= minimum.height
    : rect.height >= minimum.height * 2 + DIVIDER_SIZE && rect.width >= minimum.width;
}
export function closestPane(panes: Record<string, Rect>, sourceId: string, candidates: string[]): string | undefined {
  const source = panes[sourceId]; if (!source) return candidates[0];
  const distance = (id: string) => { const p = panes[id]; return p ? Math.hypot(p.x + p.width / 2 - source.x - source.width / 2, p.y + p.height / 2 - source.y - source.height / 2) : Infinity; };
  return [...candidates].sort((a, b) => distance(a) - distance(b))[0];
}
export function focusNeighbor(panes: Record<string, Rect>, sourceId: string, direction: FocusDirection): string | undefined {
  const source = panes[sourceId]; if (!source) return;
  const horizontal = direction === 'left' || direction === 'right';
  const axis = horizontal ? 'x' : 'y'; const cross = horizontal ? 'y' : 'x';
  const length = horizontal ? 'width' : 'height'; const breadth = horizontal ? 'height' : 'width';
  const forward = direction === 'right' || direction === 'down';
  const candidates = Object.entries(panes).filter(([id, rect]) => id !== sourceId && (forward
    ? rect[axis] >= source[axis] + source[length] - 0.5 : rect[axis] + rect[length] <= source[axis] + 0.5));
  const score = (rect: Rect) => {
    const overlap = Math.min(source[cross] + source[breadth], rect[cross] + rect[breadth]) - Math.max(source[cross], rect[cross]);
    const gap = forward ? rect[axis] - source[axis] - source[length] : source[axis] - rect[axis] - rect[length];
    const offset = Math.abs(rect[cross] + rect[breadth] / 2 - source[cross] - source[breadth] / 2);
    return [overlap > 0 ? 0 : 1, gap, offset];
  };
  candidates.sort(([, a], [, b]) => { const sa = score(a); const sb = score(b); return sa[0] - sb[0] || sa[1] - sb[1] || sa[2] - sb[2]; });
  return candidates[0]?.[0];
}
export function rectStyle(rect: Rect): string {
  return `left:${rect.x}px;top:${rect.y}px;width:${rect.width}px;height:${rect.height}px`;
}
