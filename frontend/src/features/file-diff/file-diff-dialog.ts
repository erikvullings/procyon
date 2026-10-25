import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { errorText, t } from '../../i18n';
import type { DiffRow, DiffSegment, FileDiffResult, Location } from '../../models';
import { calculateVisibleWindow, scrollOffsetForIndex } from '../directory-table/windowing';
import { adjacentChange, changeBlockAt } from './file-diff-actions';
import './file-diff.css';

/** The two files to diff, with the names shown in the header. */
export interface FileDiffRequest {
  readonly left: { readonly location: Location; readonly name: string };
  readonly right: { readonly location: Location; readonly name: string };
}

export interface FileDiffDialogAttrs {
  readonly request: FileDiffRequest | undefined;
  readonly client: Pick<FileManagerClient, 'diffFiles'>;
  readonly onClose: () => void;
}

/** Height of one diff row in pixels; fixed so the list can be virtualized. */
export const DIFF_ROW_HEIGHT = 20;
const OVERSCAN = 12;
const DEFAULT_VIEWPORT_HEIGHT = 480;

function formatBytes(bytes: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${unit === 0 ? value : value.toFixed(1)} ${units[unit]}`;
}

function textCell(
  text: string | null | undefined,
  segments: readonly DiffSegment[],
  side: 'left' | 'right',
): m.Children {
  if (text == null) return undefined;
  if (segments.length === 0) return text;
  return segments.map((segment) =>
    segment.changed ? m(`mark.fm-diff-mark.fm-diff-mark-${side}`, segment.text) : segment.text,
  );
}

function diffRow(row: DiffRow, index: number): m.Vnode {
  return m(
    `.fm-diff-row.fm-diff-${row.kind}`,
    { key: index, role: 'row', style: { height: `${DIFF_ROW_HEIGHT}px` } },
    [
      m('span.fm-diff-line', row.leftLine ?? ''),
      m('span.fm-diff-text.fm-diff-text-left', textCell(row.leftText, row.leftSegments, 'left')),
      m('span.fm-diff-line', row.rightLine ?? ''),
      m(
        'span.fm-diff-text.fm-diff-text-right',
        textCell(row.rightText, row.rightSegments, 'right'),
      ),
    ],
  );
}

/**
 * Read-only side-by-side file diff (task 0256). The backend aligns the lines and marks the
 * intra-line changes; this view only mounts the rows near the scroll position, so large diffs
 * stay cheap, and offers next/previous change navigation plus the ignore options.
 */
export const FileDiffDialog: FactoryComponent<FileDiffDialogAttrs> = () => {
  let result: FileDiffResult | undefined;
  let failure: string | undefined;
  let ignoreWhitespace = false;
  let ignoreCase = false;
  let loadedKey = '';
  let wasOpen = false;
  let abort: AbortController | undefined;
  let scrollTop = 0;
  let viewportHeight = DEFAULT_VIEWPORT_HEIGHT;
  let scroller: HTMLElement | undefined;
  let currentRow = -1;

  function load(attrs: FileDiffDialogAttrs): void {
    const request = attrs.request;
    if (request === undefined) return;
    abort?.abort();
    const controller = new AbortController();
    abort = controller;
    result = undefined;
    failure = undefined;
    currentRow = -1;
    scrollTop = 0;
    if (scroller !== undefined) scroller.scrollTop = 0;
    attrs.client
      .diffFiles(
        request.left.location,
        request.right.location,
        { ignoreWhitespace, ignoreCase },
        controller.signal,
      )
      .then((response) => {
        if (controller.signal.aborted) return;
        result = response;
        m.redraw();
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted) return;
        failure = errorText(error);
        m.redraw();
      });
  }

  function requestKey(attrs: FileDiffDialogAttrs): string {
    const request = attrs.request;
    if (request === undefined) return '';
    return [
      request.left.location.uri,
      request.right.location.uri,
      ignoreWhitespace,
      ignoreCase,
    ].join('|');
  }

  function goToChange(direction: 1 | -1): void {
    if (result === undefined) return;
    const target = adjacentChange(result.changeStarts, currentRow, direction);
    if (target === undefined) return;
    currentRow = target;
    const offset = scrollOffsetForIndex({
      entryCount: result.rows.length,
      rowHeight: DIFF_ROW_HEIGHT,
      scrollTop,
      viewportHeight,
      index: target,
      alignment: 'center',
    });
    scrollTop = offset;
    if (scroller !== undefined) scroller.scrollTop = offset;
  }

  function option(label: string, checked: boolean, onchange: (value: boolean) => void): m.Vnode {
    return m('label.fm-diff-option', [
      m('input', {
        type: 'checkbox',
        checked,
        onchange: (event: Event) => onchange((event.currentTarget as HTMLInputElement).checked),
      }),
      m('span', label),
    ]);
  }

  function body(): m.Children {
    if (failure !== undefined) {
      return m(
        'p.fm-field-error',
        { role: 'alert' },
        t('fileDiff', 'failed', { message: failure }),
      );
    }
    if (result === undefined) return m('p', { role: 'status' }, t('fileDiff', 'loading'));
    if (result.outcome === 'tooLarge') {
      return m(
        'p.fm-diff-notice',
        { role: 'status' },
        t('fileDiff', 'tooLarge', {
          size: formatBytes(result.sizeLimitBytes),
          lines: result.lineLimit,
          left: formatBytes(result.leftSize),
          right: formatBytes(result.rightSize),
        }),
      );
    }
    if (result.outcome === 'binary') {
      return m(
        'p.fm-diff-notice',
        { role: 'status' },
        result.identical
          ? t('fileDiff', 'binaryIdentical')
          : t('fileDiff', 'binaryDifferent', { offset: result.firstDifferenceOffset ?? 0 }),
      );
    }
    const visible = calculateVisibleWindow({
      entryCount: result.rows.length,
      rowHeight: DIFF_ROW_HEIGHT,
      scrollTop,
      viewportHeight,
      overscan: OVERSCAN,
    });
    const blocks = result.changeStarts.length;
    const position = changeBlockAt(result.changeStarts, currentRow);
    return [
      m('.fm-diff-toolbar', [
        m(
          'button.fm-diff-nav',
          {
            type: 'button',
            disabled: blocks === 0,
            onclick: () => goToChange(-1),
          },
          t('fileDiff', 'previousChange'),
        ),
        m(
          'button.fm-diff-nav',
          { type: 'button', disabled: blocks === 0, onclick: () => goToChange(1) },
          t('fileDiff', 'nextChange'),
        ),
        m(
          'span.fm-diff-summary',
          { role: 'status' },
          result.identical
            ? t('fileDiff', 'identical')
            : position < 0
              ? t('fileDiff', 'changes', blocks)
              : t('fileDiff', 'changePosition', { current: position + 1, total: blocks }),
        ),
      ]),
      m(
        '.fm-diff-scroll[role=table]',
        {
          tabindex: 0,
          'aria-label': t('fileDiff', 'title'),
          onscroll: (event: Event) => {
            const element = event.currentTarget as HTMLElement;
            scrollTop = element.scrollTop;
            viewportHeight = element.clientHeight || viewportHeight;
          },
          oncreate: ({ dom }) => {
            scroller = dom as HTMLElement;
            viewportHeight = scroller.clientHeight || viewportHeight;
          },
          onremove: () => {
            scroller = undefined;
          },
        },
        m(
          '.fm-diff-spacer',
          { style: { height: `${visible.totalHeight}px` } },
          m(
            '.fm-diff-window',
            { style: { transform: `translateY(${visible.offsetTop}px)` } },
            result.rows
              .slice(visible.start, visible.end)
              .map((row, offset) => diffRow(row, visible.start + offset)),
          ),
        ),
      ),
    ];
  }

  return {
    onupdate: ({ attrs }) => {
      const open = attrs.request !== undefined;
      const key = requestKey(attrs);
      if (open && (!wasOpen || key !== loadedKey)) {
        loadedKey = key;
        load(attrs);
      }
      if (!open) {
        abort?.abort();
        result = undefined;
        failure = undefined;
        loadedKey = '';
      }
      wasOpen = open;
    },
    onremove: () => abort?.abort(),
    view: ({ attrs }) => {
      const request = attrs.request;
      return m(ModalPanel, {
        title: t('fileDiff', 'title'),
        className: 'fm-dense-modal fm-file-diff-modal',
        description:
          request === undefined
            ? undefined
            : m('.fm-file-diff', [
                m('.fm-diff-header', [
                  m('span.fm-diff-name', request.left.name),
                  m('span.fm-diff-name', request.right.name),
                ]),
                m('.fm-diff-options', [
                  option(t('fileDiff', 'ignoreWhitespace'), ignoreWhitespace, (value) => {
                    ignoreWhitespace = value;
                  }),
                  option(t('fileDiff', 'ignoreCase'), ignoreCase, (value) => {
                    ignoreCase = value;
                  }),
                ]),
                body(),
              ]),
        isOpen: request !== undefined,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) attrs.onClose();
        },
        buttons: [{ label: t('button', 'close'), onclick: attrs.onClose }],
      });
    },
  };
};
