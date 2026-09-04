import { afterEach, describe, expect, it, vi } from 'vitest';

import { beginPointerFileDrag, registerPointerFileDropTarget } from './pointer-file-drag';

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
    window.dispatchEvent(
      new PointerEvent('pointermove', { clientX: -1, clientY: 10, pointerId: 2 }),
    );
    expect(onNativeDragOut).toHaveBeenCalledWith(0);
  });
});
