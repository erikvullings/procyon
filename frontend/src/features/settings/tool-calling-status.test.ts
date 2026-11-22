import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { SemanticClient } from '../../api/client/semantic-client';
import { setLocale } from '../../i18n';
import { ToolCallingStatus } from './tool-calling-status';

let root: HTMLElement;

beforeEach(() => {
  setLocale('en');
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

async function show(profile: unknown, chat: unknown): Promise<string> {
  const client = {
    listLlmProfiles: () => Promise.resolve(profile === null ? [] : [profile]),
    listLlmRoles: () => Promise.resolve({ chat, embeddings: null, image: null }),
  } as unknown as SemanticClient;
  m.mount(root, { view: () => m(ToolCallingStatus, { client }) });
  await vi.waitFor(() => expect(root.textContent).toContain('Native tool calling'));
  return root.textContent ?? '';
}

describe('ToolCallingStatus', () => {
  const chat = { profileId: 'p1', model: 'm1', allowCloud: false };

  it('says yes when the probe found the chat model takes tools', async () => {
    const text = await show({ id: 'p1', toolCalling: { model: 'm1', supported: true } }, chat);
    expect(text).toContain('yes');
  });

  it('says it uses the text protocol when the model does not', async () => {
    const text = await show({ id: 'p1', toolCalling: { model: 'm1', supported: false } }, chat);
    expect(text).toContain('no (text protocol)');
  });

  it('says not checked yet for a model the probe has not seen', async () => {
    const text = await show({ id: 'p1', toolCalling: { model: 'other', supported: true } }, chat);
    expect(text).toContain('not checked yet');
  });

  it('says when no chat model is assigned', async () => {
    const text = await show(null, null);
    expect(text).toContain('no chat model assigned');
  });
});
