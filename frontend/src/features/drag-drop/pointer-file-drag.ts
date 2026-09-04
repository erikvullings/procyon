import type { DropModifiers } from './drag-drop';

export interface PointerFileDropTarget {
  readonly onDragOver: (index: number | undefined, modifiers: DropModifiers) => boolean;
  readonly onDrop: (index: number | undefined, modifiers: DropModifiers) => void;
}

export interface PointerFileDragSource {
  readonly index: number;
  readonly onStart: (index: number, modifiers: DropModifiers) => void;
  readonly onNativeDragOut: (index: number) => void;
  readonly effectForModifiers?: (modifiers: DropModifiers) => 'copy' | 'move';
}

const targets = new WeakMap<HTMLElement, PointerFileDropTarget>();
let activeCleanup: (() => void) | undefined;
let suppressClick = false;

function modifiers(event: PointerEvent): DropModifiers {
  return { altKey: event.altKey, ctrlKey: event.ctrlKey, metaKey: event.metaKey };
}

function targetAt(
  x: number,
  y: number,
):
  | {
      element: HTMLElement;
      target: PointerFileDropTarget;
      index: number | undefined;
    }
  | undefined {
  const hit = document.elementFromPoint(x, y);
  const root = hit?.closest<HTMLElement>('[data-file-drop-root]');
  if (root === undefined || root === null) return undefined;
  const target = targets.get(root);
  if (target === undefined) return undefined;
  const item = hit?.closest<HTMLElement>('[data-entry-index]');
  const rawIndex = item?.getAttribute('data-entry-index');
  const index = rawIndex === undefined || rawIndex === null ? undefined : Number(rawIndex);
  return { element: item ?? root, target, index: Number.isFinite(index) ? index : undefined };
}

export function registerPointerFileDropTarget(
  element: HTMLElement,
  target: PointerFileDropTarget,
): () => void {
  element.setAttribute('data-file-drop-root', '');
  targets.set(element, target);
  return () => targets.delete(element);
}

export function beginPointerFileDrag(event: PointerEvent, source: PointerFileDragSource): void {
  if (event.button !== 0) return;
  activeCleanup?.();
  const startX = event.clientX;
  const startY = event.clientY;
  const pointerId = event.pointerId;
  let started = false;
  let highlighted: HTMLElement | undefined;

  const setEffect = (effect?: 'copy' | 'move' | 'none'): void => {
    if (effect === undefined) delete document.documentElement.dataset.fileDragEffect;
    else document.documentElement.dataset.fileDragEffect = effect;
  };

  const highlight = (element?: HTMLElement): void => {
    if (highlighted === element) return;
    highlighted?.classList.remove('fm-drop-target');
    highlighted = element;
    highlighted?.classList.add('fm-drop-target');
  };
  const cleanup = (): void => {
    highlight();
    document.documentElement.classList.remove('fm-pointer-file-dragging');
    setEffect();
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', end);
    window.removeEventListener('pointercancel', cancel);
    window.removeEventListener('pointerout', leaveWindow);
    window.removeEventListener('blur', leaveWindow);
    if (activeCleanup === cleanup) activeCleanup = undefined;
  };
  const start = (current: PointerEvent): boolean => {
    if (started) return true;
    if (Math.hypot(current.clientX - startX, current.clientY - startY) < 5) return false;
    started = true;
    source.onStart(source.index, modifiers(current));
    document.documentElement.classList.add('fm-pointer-file-dragging');
    return true;
  };
  const move = (current: PointerEvent): void => {
    if (current.pointerId !== pointerId || !start(current)) return;
    current.preventDefault();
    if (
      current.clientX < 0 ||
      current.clientY < 0 ||
      current.clientX >= window.innerWidth ||
      current.clientY >= window.innerHeight
    ) {
      handOffToNative();
      return;
    }
    const resolved = targetAt(current.clientX, current.clientY);
    const currentModifiers = modifiers(current);
    const valid = resolved?.target.onDragOver(resolved.index, currentModifiers) === true;
    highlight(valid ? resolved?.element : undefined);
    setEffect(valid ? (source.effectForModifiers?.(currentModifiers) ?? 'move') : 'none');
  };
  const end = (current: PointerEvent): void => {
    if (current.pointerId !== pointerId) return;
    if (started) {
      const resolved = targetAt(current.clientX, current.clientY);
      if (resolved?.target.onDragOver(resolved.index, modifiers(current)) === true) {
        resolved.target.onDrop(resolved.index, modifiers(current));
      }
      suppressClick = true;
      setTimeout(() => {
        suppressClick = false;
      }, 0);
    }
    cleanup();
  };
  const cancel = (current: PointerEvent): void => {
    if (current.pointerId === pointerId) cleanup();
  };
  const handOffToNative = (): void => {
    if (!started) return;
    cleanup();
    source.onNativeDragOut(source.index);
  };
  const leaveWindow = (current: PointerEvent | Event): void => {
    if (current instanceof PointerEvent) {
      if (current.pointerId !== pointerId || current.relatedTarget !== null) return;
    }
    handOffToNative();
  };

  activeCleanup = cleanup;
  window.addEventListener('pointermove', move, { passive: false });
  window.addEventListener('pointerup', end);
  window.addEventListener('pointercancel', cancel);
  window.addEventListener('pointerout', leaveWindow);
  window.addEventListener('blur', leaveWindow);
}

export function consumePointerFileDragClick(): boolean {
  return suppressClick;
}
