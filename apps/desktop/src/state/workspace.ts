import type { Binding, Size, Snapshot, Workspace } from '../contracts/models';

import type { PaneLayout } from './layout';
export type { SplitDirection } from './layout';
export interface Pane {
  id: string; binding: Binding; profile: string; programTitle: string; snapshot: Snapshot | null;
  activity: boolean; error: string; size: Size | null; renderer: 'webgl' | 'dom';
  outputComplete: boolean; closing: boolean; tailTruncated: boolean; forgotten: boolean;
}
export interface Tab {
  id: string; panes: Pane[]; activePaneId: string; userTitle: string | null;
  layout: PaneLayout; zoomed: boolean; closing: boolean; pending: boolean;
}
export function createPane(binding: Binding, profile: string): Pane {
  return { id: crypto.randomUUID(), binding, profile, programTitle: '', snapshot: null, activity: false,
    error: '', size: null, renderer: 'dom', outputComplete: false, closing: false, tailTruncated: false, forgotten: false };
}
export function createTab(pane: Pane, userTitle: string | null = null): Tab {
  return { id: pane.id, panes: [pane], activePaneId: pane.id, userTitle, layout: { kind: 'pane', id: pane.id },
    zoomed: false, closing: false, pending: false };
}
export function workspaceSnapshot(tabs: Tab[], activeId: string | null): Workspace {
  // Schema 1 stores startup information only. Keep every pane recoverable as a standalone tab.
  const saved = tabs.flatMap(tab => tab.panes.filter(pane => !pane.forgotten && pane.snapshot?.startupCwd != null).map(pane => ({
    id: pane.id, profileId: pane.snapshot!.profileId, startupCwd: pane.snapshot!.startupCwd, title: tab.userTitle,
  })));
  const focused = tabs.find(tab => tab.id === activeId)?.activePaneId;
  return { schemaVersion: 1, geometry: null, tabs: saved, activeTab: saved.find(tab => tab.id === focused)?.id ?? saved[0]?.id ?? null };
}
