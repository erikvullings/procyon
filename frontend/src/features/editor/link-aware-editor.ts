import m, { type FactoryComponent } from 'mithril';
import { t } from '../../i18n';
import { CodeMirrorEditor, type CodeMirrorEditorAttrs } from './code-mirror-editor';
import type { EditorLinkHandlers, ResolvedLink } from './link-actions';
import type { DetectedLink } from './link-detection';

export interface LinkAwareEditorAttrs extends CodeMirrorEditorAttrs {
  readonly links?: EditorLinkHandlers;
}

interface OpenMenu {
  readonly link: DetectedLink;
  readonly x: number;
  readonly y: number;
  /** `undefined` while the path is being checked, `null` when it cannot be acted on. */
  resolved: ResolvedLink | null | undefined;
}

function isLocalFile(link: Extract<ResolvedLink, { kind: 'path' }>): boolean {
  return link.location.providerId === 'local' && link.location.uri.startsWith('file://');
}

/**
 * A code editor whose paths and URLs have a context menu (task 0271): open a URL in the browser,
 * or open, reveal or edit a path. Nothing is executed - paths only navigate or open through the
 * app's existing safe actions, and only http(s) URLs reach the system opener.
 */
export const LinkAwareEditor: FactoryComponent<LinkAwareEditorAttrs> = () => {
  let menu: OpenMenu | undefined;

  function close(): void {
    menu = undefined;
    m.redraw();
  }

  function open(attrs: LinkAwareEditorAttrs, hit: { link: DetectedLink; x: number; y: number }) {
    const handlers = attrs.links;
    if (handlers === undefined) return;
    const opened: OpenMenu = { ...hit, resolved: undefined };
    menu = opened;
    void handlers.resolve(hit.link).then((resolved) => {
      if (menu !== opened) return;
      opened.resolved = resolved ?? null;
      m.redraw();
    });
    m.redraw();
  }

  function items(handlers: EditorLinkHandlers, opened: OpenMenu): m.Children[] {
    const resolved = opened.resolved;
    if (resolved === undefined) return [m('.fm-link-menu-note', t('editorLinks', 'resolving'))];
    if (resolved === null) {
      return [
        m(
          '.fm-link-menu-note',
          opened.link.kind === 'url' ? t('editorLinks', 'unsafeUrl') : t('editorLinks', 'notFound'),
        ),
      ];
    }
    const entry = (label: string, run: () => void) =>
      m(
        'button.fm-link-menu-item',
        {
          type: 'button',
          role: 'menuitem',
          onclick: () => {
            close();
            run();
          },
        },
        label,
      );
    if (resolved.kind === 'url') {
      return [entry(t('editorLinks', 'openUrl'), () => handlers.openUrl(resolved.url))];
    }
    return [
      entry(t('editorLinks', 'openInPane'), () => handlers.openInPane(resolved)),
      ...(isLocalFile(resolved)
        ? [entry(t('editorLinks', 'reveal'), () => handlers.reveal(resolved))]
        : []),
      ...(resolved.entry.kind === 'file'
        ? [
            entry(
              resolved.line === undefined
                ? t('editorLinks', 'editFile')
                : t('editorLinks', 'editFileAtLine', { line: resolved.line }),
              () => handlers.editAt(resolved),
            ),
          ]
        : []),
    ];
  }

  return {
    view: ({ attrs }) => {
      const { links, ...editorAttrs } = attrs;
      return m('.fm-link-aware-editor', [
        m(CodeMirrorEditor, {
          ...editorAttrs,
          ...(links === undefined
            ? {}
            : {
                onLinkContextMenu: (hit: { link: DetectedLink; x: number; y: number }) =>
                  open(attrs, hit),
              }),
        }),
        menu !== undefined && links !== undefined
          ? [
              m('.fm-link-menu-backdrop', {
                onclick: close,
                oncontextmenu: (event: Event) => {
                  event.preventDefault();
                  close();
                },
              }),
              m(
                '.fm-link-menu[role=menu]',
                {
                  'aria-label': t('editorLinks', 'menuLabel'),
                  style: { left: `${menu.x}px`, top: `${menu.y}px` },
                  onkeydown: (event: KeyboardEvent) => {
                    if (event.key === 'Escape') close();
                  },
                },
                items(links, menu),
              ),
            ]
          : undefined,
      ]);
    },
  };
};
