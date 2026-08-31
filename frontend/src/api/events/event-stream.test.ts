import { describe, expect, it, vi } from 'vitest';

import fixture from '../../../../fixtures/events/operation-progress.json';
import workspaceActivePaneChangedFixture from '../../../../fixtures/events/workspace-active-pane-changed.json';
import workspaceClosedFixture from '../../../../fixtures/events/workspace-closed.json';
import workspaceCreatedFixture from '../../../../fixtures/events/workspace-created.json';
import workspaceDeletedFixture from '../../../../fixtures/events/workspace-deleted.json';
import workspaceLayoutChangedFixture from '../../../../fixtures/events/workspace-layout-changed.json';
import workspaceOpenedFixture from '../../../../fixtures/events/workspace-opened.json';
import workspaceRenamedFixture from '../../../../fixtures/events/workspace-renamed.json';
import workspaceTabActivatedFixture from '../../../../fixtures/events/workspace-tab-activated.json';
import workspaceTabAddedFixture from '../../../../fixtures/events/workspace-tab-added.json';
import workspaceTabClosedFixture from '../../../../fixtures/events/workspace-tab-closed.json';
import workspaceTabNavigatedFixture from '../../../../fixtures/events/workspace-tab-navigated.json';
import workspaceTabViewChangedFixture from '../../../../fixtures/events/workspace-tab-view-changed.json';
import type { BackendEvent } from '../../models';
import {
  BackendEventListenerRegistry,
  parseBackendEvent,
  type UnknownEventLogger,
} from './event-stream';

describe('parseBackendEvent', () => {
  it('parses the Rust-generated event envelope fixture', () => {
    const event = parseBackendEvent(fixture);

    expect(event).toEqual(fixture);
  });

  it.each([
    workspaceCreatedFixture,
    workspaceRenamedFixture,
    workspaceOpenedFixture,
    workspaceClosedFixture,
    workspaceDeletedFixture,
    workspaceLayoutChangedFixture,
    workspaceActivePaneChangedFixture,
    workspaceTabAddedFixture,
    workspaceTabClosedFixture,
    workspaceTabActivatedFixture,
    workspaceTabNavigatedFixture,
    workspaceTabViewChangedFixture,
  ])('round-trips the Rust-generated $payload.type fixture', (workspaceFixture) => {
    expect(parseBackendEvent(workspaceFixture)).toEqual(workspaceFixture);
  });

  it('ignores a future event type without throwing and logs it once in development', () => {
    const logger = vi.fn<UnknownEventLogger>();
    const futureEvent = {
      eventId: 1043,
      timestamp: '2026-07-29T12:35:00Z',
      payload: { type: 'directory.reindexed', revision: 9 },
    };

    expect(parseBackendEvent(futureEvent, { development: true, logger })).toBeUndefined();
    expect(parseBackendEvent(futureEvent, { development: true, logger })).toBeUndefined();
    expect(logger).toHaveBeenCalledTimes(1);
    expect(logger).toHaveBeenCalledWith('directory.reindexed');
  });

  it('ignores malformed envelopes without throwing', () => {
    expect(parseBackendEvent({ payload: { type: 'runtime.ready' } })).toBeUndefined();
    expect(parseBackendEvent(null)).toBeUndefined();
  });

  it('recognises a search.resultsBatch event (task 0068) instead of dropping it as unknown', () => {
    const searchEvent = {
      eventId: 1044,
      timestamp: '2026-08-02T12:00:00Z',
      payload: {
        type: 'search.resultsBatch',
        searchId: '11111111-1111-4111-8111-111111111111',
        entries: [],
        isComplete: true,
        warningsCount: 0,
      },
    };

    expect(parseBackendEvent(searchEvent)).toEqual(searchEvent);
  });

  it('recognises disk-usage progress events', () => {
    const progressEvent = {
      eventId: 1045,
      timestamp: '2026-08-28T12:00:00Z',
      workspaceId: 'workspace-1',
      payload: {
        type: 'diskUsage.progress',
        scanId: 'scan-1',
        root: {
          name: '/',
          location: { providerId: 'file', uri: 'file:///' },
          kind: 'directory',
          logicalBytes: 10,
          physicalBytes: 10,
          collapsed: false,
          children: [],
        },
        unreadableEntries: 0,
        isComplete: false,
      },
    };

    expect(parseBackendEvent(progressEvent)).toEqual(progressEvent);
  });
});

describe('type safety', () => {
  it('never lets a directory snapshot satisfy a workspace event payload', () => {
    const directorySnapshotShaped = {
      type: 'directory.snapshot',
      snapshot: { paneId: 'pane', requestId: 'req', revision: 1 },
    };

    // @ts-expect-error a directory.snapshot payload must never satisfy the workspace.opened shape.
    const asWorkspaceOpened: { type: 'workspace.opened'; revision: number } =
      directorySnapshotShaped;
    void asWorkspaceOpened;
  });
});

describe('BackendEventListenerRegistry', () => {
  it('dispatches known events to every listener and supports unsubscribe', () => {
    const registry = new BackendEventListenerRegistry();
    const first = vi.fn<(event: BackendEvent) => void>();
    const second = vi.fn<(event: BackendEvent) => void>();
    const unsubscribeFirst = registry.subscribe(first);
    registry.subscribe(second);
    const event = parseBackendEvent(fixture);
    expect(event).toBeDefined();

    registry.dispatch(event as BackendEvent);
    unsubscribeFirst();
    registry.dispatch(event as BackendEvent);

    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(2);
  });
});
