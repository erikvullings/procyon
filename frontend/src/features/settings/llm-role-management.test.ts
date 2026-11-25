import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import type { LlmApiCapability, LlmProfile } from '../../models';
import { LlmRoleManagement, thinkingSupportText } from './llm-role-management';

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

/** Types into a field the way a person does, so the component sees the input event. */
function setValue(input: HTMLInputElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
  setter?.call(input, value);
  input.dispatchEvent(new Event('input', { bubbles: true }));
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

  it('offers Save role only while the role differs from what is saved', async () => {
    await mountLoaded(new MockFileManagerClient());
    const save = (): HTMLButtonElement | null | undefined =>
      root
        .querySelector<HTMLElement>('[aria-label="Chat and summaries"]')
        ?.querySelector<HTMLButtonElement>('.fm-primary-action');

    expect(save()?.disabled).toBe(true);
    expect(root.textContent).not.toContain('Not saved yet');
  });

  it('explains managed embeddings without offering an unwired provider selector', async () => {
    await mountLoaded(new MockFileManagerClient());

    expect(root.querySelector('[aria-label="Semantic search (embeddings)"]')).not.toBeNull();
    expect(
      root.querySelector('[aria-label="Semantic search (embeddings)"] .select-dropdown'),
    ).toBeNull();
    expect(root.textContent).toContain('The Chat model writes answers.');
    expect(root.textContent).toContain('Terminal assistance.');
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
    // Chat also picks its default thinking effort (task 0361).
    expect(drawn('[aria-label="Chat and summaries"]')).toEqual({ dropdowns: 2, typed: 1 });
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

  describe('naming the drawing model', () => {
    /** An endpoint that reports its models, as one with `modelDiscovery` does. */
    function drawable(name: string, capabilities: LlmApiCapability[]) {
      return {
        name,
        preset: 'mlxServe' as const,
        baseUrl: 'http://127.0.0.1:11234',
        deployment: null,
        apiVersion: null,
        model: 'fox-draw',
        credential: null,
        advanced: {
          contextWindow: 0,
          maximumAnswerTokens: 512,
          temperature: 0,
          timeoutSeconds: 300,
          tlsPolicy: 'requireValidCertificate' as const,
          customHeaders: {},
        },
        capabilities,
        redactFilenames: false,
      };
    }

    /** Picks `name` from the picture row's provider pulldown, and waits for the model field to settle. */
    async function chooseProvider(name: string, picked: 'list' | 'typed'): Promise<void> {
      await choicesOf(fieldOf('[aria-label="Image generation"]', 'Provider'));
      [...document.body.querySelectorAll('ul.dropdown-content li')]
        .find((option) => option.textContent?.trim() === name)
        ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      m.redraw.sync();
      // The models arrive with the endpoint's answer, so the field changes shape a moment later.
      await vi.waitFor(() =>
        expect(
          fieldOf('[aria-label="Image generation"]', 'Model')?.querySelector(
            'button.select-dropdown',
          ) !== null,
        ).toBe(picked === 'list'),
      );
    }

    it('picks the drawing model from what the endpoint reports', async () => {
      const client = new MockFileManagerClient();
      await client.createLlmProfile(drawable('Draw box', ['chatCompletions', 'modelDiscovery']));
      const save = vi.spyOn(client, 'setLlmRole');
      await mountLoaded(client);
      await chooseProvider('Draw box', 'list');

      // the same pulldown the chat model gets, holding what the endpoint listed
      const model = fieldOf('[aria-label="Image generation"]', 'Model');
      expect(
        model?.querySelector('button.select-dropdown'),
        'a drawing model is picked, not copied by hand',
      ).not.toBeNull();
      expect(await choicesOf(model)).toEqual(['fox-draw', 'model-b', 'Type another name…']);
      [...document.body.querySelectorAll('ul.dropdown-content li')]
        .find((option) => option.textContent?.trim() === 'model-b')
        ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      m.redraw.sync();

      root
        .querySelector<HTMLElement>('[aria-label="Image generation"] .fm-primary-action')
        ?.click();
      await vi.waitFor(() => expect(save).toHaveBeenCalledOnce());
      expect(save).toHaveBeenCalledWith('image', expect.objectContaining({ model: 'model-b' }));
    });

    it('still takes a typed name for a model the endpoint does not report', async () => {
      const client = new MockFileManagerClient();
      await client.createLlmProfile(drawable('Draw box', ['chatCompletions', 'modelDiscovery']));
      await mountLoaded(client);
      await chooseProvider('Draw box', 'list');

      const model = fieldOf('[aria-label="Image generation"]', 'Model');
      await choicesOf(model);
      [...document.body.querySelectorAll('ul.dropdown-content li')]
        .find((option) => option.textContent?.trim() === 'Type another name…')
        ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      m.redraw.sync();

      const typed = fieldOf('[aria-label="Image generation"]', 'Model')?.querySelector('input');
      expect(typed, 'a model nobody listed stays nameable').toBeTruthy();
      setValue(typed as HTMLInputElement, 'Aurora XL');
      m.redraw.sync();
      expect(
        root
          .querySelector<HTMLElement>('[aria-label="Image generation"] .fm-primary-action')
          ?.hasAttribute('disabled'),
        'a typed name is enough to save',
      ).toBe(false);
    });

    it('types the drawing model when the endpoint reports nothing to pick from', async () => {
      const client = new MockFileManagerClient();
      await client.createLlmProfile(drawable('Quiet box', ['chatCompletions']));
      await mountLoaded(client);
      await chooseProvider('Quiet box', 'typed');

      const model = fieldOf('[aria-label="Image generation"]', 'Model');
      expect(
        model?.querySelector('button.select-dropdown'),
        'a pulldown of one model would only hide the field',
      ).toBeNull();
      expect(model?.querySelector('input')).toBeTruthy();
    });
  });
});

describe('thinking support line', () => {
  it('says what the model card lists, and when it is only a family guess', () => {
    setLocale('en');
    expect(
      thinkingSupportText({
        thinks: true,
        canTurnOff: true,
        defaultOn: true,
        levels: ['low', 'medium', 'high'],
        verified: true,
        source: 'Qwen/Qwen3.8-27B',
        origin: 'card',
      }),
    ).toBe(
      'Thinking can be turned off; it is on by default. Levels it distinguishes: Low, Medium, High. From the model card of Qwen/Qwen3.8-27B.',
    );
    expect(
      thinkingSupportText({
        thinks: true,
        canTurnOff: true,
        defaultOn: true,
        levels: [],
        verified: false,
        source: 'Qwen (family)',
        origin: 'family',
      }),
    ).toContain(
      'Levels are tried and dropped if the model refuses them. Guessed from its model family',
    );
    expect(
      thinkingSupportText({
        thinks: false,
        canTurnOff: false,
        defaultOn: false,
        levels: [],
        verified: true,
        source: 'meta-llama/Llama-3.2-3B-Instruct',
        origin: 'card',
      }),
    ).toBe('This model does not think. From the model card of meta-llama/Llama-3.2-3B-Instruct.');
    expect(
      thinkingSupportText({
        thinks: true,
        canTurnOff: true,
        defaultOn: true,
        levels: ['low', 'high'],
        verified: true,
        source: 'the chat template served with the model',
        origin: 'template',
        learnedAt: '2026-11-25T17:00:00Z',
      }),
    ).toContain("Read from the model's chat template on this server (2026-11-25).");
  });
});
