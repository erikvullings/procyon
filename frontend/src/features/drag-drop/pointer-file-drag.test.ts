import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  beginPointerFileDrag,
  consumePointerFileDragClick,
  registerPointerFileDropTarget,
} from './pointer-file-drag';

describe('pointer file drag', () => {
  afterEach(() => {
    document.body.replaceChildren();
    vi.restoreAllMocks();
  });

  it('drops on an in-app row without starting a native drag', () => {
    const root = document.createElement('div');
    const row = document.createElement('div');
    row.dataset.entryIndex = '3';
    root.append(row);
    document.body.append(root);
    const onDrop = vi.fn();
    registerPointerFileDropTarget(root, { onDragOver: () => true, onDrop });
    Object.defineProperty(document, 'elementFromPoint', {
      configurable: true,
      value: vi.fn(() => row),
    });
    const onStart = vi.fn();
    const onNativeDragOut = vi.fn();

    beginPointerFileDrag(
      new PointerEvent('pointerdown', { button: 0, clientX: 10, clientY: 10, pointerId: 1 }),
      { index: 2, onStart, onNativeDragOut },
    );
    window.dispatchEvent(
      new PointerEvent('pointermove', { clientX: 30, clientY: 10, pointerId: 1 }),
    );
    window.dispatchEvent(new PointerEvent('pointerup', { clientX: 30, clientY: 10, pointerId: 1 }));

    expect(onStart).toHaveBeenCalledWith(2, { altKey: false, ctrlKey: false, metaKey: false });
    expect(onDrop).toHaveBeenCalledWith(3, { altKey: false, ctrlKey: false, metaKey: false });
    expect(onNativeDragOut).not.toHaveBeenCalled();
  });

  it('shows copy, move, and cancelled feedback and hands off outside the window', () => {
    const root = document.createElement('div');
    document.body.append(root);
    registerPointerFileDropTarget(root, {
      onDragOver: (_index, state) => !state.ctrlKey,
      onDrop: vi.fn(),
    });
    Object.defineProperty(document, 'elementFromPoint', {
      configurable: true,
      value: vi.fn(() => root),
    });
    const onNativeDragOut = vi.fn();

    beginPointerFileDrag(
      new PointerEvent('pointerdown', { button: 0, clientX: 10, clientY: 10, pointerId: 2 }),
      {
        index: 0,
        onStart: vi.fn(),
        onNativeDragOut,
        effectForModifiers: (state) => (state.metaKey ? 'copy' : 'move'),
      },
    );
    window.dispatchEvent(
      new PointerEvent('pointermove', { clientX: 30, clientY: 10, pointerId: 2 }),
    );
    expect(document.documentElement.dataset.fileDragEffect).toBe('move');
    window.dispatchEvent(
      new PointerEvent('pointermove', { clientX: 30, clientY: 10, metaKey: true, pointerId: 2 }),
    );
    expect(document.documentElement.dataset.fileDragEffect).toBe('copy');
    window.dispatchEvent(
      new PointerEvent('pointermove', { clientX: 30, clientY: 10, ctrlKey: true, pointerId: 2 }),
    );
    expect(document.documentElement.dataset.fileDragEffect).toBe('none');
    expect(document.querySelector('.fm-file-drag-effect')).toBeNull();
    window.dispatchEvent(
      new PointerEvent('pointermove', { clientX: -1, clientY: 10, pointerId: 2 }),
    );
    expect(onNativeDragOut).toHaveBeenCalledWith(0);
  });

  it('updates operation feedback when a modifier changes without mouse movement', () => {
    const root = document.createElement('div');
    document.body.append(root);
    registerPointerFileDropTarget(root, {
      onDragOver: () => true,
      onDrop: vi.fn(),
    });
    Object.defineProperty(document, 'elementFromPoint', {
      configurable: true,
      value: vi.fn(() => root),
    });

    beginPointerFileDrag(
      new PointerEvent('pointerdown', { button: 0, clientX: 10, clientY: 10, pointerId: 3 }),
      {
        index: 0,
        onStart: vi.fn(),
        onNativeDragOut: vi.fn(),
        effectForModifiers: (state) => (state.altKey ? 'copy' : 'move'),
      },
    );
    window.dispatchEvent(
      new PointerEvent('pointermove', { clientX: 30, clientY: 10, pointerId: 3 }),
    );

    expect(document.documentElement.dataset.fileDragEffect).toBe('move');
    const indicator = document.querySelector<HTMLElement>('.fm-file-drag-effect');
    expect(indicator?.classList.contains('fm-file-drag-effect-move')).toBe(true);
    expect(indicator?.textContent).toBe('');
    expect(indicator?.style.left).toBe('44px');
    expect(indicator?.style.top).toBe('26px');

    window.dispatchEvent(new KeyboardEvent('keydown', { altKey: true, key: 'Alt' }));
    expect(document.documentElement.dataset.fileDragEffect).toBe('copy');
    expect(indicator?.classList.contains('fm-file-drag-effect-copy')).toBe(true);
    expect(indicator?.textContent).toBe('');

    window.dispatchEvent(new KeyboardEvent('keyup', { key: 'Alt' }));
    expect(document.documentElement.dataset.fileDragEffect).toBe('move');
    expect(indicator?.classList.contains('fm-file-drag-effect-move')).toBe(true);

    window.dispatchEvent(new PointerEvent('pointerup', { clientX: 30, clientY: 10, pointerId: 3 }));
    expect(document.querySelector('.fm-file-drag-effect')).toBeNull();
  });

  it('cancels an in-app drag with Escape without dropping or starting a native drag', () => {
    const root = document.createElement('div');
    document.body.append(root);
    const onDrop = vi.fn();
    const onNativeDragOut = vi.fn();
    registerPointerFileDropTarget(root, { onDragOver: () => true, onDrop });
    Object.defineProperty(document, 'elementFromPoint', {
      configurable: true,
      value: vi.fn(() => root),
    });
    beginPointerFileDrag(
      new PointerEvent('pointerdown', { button: 0, clientX: 10, clientY: 10, pointerId: 8 }),
      { index: 0, onStart: vi.fn(), onNativeDragOut },
    );
    window.dispatchEvent(
      new PointerEvent('pointermove', { clientX: 30, clientY: 10, pointerId: 8 }),
    );
    const escapeKey = new KeyboardEvent('keydown', { key: 'Escape', cancelable: true });
    window.dispatchEvent(escapeKey);
    window.dispatchEvent(new PointerEvent('pointerup', { pointerId: 8 }));

    expect(escapeKey.defaultPrevented).toBe(true);
    expect(onDrop).not.toHaveBeenCalled();
    expect(onNativeDragOut).not.toHaveBeenCalled();
    expect(document.querySelector('.fm-file-drag-effect')).toBeNull();
    expect(consumePointerFileDragClick()).toBe(true);
  });

  it('suppresses a delayed click after native handoff but preserves the next intentional click', () => {
    beginPointerFileDrag(
      new PointerEvent('pointerdown', { button: 0, clientX: 10, clientY: 10, pointerId: 4 }),
      {
        index: 0,
        onStart: vi.fn(),
        onNativeDragOut: vi.fn(),
      },
    );
    window.dispatchEvent(
      new PointerEvent('pointermove', { clientX: -1, clientY: 10, pointerId: 4 }),
    );

    expect(consumePointerFileDragClick()).toBe(true);
    expect(consumePointerFileDragClick()).toBe(false);

    beginPointerFileDrag(
      new PointerEvent('pointerdown', { button: 0, clientX: 10, clientY: 10, pointerId: 5 }),
      {
        index: 0,
        onStart: vi.fn(),
        onNativeDragOut: vi.fn(),
      },
    );
    expect(consumePointerFileDragClick()).toBe(false);
    window.dispatchEvent(new PointerEvent('pointerup', { pointerId: 5 }));
  });

  it('pauses only the source viewport until the native drag session finishes', async () => {
    const viewport = document.createElement('div');
    viewport.className = 'fm-directory-viewport';
    const row = document.createElement('div');
    viewport.append(row);
    document.body.append(viewport);
    viewport.scrollTop = 150;
    Object.defineProperty(document, 'elementFromPoint', {
      configurable: true,
      value: vi.fn(() => row),
    });
    let finishDrag: (() => void) | undefined;
    const onNativeDragOut = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finishDrag = resolve;
        }),
    );
    row.addEventListener('pointerdown', (event) => {
      beginPointerFileDrag(event, { index: 0, onStart: vi.fn(), onNativeDragOut });
    });

    row.dispatchEvent(
      new PointerEvent('pointerdown', { bubbles: true, button: 0, clientX: 10, pointerId: 6 }),
    );
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: 30, pointerId: 6 }));
    expect(viewport.style.overflowY).toBe('');

    window.dispatchEvent(new PointerEvent('pointermove', { clientX: -1, pointerId: 6 }));
    expect(onNativeDragOut).toHaveBeenCalledExactlyOnceWith(0);
    expect(viewport.style.overflowY).toBe('hidden');
    expect(viewport.scrollTop).toBe(150);
    viewport.scrollTop = 320;
    viewport.dispatchEvent(new Event('scroll'));
    expect(viewport.scrollTop).toBe(150);

    finishDrag?.();
    await vi.waitFor(() => expect(viewport.style.overflowY).toBe(''));
    viewport.scrollTop = 320;
    viewport.dispatchEvent(new Event('scroll'));
    expect(viewport.scrollTop).toBe(320);
  });

  it('hands off when dragging onto the pane status bar before leaving the window', () => {
    const viewport = document.createElement('div');
    viewport.className = 'fm-directory-viewport';
    const row = document.createElement('div');
    viewport.append(row);
    const status = document.createElement('div');
    status.className = 'fm-pane-status';
    document.body.append(viewport, status);
    Object.defineProperty(document, 'elementFromPoint', {
      configurable: true,
      value: vi.fn((_, y: number) => (y > 30 ? status : row)),
    });
    const onNativeDragOut = vi.fn();
    row.addEventListener('pointerdown', (event) =>
      beginPointerFileDrag(event, { index: 1, onStart: vi.fn(), onNativeDragOut }),
    );
    row.dispatchEvent(
      new PointerEvent('pointerdown', { bubbles: true, button: 0, clientX: 10, pointerId: 7 }),
    );
    window.dispatchEvent(new PointerEvent('pointermove', { clientX: 30, pointerId: 7 }));
    window.dispatchEvent(
      new PointerEvent('pointermove', { clientX: 30, clientY: 40, pointerId: 7 }),
    );
    expect(onNativeDragOut).toHaveBeenCalledExactlyOnceWith(1);
  });
});
