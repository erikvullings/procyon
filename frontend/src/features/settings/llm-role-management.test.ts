import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import type { LlmProfile, LlmProfilePreset } from '../../models';
import { LlmRoleManagement, providerChoice } from './llm-role-management';

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

const PRESETS = [
  {
    preset: 'lmStudio',
    name: 'LM Studio',
    baseUrl: 'http://127.0.0.1:1234/v1',
    model: '',
  },
  {
    preset: 'mlxServe',
    name: 'mlx-serve',
    baseUrl: 'http://127.0.0.1:11234',
    model: 'Qwen3-4B-MLX-4bit',
  },
] as unknown as LlmProfilePreset[];

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

  it('hides the embeddings role until advanced is on', async () => {
    await mountLoaded(new MockFileManagerClient());

    expect(root.querySelector('[aria-label="Semantic search (embeddings)"]')).toBeNull();
  });

  it('binds the picture model beside the chat model, not behind a switch', async () => {
    await mountLoaded(new MockFileManagerClient());

    const image = root.querySelector<HTMLElement>('[aria-label="Image generation"]');
    expect(image, 'most chat models cannot draw, so this is its own choice').not.toBeNull();
    expect(root.querySelector('[aria-label="Semantic search (embeddings)"]')).toBeNull();

    // Each row picks its endpoint from a list. Chat then picks the model from a list too, while the
    // picture model is typed: what an endpoint lists for chat is almost never what it can draw with.
    const drawn = (selector: string): { dropdowns: number; typed: number } => {
      const row = root.querySelector<HTMLElement>(selector);
      const inputs = [...(row?.querySelectorAll<HTMLInputElement>('input') ?? [])];
      return {
        dropdowns: inputs.filter((input) => input.classList.contains('select-dropdown')).length,
        typed: inputs.filter(
          (input) => !input.classList.contains('select-dropdown') && input.type === 'text',
        ).length,
      };
    };
    expect(drawn('[aria-label="Chat and summaries"]')).toEqual({ dropdowns: 2, typed: 0 });
    // The picture row adds its own provider choice, then names an endpoint and types a model.
    expect(drawn('[aria-label="Image generation"]')).toEqual({ dropdowns: 2, typed: 1 });
  });

  it('offers every server the app knows as the picture model’s provider', async () => {
    await mountLoaded(new MockFileManagerClient());

    const provider = fieldOf('[aria-label="Image generation"]', 'Provider');
    expect(
      provider,
      'an endpoint that answers chat is rarely one that can draw, so its provider is chosen here',
    ).not.toBeUndefined();
    const choices = await choicesOf(provider);
    expect(choices).toContain('Choose a provider');
    for (const name of ['Ollama', 'LM Studio', 'vLLM', 'mlx-serve', 'OpenAI-compatible']) {
      expect(choices, 'the list is every server the app can talk to, not a curated few').toContain(
        name,
      );
    }
    // The chat row has no provider choice: its endpoint is already named in the section above.
    expect(fieldOf('[aria-label="Chat and summaries"]', 'Provider')).toBeUndefined();
  });

  it('shows no status line for an unassigned or ready role', async () => {
    await mountLoaded(new MockFileManagerClient());

    expect(root.textContent).not.toContain('No endpoint assigned.');
    expect(root.textContent).not.toContain('Ready.');
  });
});

describe('providerChoice', () => {
  it('does nothing for the unassigned choice', () => {
    expect(providerChoice(PRESETS, [], '', '')).toEqual({ kind: 'none' });
  });

  it('binds the endpoint already added for that provider', () => {
    expect(
      providerChoice(PRESETS, [endpoint('p-mlx', 'mlxServe', 'Studio box', '')], 'mlxServe', ''),
    ).toEqual({ kind: 'bind', profileId: 'p-mlx', model: 'Qwen3-4B-MLX-4bit' });
  });

  it('keeps the model the endpoint was saved with over the preset’s own', () => {
    expect(
      providerChoice(
        PRESETS,
        [endpoint('p-mlx', 'mlxServe', 'Studio box', 'qwen-image-2.1-pro')],
        'mlxServe',
        '',
      ),
    ).toEqual({ kind: 'bind', profileId: 'p-mlx', model: 'qwen-image-2.1-pro' });
  });

  it('keeps a model typed by the person over both of those', () => {
    expect(
      providerChoice(
        PRESETS,
        [endpoint('p-mlx', 'mlxServe', 'Studio box', 'flux')],
        'mlxServe',
        'z-image-turbo',
      ),
    ).toEqual({ kind: 'bind', profileId: 'p-mlx', model: 'z-image-turbo' });
  });

  it('asks for an endpoint when the chosen provider has none, naming the provider', () => {
    // The provider may not be running here yet, and a role may not be bound to an endpoint that
    // does not exist - the person is told which one is missing and the editor is filled from it.
    expect(providerChoice(PRESETS, [], 'lmStudio', '')).toEqual({
      kind: 'create',
      name: 'LM Studio',
    });
  });

  it('names an unknown provider by its id rather than inventing a label', () => {
    expect(providerChoice(PRESETS, [], 'somethingNew', '')).toEqual({
      kind: 'create',
      name: 'somethingNew',
    });
  });
});
