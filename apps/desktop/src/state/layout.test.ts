import { expect, it } from 'vitest';
import { closestPane, fitsSplit, focusNeighbor, geometry, leafIds, minimumPane, removeLeaf, replaceLeaf, splitLeaf, subtreeMinimum, type PaneLayout } from './layout';
const min = { width: 200, height: 100 };
const size = { width: 1007, height: 607 };
const leaf = (id: string): PaneLayout => ({ kind: 'pane', id });
function nested() {
  return splitLeaf(splitLeaf(leaf('left'), 'left', 'columns', 'top-right', 'root'), 'top-right', 'rows', 'bottom-right', 'right');
}
it('splits only the selected region, leaving existing leaf identities and unrelated geometry intact', () => {
  const initial = splitLeaf(leaf('left'), 'left', 'columns', 'top-right', 'root');
  const next = splitLeaf(initial, 'top-right', 'rows', 'bottom-right', 'right');
  expect(geometry(next, size, min).panes).toEqual({
    left: { x: 0, y: 0, width: 500, height: 607 },
    'top-right': { x: 507, y: 0, width: 500, height: 300 },
    'bottom-right': { x: 507, y: 307, width: 500, height: 300 },
  });
  expect(leafIds(initial)).toEqual(['left', 'top-right']);
  expect(geometry(next, size, min).panes.left).toEqual(geometry(initial, size, min).panes.left);
  expect(splitLeaf(initial, 'absent', 'rows', 'unused', 'unused')).toBe(initial);
});
it('closing a leaf expands only its sibling subtree and retains ancestor and child ratios', () => {
  const layout = nested(); if (layout.kind !== 'split' || layout.second.kind !== 'split') throw new Error();
  layout.ratio = 0.4; layout.second.ratio = 0.7;
  const removed = removeLeaf(layout, 'left');
  expect(removed.layout).toBe(layout.second); expect(removed.neighbors).toEqual(['top-right', 'bottom-right']);
  const internal = removeLeaf(layout, 'top-right');
  expect(leafIds(internal.layout!)).toEqual(['left', 'bottom-right']);
  expect(internal.neighbors).toEqual(['bottom-right']);
  expect(geometry(internal.layout!, size, min).panes.left).toEqual(geometry(layout, size, min).panes.left);
  expect(removeLeaf(leaf('only'), 'only').layout).toBeNull();
  expect(removeLeaf(layout, 'absent').layout).toBe(layout);
});
it('restarting replaces the leaf without moving other panes or changing divider ratios', () => {
  const layout = nested(); const replaced = replaceLeaf(layout, 'top-right', leaf('restarted'));
  const before = geometry(layout, size, min); const after = geometry(replaced, size, min);
  expect(after.panes.restarted).toEqual(before.panes['top-right']);
  expect(after.panes.left).toEqual(before.panes.left);
  expect(after.dividers.map(d => d.ratio)).toEqual(before.dividers.map(d => d.ratio));
});
it('bounds parent dividers by all descendant minima and preserves preferred ratios across window shrink', () => {
  const layout = splitLeaf(nested(), 'left', 'columns', 'middle', 'left-split');
  if (layout.kind !== 'split') throw new Error(); layout.ratio = 0.6;
  expect(subtreeMinimum(layout, min)).toEqual({ width: 614, height: 207 });
  const full = geometry(layout, size, min); const small = geometry(layout, { width: 400, height: 100 }, min);
  expect(full.dividers[0].minimum).toBe(0.407); expect(full.dividers[0].maximum).toBe(0.8);
  expect(small.dividers[0].minimum).toBe(small.dividers[0].maximum);
  expect(Object.values(small.panes).every(p => p.width >= 0 && p.height >= 0)).toBe(true);
  expect(layout.ratio).toBe(0.6); expect(geometry(layout, size, min)).toEqual(full);
  expect(Object.values(geometry(layout, { width: 0, height: 0 }, min).panes).every(p => p.width >= 0 && p.height >= 0)).toBe(true);
});
it('guards both directions using the selected restored rectangle and actual font cell dimensions', () => {
  const minimum = minimumPane({ width: 8.4, height: 16 });
  expect(minimum).toEqual({ width: 386, height: 216 });
  expect(fitsSplit({ width: 779, height: 216 }, 'columns', minimum)).toBe(true);
  expect(fitsSplit({ width: 778, height: 216 }, 'columns', minimum)).toBe(false);
  expect(fitsSplit({ width: 386, height: 439 }, 'rows', minimum)).toBe(true);
  expect(fitsSplit({ width: 385, height: 439 }, 'rows', minimum)).toBe(false);
  expect(fitsSplit(geometry(nested(), size, min).panes['bottom-right'], 'rows', minimum)).toBe(false);
  expect(fitsSplit(undefined, 'rows', minimum)).toBe(false);
});
it('moves focus by spatial adjacency, with deterministic ties and no wrap at the outer edge', () => {
  const panes = geometry(nested(), size, min).panes;
  expect(focusNeighbor(panes, 'bottom-right', 'up')).toBe('top-right');
  expect(focusNeighbor(panes, 'bottom-right', 'left')).toBe('left');
  expect(focusNeighbor(panes, 'left', 'right')).toBe('top-right');
  expect(focusNeighbor(panes, 'top-right', 'up')).toBeUndefined();
  expect(focusNeighbor(panes, 'bottom-right', 'right')).toBeUndefined();
  expect(closestPane(panes, 'left', ['top-right', 'bottom-right'])).toBe('top-right');
});
