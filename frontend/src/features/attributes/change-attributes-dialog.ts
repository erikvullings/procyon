import m, { type FactoryComponent, type Vnode, type VnodeDOM } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import { errorText, t } from '../../i18n';
import type {
  AttributeChangePreview,
  AttributeScope,
  AttributeUnsupportedReason,
  ChangeAttributes,
  InspectAttributesResult,
  Location,
} from '../../models';
import {
  type AttributeForm,
  applyOctal,
  buildChange,
  formProblems,
  initialForm,
  MODE_BITS,
  modeBitState,
  octalText,
  type TriState,
  type TriValue,
  toggleBit,
  toggleTri,
} from './attribute-form';

/** The entries whose permissions, ownership or attributes are being edited. */
export interface ChangeAttributesRequest {
  readonly files: readonly Location[];
}

export interface ChangeAttributesDialogAttrs {
  readonly request: ChangeAttributesRequest | undefined;
  readonly client: Pick<FileManagerClient, 'inspectAttributes' | 'previewAttributeChange'>;
  readonly onApply: (change: ChangeAttributes, count: number) => void;
  readonly onCancel: () => void;
}

const SCOPES: readonly AttributeScope[] = ['both', 'filesOnly', 'foldersOnly'];
const SCOPE_LABEL = {
  both: 'scopeBoth',
  filesOnly: 'scopeFiles',
  foldersOnly: 'scopeFolders',
} as const;

const UNSUPPORTED_LABEL: Record<
  AttributeUnsupportedReason,
  'unsupportedProvider' | 'unsupportedMixed' | 'unsupportedSymlink'
> = {
  providerUnsupported: 'unsupportedProvider',
  mixedProviders: 'unsupportedMixed',
  symbolicLink: 'unsupportedSymlink',
};

type Who = 'user' | 'group' | 'other';
type Right = 'read' | 'write' | 'execute';

const PERMISSION_ROWS: readonly { who: Who; bits: readonly (readonly [Right, number])[] }[] = [
  {
    who: 'user',
    bits: [
      ['read', MODE_BITS.userRead],
      ['write', MODE_BITS.userWrite],
      ['execute', MODE_BITS.userExecute],
    ],
  },
  {
    who: 'group',
    bits: [
      ['read', MODE_BITS.groupRead],
      ['write', MODE_BITS.groupWrite],
      ['execute', MODE_BITS.groupExecute],
    ],
  },
  {
    who: 'other',
    bits: [
      ['read', MODE_BITS.otherRead],
      ['write', MODE_BITS.otherWrite],
      ['execute', MODE_BITS.otherExecute],
    ],
  },
];

function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

function triCheckbox(
  label: string,
  state: TriState,
  onToggle: () => void,
  showLabel = true,
): Vnode {
  return m('label.fm-attr-check', [
    m('input', {
      type: 'checkbox',
      checked: state === 'on',
      'aria-label': label,
      'aria-checked': state === 'mixed' ? 'mixed' : state === 'on' ? 'true' : 'false',
      oncreate: ({ dom }: VnodeDOM) => {
        (dom as HTMLInputElement).indeterminate = state === 'mixed';
      },
      onupdate: ({ dom }: VnodeDOM) => {
        (dom as HTMLInputElement).indeterminate = state === 'mixed';
      },
      onchange: onToggle,
    }),
    showLabel ? m('span', label) : undefined,
  ]);
}

/**
 * "Change attributes…" (task 0258): rwx/special bits, owner and group, and Windows attributes for
 * a selection, with an optional recursive scope and a count of what will change before anything
 * is written. Only fields the selection's providers support are offered.
 */
export const ChangeAttributesDialog: FactoryComponent<ChangeAttributesDialogAttrs> = () => {
  let inspected: InspectAttributesResult | undefined;
  let form: AttributeForm | undefined;
  let preview: AttributeChangePreview | undefined;
  let failure: string | undefined;
  let inspectToken = 0;
  let previewToken = 0;
  let wasOpen = false;
  let previewedKey = '';

  function load(attrs: ChangeAttributesDialogAttrs): void {
    const request = attrs.request;
    if (request === undefined) return;
    const current = ++inspectToken;
    inspected = undefined;
    form = undefined;
    preview = undefined;
    failure = undefined;
    previewedKey = '';
    attrs.client
      .inspectAttributes(request.files)
      .then((result) => {
        if (current !== inspectToken) return;
        inspected = result;
        form = initialForm(result.entries);
        m.redraw();
      })
      .catch((error: unknown) => {
        if (current !== inspectToken) return;
        failure = errorText(error);
        m.redraw();
      });
  }

  function refreshPreview(attrs: ChangeAttributesDialogAttrs): void {
    const request = attrs.request;
    if (request === undefined || form === undefined) return;
    const key = `${form.recursive}|${form.scope}`;
    if (key === previewedKey) return;
    previewedKey = key;
    const current = ++previewToken;
    preview = undefined;
    attrs.client
      .previewAttributeChange(request.files, form.recursive, form.scope)
      .then((result) => {
        if (current !== previewToken) return;
        preview = result;
        m.redraw();
      })
      .catch(() => {
        if (current !== previewToken) return;
        preview = undefined;
        m.redraw();
      });
  }

  function update(patch: Partial<AttributeForm>): void {
    if (form !== undefined) form = { ...form, ...patch };
  }

  function triRow(label: string, value: TriValue, apply: (next: TriValue) => void): Vnode {
    return triCheckbox(label, value.value, () => apply(toggleTri(value)));
  }

  function permissionsSection(current: AttributeForm): Vnode {
    return m('fieldset.fm-attr-group', [
      m('legend', t('attributes', 'permissions')),
      m('table.fm-attr-matrix', [
        m(
          'thead',
          m('tr', [
            m('th'),
            m('th', t('attributes', 'read')),
            m('th', t('attributes', 'write')),
            m('th', t('attributes', 'execute')),
          ]),
        ),
        m(
          'tbody',
          PERMISSION_ROWS.map(({ who, bits }) =>
            m('tr', { key: who }, [
              m('th', { scope: 'row' }, t('attributes', who)),
              ...bits.map(([right, bit]) =>
                m(
                  'td',
                  triCheckbox(
                    `${t('attributes', who)} ${t('attributes', right)}`,
                    modeBitState(current, bit),
                    () => {
                      form = toggleBit(current, bit);
                    },
                    false,
                  ),
                ),
              ),
            ]),
          ),
        ),
      ]),
      m('.fm-attr-special', [
        triCheckbox(t('attributes', 'setuid'), modeBitState(current, MODE_BITS.setuid), () => {
          form = toggleBit(current, MODE_BITS.setuid);
        }),
        triCheckbox(t('attributes', 'setgid'), modeBitState(current, MODE_BITS.setgid), () => {
          form = toggleBit(current, MODE_BITS.setgid);
        }),
        triCheckbox(t('attributes', 'sticky'), modeBitState(current, MODE_BITS.sticky), () => {
          form = toggleBit(current, MODE_BITS.sticky);
        }),
      ]),
      m('label.fm-create-directory-field', [
        m('span', t('attributes', 'octal')),
        m('input', {
          type: 'text',
          inputmode: 'numeric',
          maxlength: 4,
          placeholder: t('attributes', 'mixedPlaceholder'),
          value: current.octalInput ?? octalText(current),
          oninput: (event: InputEvent) => {
            form = applyOctal(current, (event.currentTarget as HTMLInputElement).value);
          },
        }),
      ]),
    ]);
  }

  function ownershipSection(current: AttributeForm): Vnode {
    const field = (label: string, key: 'uid' | 'gid', hint: number | undefined): Vnode =>
      m('label.fm-create-directory-field', [
        m('span', label),
        m('input', {
          type: 'text',
          inputmode: 'numeric',
          value: current[key],
          placeholder:
            hint === undefined
              ? t('attributes', 'unchanged')
              : t('attributes', 'currentId', { id: hint }),
          oninput: (event: InputEvent) =>
            update({ [key]: (event.currentTarget as HTMLInputElement).value }),
        }),
      ]);
    return m('fieldset.fm-attr-group', [
      m('legend', t('attributes', 'ownership')),
      field(t('attributes', 'owner'), 'uid', current.currentUid),
      field(t('attributes', 'groupId'), 'gid', current.currentGid),
    ]);
  }

  function windowsSection(current: AttributeForm): Vnode {
    return m('fieldset.fm-attr-group', [
      m('legend', t('attributes', 'windowsAttributes')),
      triRow(t('attributes', 'readOnly'), current.readOnly, (readOnly) => update({ readOnly })),
      triRow(t('attributes', 'hidden'), current.hidden, (hidden) => update({ hidden })),
      triRow(t('attributes', 'archive'), current.archive, (archive) => update({ archive })),
    ]);
  }

  function scopeSection(current: AttributeForm, hasFolder: boolean): Vnode {
    return m('fieldset.fm-attr-group', [
      m('legend', t('attributes', 'scope')),
      hasFolder
        ? m('label.fm-attr-check', [
            m('input', {
              type: 'checkbox',
              checked: current.recursive,
              onchange: (event: Event) =>
                update({ recursive: (event.currentTarget as HTMLInputElement).checked }),
            }),
            m('span', t('attributes', 'recursive')),
          ])
        : undefined,
      m(
        'select.browser-default',
        {
          'aria-label': t('attributes', 'scope'),
          value: current.scope,
          disabled: !current.recursive && !hasFolder,
          onchange: (event: Event) =>
            update({ scope: (event.currentTarget as HTMLSelectElement).value as AttributeScope }),
        },
        SCOPES.map((scope) =>
          m(
            'option',
            { value: scope, selected: scope === current.scope },
            t('attributes', SCOPE_LABEL[scope]),
          ),
        ),
      ),
      m('.fm-attr-preview', { role: 'status' }, previewText()),
    ]);
  }

  function previewText(): m.Children {
    if (preview === undefined) return t('attributes', 'counting');
    const lines: m.Children[] = [];
    if (preview.total === 0) lines.push(m('p', t('attributes', 'previewNone')));
    else {
      lines.push(
        m(
          'p',
          preview.truncated
            ? t('attributes', 'previewAtLeast', {
                total: preview.total,
                files: preview.files,
                folders: preview.folders,
              })
            : t('attributes', 'previewCount', {
                total: preview.total,
                files: preview.files,
                folders: preview.folders,
              }),
        ),
      );
    }
    if (preview.skippedSymlinks > 0) {
      lines.push(m('p.fm-attr-note', t('attributes', 'skippedLinks', preview.skippedSymlinks)));
    }
    return lines;
  }

  function body(attrs: ChangeAttributesDialogAttrs): m.Children {
    const request = attrs.request;
    if (request === undefined) return undefined;
    if (failure !== undefined) {
      return m(
        'p.fm-field-error',
        { role: 'alert' },
        t('attributes', 'loadFailed', { message: failure }),
      );
    }
    if (inspected === undefined || form === undefined) {
      return m('p', { role: 'status' }, t('attributes', 'loading'));
    }
    const { support, unsupportedReason, entries } = inspected;
    const intro = m('p', t('attributes', 'selection', request.files.length));
    if (unsupportedReason != null) {
      return [
        intro,
        m(
          'p.fm-field-error',
          { role: 'alert' },
          t('attributes', UNSUPPORTED_LABEL[unsupportedReason]),
        ),
      ];
    }
    const hasFolder = entries.some((entry) => entry.kind === 'directory');
    const problems = formProblems(form, support);
    return m('.fm-attr', [
      intro,
      support.mode ? permissionsSection(form) : undefined,
      support.ownership ? ownershipSection(form) : undefined,
      support.windowsAttributes ? windowsSection(form) : undefined,
      scopeSection(form, hasFolder),
      m('p.fm-attr-note', t('attributes', 'undoNote')),
      problems.map((problem) => m('.fm-field-error', t('attributes', problem))),
    ]);
  }

  function pending(attrs: ChangeAttributesDialogAttrs): ChangeAttributes | undefined {
    if (attrs.request === undefined || inspected === undefined || form === undefined) {
      return undefined;
    }
    if (inspected.unsupportedReason != null) return undefined;
    if (formProblems(form, inspected.support).length > 0) return undefined;
    const change = buildChange(form, inspected.support);
    return change === undefined
      ? undefined
      : { change, recursive: form.recursive, scope: form.scope };
  }

  return {
    onupdate: ({ attrs }) => {
      const open = attrs.request !== undefined;
      if (open && !wasOpen) load(attrs);
      if (!open && wasOpen) {
        inspectToken += 1;
        previewToken += 1;
        inspected = undefined;
        form = undefined;
        preview = undefined;
        failure = undefined;
      }
      if (open) refreshPreview(attrs);
      wasOpen = open;
    },
    oncreate: ({ attrs }) => {
      if (attrs.request !== undefined) {
        wasOpen = true;
        load(attrs);
        refreshPreview(attrs);
      }
    },
    view: ({ attrs }) => {
      const change = pending(attrs);
      return m(ModalPanel, {
        title: t('attributes', 'title'),
        className: 'fm-dense-modal',
        description: body(attrs),
        isOpen: attrs.request !== undefined,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) {
            blurActive();
            attrs.onCancel();
          }
        },
        buttons: [
          {
            label: t('button', 'cancel'),
            onclick: () => {
              blurActive();
              attrs.onCancel();
            },
          },
          {
            label: t('attributes', 'apply'),
            disabled: change === undefined,
            onclick: () => {
              if (change === undefined) return;
              blurActive();
              attrs.onApply(change, preview?.total ?? attrs.request?.files.length ?? 0);
            },
          },
        ],
      });
    },
  };
};
