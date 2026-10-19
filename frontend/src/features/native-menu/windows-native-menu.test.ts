import m from 'mithril';
import { afterEach, describe, expect, it } from 'vitest';

import type { NativeMenuSpec } from '../../models';
import { WindowsNativeMenu } from './windows-native-menu';

const spec: NativeMenuSpec = {
  menus: [
    { title: 'App', items: [] },
    {
      title: 'File',
      items: [{ kind: 'action', id: 'file.new', title: 'New', enabled: true, checked: false }],
    },
  ],
};

function mount(): HTMLElement {
  const root = document.createElement('div');
  const outside = document.createElement('main');
  outside.id = 'outside';
  document.body.append(root, outside);
  m.mount(root, {
    view: () => m(WindowsNativeMenu, { spec, onAction: () => {}, onRole: () => {} }),
  });
  return root;
}

function openFileMenu(root: HTMLElement): void {
  root.querySelector<HTMLButtonElement>('.fm-windows-native-menu-trigger')?.click();
  m.redraw.sync();
  expect(root.querySelector('.fm-windows-native-menu-popup')).not.toBeNull();
}

describe('WindowsNativeMenu', () => {
  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('closes an open menu when the pointer goes down outside it, even without a click', () => {
    const root = mount();
    openFileMenu(root);

    document
      .getElementById('outside')
      ?.dispatchEvent(new Event('pointerdown', { bubbles: true, cancelable: true }));
    m.redraw.sync();

    expect(root.querySelector('.fm-windows-native-menu-popup')).toBeNull();
  });

  it('closes an open menu when the window loses focus', () => {
    const root = mount();
    openFileMenu(root);

    window.dispatchEvent(new Event('blur'));
    m.redraw.sync();

    expect(root.querySelector('.fm-windows-native-menu-popup')).toBeNull();
  });
});
