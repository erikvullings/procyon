const FOCUSABLE_SELECTOR = [
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  'a[href]',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

export function createDialogFocusCycle(initialFocus: string) {
  let dialog: HTMLElement | null = null;

  const onKeydown = (event: KeyboardEvent) => {
    if (event.key !== 'Tab' || dialog === null) return;
    const controls = [...dialog.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)].filter(
      (element) => element.getAttribute('aria-hidden') !== 'true',
    );
    if (controls.length === 0) return;
    const currentIndex = controls.indexOf(document.activeElement as HTMLElement);
    const nextIndex = event.shiftKey
      ? (currentIndex <= 0 ? controls.length : currentIndex) - 1
      : (currentIndex + 1) % controls.length;
    event.preventDefault();
    event.stopPropagation();
    controls[nextIndex]?.focus();
  };

  const mount = (dom: Element) => {
    dialog?.removeEventListener('keydown', onKeydown);
    dialog = dom.closest<HTMLElement>('.modal');
    dialog?.addEventListener('keydown', onKeydown);
    dialog?.querySelector<HTMLElement>(initialFocus)?.focus();
  };

  const unmount = () => {
    dialog?.removeEventListener('keydown', onKeydown);
    dialog = null;
  };

  return { mount, unmount };
}
