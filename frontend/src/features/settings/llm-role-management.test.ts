import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import type { LlmProfile } from '../../models';
import { LlmRoleManagement } from './llm-role-management';

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

async function mountLoaded(
  client: MockFileManagerClient,
  onCreateEndpoint?: (presetId: string) => void,
): Promise<void> {
  m.mount(root, {
    view: () => m(LlmRoleManagement, { client, revision: 0, onCreateEndpoint }),
  });
  await vi.waitFor(() => expect(root.textContent).not.toContain('Loading generation profiles'));
  m.redraw.sync();
}

/** The field of a role row whose own label reads `label` - the rows hold several dropdowns. */
function fieldOf(rowSelector: string, label: string): HTMLElement | undefined {
  const row = root.querySelector<HTMLElement>(rowSelector);
  return [...(row?.querySelectorAll<HTMLElement>('.input-field') ?? [])].find((field) =>
    [...field.querySelectorAll(':scope > label')].some(
      (element) => element.textContent?.trim() === label,
    ),
  );
}

/** The choices a Materialize select offers, read with its portal open. */
async function choicesOf(field: HTMLElement | undefined): Promise<(string | undefined)[]> {
  field?.querySelector<HTMLInputElement>('.select-dropdown')?.click();
  m.redraw.sync();
  await vi.waitFor(() =>
    expect(document.body.querySelectorAll('ul.dropdown-content li').length).toBeGreaterThan(0),
  );
  return [...document.body.querySelectorAll('ul.dropdown-content li')].map((option) =>
    option.textContent?.trim(),
  );
}

function endpoint(id: string, preset: string, name: string, model: string): LlmProfile {
  return {
    id,
    name,
    preset: preset as LlmProfile['preset'],
    baseUrl: 'http://127.0.0.1:11434/v1',
    model,
    capabilities: ['chatCompletions'],
    redactFilenames: false,
    hasCredential: false,
    locality: 'loopback',
    advanced: {
      contextWindow: 0,
      customHeaders: {},
      maximumAnswerTokens: 0,
      temperature: 0,
      timeoutSeconds: 120,
      tlsPolicy: 'requireHttps',
    },
  };
}

describe('LlmRoleManagement', () => {
  it('shows the chat role with one leading secondary and one trailing primary action', async () => {
    await mountLoaded(new MockFileManagerClient());

    const chat = root.querySelector<HTMLElement>('[aria-label="Chat and summaries"]');
    expect(chat).not.toBeNull();
    const actions = [
      ...(chat?.querySelectorAll<HTMLButtonElement>('.fm-form-actions button') ?? []),
    ];
    expect(actions.map((button) => button.textContent?.trim())).toEqual(['Clear', 'Save role']);
    expect(actions[0]?.classList.contains('fm-form-actions-leading')).toBe(true);
    expect(actions[1]?.classList.contains('fm-primary-action')).toBe(true);
  });

  it('shows all roles without an advanced toggle', async () => {
    await mountLoaded(new MockFileManagerClient());

    expect(root.querySelector('[aria-label="Semantic search (embeddings)"]')).not.toBeNull();
  });

  it('binds the picture model beside the chat model, not behind a switch', async () => {
    await mountLoaded(new MockFileManagerClient());

    const image = root.querySelector<HTMLElement>('[aria-label="Image generation"]');
    expect(image, 'most chat models cannot draw, so this is its own choice').not.toBeNull();
    expect(root.querySelector('[aria-label="Semantic search (embeddings)"]')).not.toBeNull();

    // Each row picks its endpoint from a list. Chat then picks the model from a list too, while the
    // picture model is typed: what an endpoint lists for chat is almost never what it can draw with.
    const drawn = (selector: string): { dropdowns: number; typed: number } => {
      const row = root.querySelector<HTMLElement>(selector);
      const inputs = [...(row?.querySelectorAll<HTMLInputElement>('input') ?? [])];
      return {
        dropdowns: row?.querySelectorAll('button.select-dropdown').length ?? 0,
        typed: inputs.filter(
          (input) => !input.classList.contains('select-dropdown') && input.type === 'text',
        ).length,
      };
    };
    expect(drawn('[aria-label="Chat and summaries"]')).toEqual({ dropdowns: 1, typed: 1 });
    // The picture row adds its own provider choice, then names an endpoint and types a model.
    expect(drawn('[aria-label="Image generation"]')).toEqual({ dropdowns: 1, typed: 1 });
  });

  it('offers saved connections and an add action consistently for every role', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'listLlmProfiles').mockResolvedValue([
      endpoint('p-mlx', 'mlxServe', 'Studio box', 'chat-only-model'),
    ]);
    const create = vi.fn();
    await mountLoaded(client, create);
    const provider = fieldOf('[aria-label="Image generation"]', 'Provider');
    expect(await choicesOf(provider)).toEqual(['Not assigned', 'Studio box', 'Add connection…']);
    [...document.body.querySelectorAll('ul.dropdown-content li')]
      .find((option) => option.textContent?.trim() === 'Add connection…')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(create).toHaveBeenCalledOnce();
    expect(fieldOf('[aria-label="Chat and summaries"]', 'Provider')).toBeDefined();
  });

  it('never fills an image role with the selected connection’s chat model', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'listLlmProfiles').mockResolvedValue([
      endpoint('p-mlx', 'mlxServe', 'Studio box', 'chat-only-model'),
    ]);
    await mountLoaded(client);
    await choicesOf(fieldOf('[aria-label="Image generation"]', 'Provider'));
    [...document.body.querySelectorAll('ul.dropdown-content li')]
      .find((option) => option.textContent?.trim() === 'Studio box')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    m.redraw.sync();
    const model = fieldOf('[aria-label="Image generation"]', 'Model')?.querySelector('input');
    expect(model?.disabled).toBe(false);
    expect(model?.value).toBe('');
  });

  it('saves chat independently from the embedding model', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'listLlmProfiles').mockResolvedValue([
      endpoint('p-mlx', 'mlxServe', 'Studio box', 'chat-only-model'),
    ]);
    const save = vi.spyOn(client, 'setLlmRole');
    await mountLoaded(client);
    await choicesOf(fieldOf('[aria-label="Chat and summaries"]', 'Provider'));
    [...document.body.querySelectorAll('ul.dropdown-content li')]
      .find((option) => option.textContent?.trim() === 'Studio box')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    m.redraw.sync();
    root
      .querySelector<HTMLElement>('[aria-label="Chat and summaries"] .fm-primary-action')
      ?.click();
    await vi.waitFor(() => expect(save).toHaveBeenCalledOnce());
    expect(save).toHaveBeenCalledWith(
      'chat',
      expect.objectContaining({ model: 'chat-only-model' }),
    );
  });

  it('shows no status line for an unassigned or ready role', async () => {
    await mountLoaded(new MockFileManagerClient());

    expect(root.textContent).not.toContain('No endpoint assigned.');
    expect(root.textContent).not.toContain('Ready.');
  });
});
