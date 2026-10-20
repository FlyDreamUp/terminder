import { describe, expect, it } from 'vitest';
import fixture from '../../../../tests/fixtures/protocol.json';
import { validateSessionEvent, type SessionEvent } from './models';

describe('native control messages', () => {
  it('accepts the shared fixture and rejects stale versions, bindings and malformed payloads', () => {
    const state = fixture.state as SessionEvent;
    expect(() => validateSessionEvent(state, fixture.binding)).not.toThrow();
    expect(() => validateSessionEvent(fixture.outputEnd as SessionEvent, fixture.binding)).not.toThrow();
    for (const patch of [
      { binding: { ...fixture.binding, protocolVersion: 2 } },
      { binding: { ...fixture.binding, viewEpoch: crypto.randomUUID() } },
      { phase: 'unknown' }, { tailComplete: 'yes' }, { size: { cols: 0, rows: 24 } },
    ]) expect(() => validateSessionEvent({ type: 'state', snapshot: { ...fixture.state.snapshot, ...patch } } as SessionEvent, fixture.binding)).toThrow();
    expect(() => validateSessionEvent({ type: 'output_end', sequence: '1', bytes: 1, truncated: false } as unknown as SessionEvent, fixture.binding)).toThrow();
    expect(() => validateSessionEvent({ type: 'unexpected' } as unknown as SessionEvent, fixture.binding)).toThrow();
  });
});
