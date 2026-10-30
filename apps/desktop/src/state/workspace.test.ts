import { expect, it } from 'vitest';
import type { Snapshot } from '../contracts/models';
import { createPane, createTab, workspaceSnapshot } from './workspace';

function pane(id: string, profile: string, cwd: string | null) {
  const result = createPane({ sessionId: id, viewEpoch: 'view', protocolVersion: 1 }, profile);
  result.id = id;
  result.snapshot = { profileId: profile, startupCwd: cwd } as Snapshot;
  return result;
}
it('saves every pane as a schema 1 tab with the focused pane selected', () => {
  const tab = createTab(pane('one', 'pwsh', 'C:\\project'), '项目');
  tab.panes.push(pane('two', 'cmd', 'D:\\logs')); tab.activePaneId = 'two'; tab.zoomed = true;
  expect(workspaceSnapshot([tab], tab.id)).toEqual({ schemaVersion: 1, geometry: null, tabs: [
    { id: 'one', profileId: 'pwsh', startupCwd: 'C:\\project', title: '项目' },
    { id: 'two', profileId: 'cmd', startupCwd: 'D:\\logs', title: '项目' },
  ], activeTab: 'two' });
});
it('omits unrepresentable startup paths and selects a recoverable tab', () => {
  const tab = createTab(pane('one', 'pwsh', null));
  tab.panes.push(pane('two', 'cmd', 'D:\\logs'));
  const saved = workspaceSnapshot([tab], tab.id);
  expect(saved.tabs.map(t => t.id)).toEqual(['two']); expect(saved.activeTab).toBe('two');
  expect(workspaceSnapshot([], null)).toEqual({ schemaVersion: 1, geometry: null, tabs: [], activeTab: null });
});

it('does not save a forgotten session while its replacement is pending or failed', () => {
  const tab = createTab(pane('one', 'pwsh', 'C:\\project'));
  tab.panes[0].forgotten = true;
  expect(workspaceSnapshot([tab], tab.id).tabs).toEqual([]);
});
