import { describe, expect, it } from 'vitest';
import type { LlmProfile } from '../../models';
import {
  type AssistantTabEntry,
  canSendDraft,
  oppositePaneId,
  transcriptForRequest,
} from './assistant-tab-state';

const profile = { id: 'p1', name: 'Local' } as LlmProfile;

function entry(overrides: Partial<AssistantTabEntry> = {}): AssistantTabEntry {
  return {
    key: 'right:t1',
    paneId: 'right',
    tabId: 't1',
    sourcePaneId: 'left',
    fileName: 'script.sh',
    profiles: [profile],
    profileId: 'p1',
    messages: [],
    ...overrides,
  };
}

describe('file assistant tab state', () => {
  it('opens the chat in the pane opposite the editor', () => {
    expect(oppositePaneId(['left', 'right'], 'left')).toBe('right');
    expect(oppositePaneId(['left', 'right'], 'right')).toBe('left');
    expect(oppositePaneId(['left'], 'left')).toBeUndefined();
  });

  it('sends only role and content to the backend', () => {
    expect(transcriptForRequest([{ role: 'user', content: 'hi' }])).toEqual([
      { role: 'user', content: 'hi' },
    ]);
  });

  it('allows a send only with text, a profile, and nothing in flight', () => {
    expect(canSendDraft(entry(), 'write a script')).toBe(true);
    expect(canSendDraft(entry(), '   ')).toBe(false);
    expect(canSendDraft(entry({ profileId: '' }), 'write a script')).toBe(false);
    expect(canSendDraft(entry({ pendingRequestId: 'r1' }), 'write a script')).toBe(false);
  });
});
