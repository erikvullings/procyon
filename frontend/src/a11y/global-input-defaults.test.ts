import { afterEach, describe, expect, it } from 'vitest';

import { installGlobalInputDefaults } from './global-input-defaults';

const settle = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

describe('installGlobalInputDefaults', () => {
  let stop: (() => void) | undefined;
  afterEach(() => {
    stop?.();
    document.body.replaceChildren();
  });

  it('stamps existing and later-added text controls', async () => {
    const existing = document.createElement('input');
    document.body.append(existing);
    stop = installGlobalInputDefaults(document);
    expect(document.documentElement.getAttribute('autocapitalize')).toBe('none');
    const wrapper = document.createElement('div');
    wrapper.innerHTML = '<input type="search"><textarea></textarea>';
    document.body.append(wrapper);
    await settle();
    for (const control of [existing, ...wrapper.querySelectorAll('input', 'textarea')]) {
      expect(control.getAttribute('autocomplete')).toBe('off');
      expect(control.getAttribute('autocapitalize')).toBe('none');
      expect(control.getAttribute('autocorrect')).toBe('off');
      expect(control.getAttribute('spellcheck')).toBe('false');
    }
  });

  it('keeps attributes an element sets itself and skips non-text inputs', async () => {
    document.body.innerHTML =
      '<input type="password" autocomplete="current-password"><input type="checkbox">';
    stop = installGlobalInputDefaults(document);
    await settle();
    const [password, checkbox] = document.querySelectorAll('input');
    expect(password?.getAttribute('autocomplete')).toBe('current-password');
    expect(password?.getAttribute('autocapitalize')).toBe('none');
    expect(checkbox?.hasAttribute('autocomplete')).toBe(false);
  });

  it('covers an input whose type changes from checkbox to text', async () => {
    document.body.innerHTML = '<input type="checkbox">';
    stop = installGlobalInputDefaults(document);
    const input = document.querySelector('input');
    input?.setAttribute('type', 'text');
    await settle();
    expect(input?.getAttribute('autocapitalize')).toBe('none');
  });
});
