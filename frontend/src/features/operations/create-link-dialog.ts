import m, { type FactoryComponent } from 'mithril';
import { ModalPanel } from 'mithril-materialized';
import { t } from '../../i18n';
import type {
  LinkKind,
  LinkKindOption,
  LinkOptions,
  LinkOptionsRequest,
  LinkRequest,
  LinkRequirement,
  LinkTargetStyle,
  Location,
} from '../../models';
import { validateEntryName } from './create-directory-dialog';

/** The entry a link points at and the directory the link is created in (the other pane). */
export interface CreateLinkDialogRequest {
  readonly target: Location;
  readonly destination: Location;
}

export interface CreateLinkDialogAttrs {
  readonly request: CreateLinkDialogRequest | undefined;
  readonly loadOptions: (request: LinkOptionsRequest) => Promise<LinkOptions>;
  readonly onConfirm: (name: string, link: LinkRequest) => void;
  readonly onCancel: () => void;
}

/**
 * Builds the link request for the chosen option. Kinds without relative support (junctions,
 * shortcuts) always store an absolute target, whatever the style selector last held.
 */
export function linkRequestFor(option: LinkKindOption, targetStyle: LinkTargetStyle): LinkRequest {
  return {
    kind: option.kind,
    targetStyle: option.supportsRelative ? targetStyle : 'absolute',
  };
}

export function linkKindLabel(kind: LinkKind): string {
  switch (kind) {
    case 'symbolicLink':
      return t('operation', 'linkKindSymbolicLink');
    case 'junction':
      return t('operation', 'linkKindJunction');
    case 'shortcut':
      return t('operation', 'linkKindShortcut');
  }
}

export function linkRequirementText(requirement: LinkRequirement): string {
  switch (requirement) {
    case 'developerModeOrAdministrator':
      return t('operation', 'linkRequirementDeveloperMode');
    case 'localDirectoryTarget':
      return t('operation', 'linkRequirementLocalDirectory');
    case 'shellOnly':
      return t('operation', 'linkRequirementShellOnly');
  }
}

function blurActive(): void {
  const active = document.activeElement;
  if (active instanceof HTMLElement) active.blur();
}

/**
 * Create link dialog (Ctrl+Shift+F5, task 0168). Offers only the link kinds the backend reports
 * for this target/destination pair, and shows each kind's preconditions (Windows privilege,
 * junction locality, shell-only `.lnk`) before anything runs.
 */
export const CreateLinkDialog: FactoryComponent<CreateLinkDialogAttrs> = () => {
  let options: readonly LinkKindOption[] | undefined;
  let loadError: string | undefined;
  let selectedKind: LinkKind | undefined;
  let targetStyle: LinkTargetStyle = 'relative';
  let name = '';
  let nameEdited = false;
  let nameError: string | undefined;
  let activeRequest: CreateLinkDialogRequest | undefined;

  function selectedOption(): LinkKindOption | undefined {
    return options?.find((option) => option.kind === selectedKind);
  }

  function selectKind(kind: LinkKind): void {
    selectedKind = kind;
    const option = selectedOption();
    if (option !== undefined && !nameEdited) {
      name = option.suggestedName;
      nameError = validateEntryName(name);
    }
  }

  function load(attrs: CreateLinkDialogAttrs, request: CreateLinkDialogRequest): void {
    options = undefined;
    loadError = undefined;
    selectedKind = undefined;
    targetStyle = 'relative';
    name = '';
    nameEdited = false;
    nameError = undefined;
    attrs.loadOptions({ target: request.target, destination: request.destination }).then(
      (loaded) => {
        if (activeRequest !== request) return;
        options = loaded.kinds;
        const first = loaded.kinds[0];
        if (first !== undefined) selectKind(first.kind);
        m.redraw();
        requestAnimationFrame(() => document.getElementById('create-link-name')?.focus());
      },
      (error: unknown) => {
        if (activeRequest !== request) return;
        loadError = error instanceof Error ? error.message : String(error);
        options = [];
        m.redraw();
      },
    );
  }

  function confirm(attrs: CreateLinkDialogAttrs): void {
    const option = selectedOption();
    nameError = validateEntryName(name);
    if (option === undefined || nameError !== undefined) return;
    blurActive();
    attrs.onConfirm(name, linkRequestFor(option, targetStyle));
  }

  function cancel(attrs: CreateLinkDialogAttrs): void {
    blurActive();
    attrs.onCancel();
  }

  function body(attrs: CreateLinkDialogAttrs): m.Children {
    if (options === undefined) return m('.fm-field-help', t('operation', 'linkLoadingOptions'));
    if (loadError !== undefined) return m('.fm-field-error', loadError);
    if (options.length === 0) return m('.fm-field-help', t('operation', 'linkUnsupported'));
    const option = selectedOption();
    return [
      m('label', [
        m('span', t('operation', 'linkName')),
        m('input#create-link-name', {
          type: 'text',
          value: name,
          required: true,
          'aria-invalid': nameError === undefined ? undefined : 'true',
          oninput: (event: InputEvent) => {
            name = (event.currentTarget as HTMLInputElement).value;
            nameEdited = true;
            nameError = validateEntryName(name);
          },
          onkeydown: (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
              event.stopPropagation();
              cancel(attrs);
            } else if (event.key === 'Enter') {
              event.preventDefault();
              event.stopPropagation();
              confirm(attrs);
            }
          },
        }),
      ]),
      nameError === undefined ? undefined : m('.fm-field-error', nameError),
      m('label', [
        m('span', t('operation', 'linkKind')),
        m(
          'select#create-link-kind',
          {
            value: selectedKind,
            onchange: (event: Event) => {
              selectKind((event.currentTarget as HTMLSelectElement).value as LinkKind);
            },
          },
          options.map((candidate) =>
            m('option', { value: candidate.kind }, linkKindLabel(candidate.kind)),
          ),
        ),
      ]),
      option?.supportsRelative === true
        ? m('label', [
            m('span', t('operation', 'linkTargetStyle')),
            m(
              'select#create-link-target-style',
              {
                value: targetStyle,
                onchange: (event: Event) => {
                  targetStyle = (event.currentTarget as HTMLSelectElement).value as LinkTargetStyle;
                },
              },
              [
                m('option', { value: 'relative' }, t('operation', 'linkTargetRelative')),
                m('option', { value: 'absolute' }, t('operation', 'linkTargetAbsolute')),
              ],
            ),
          ])
        : undefined,
      (option?.requirements ?? []).map((requirement) =>
        m('.fm-field-help', { key: requirement }, linkRequirementText(requirement)),
      ),
    ];
  }

  // ModalPanel stays mounted; load options on each new request (open transition).
  function syncRequest(attrs: CreateLinkDialogAttrs): void {
    if (attrs.request === activeRequest) return;
    activeRequest = attrs.request;
    if (attrs.request !== undefined) load(attrs, attrs.request);
  }

  return {
    oncreate: ({ attrs }) => syncRequest(attrs),
    onupdate: ({ attrs }) => syncRequest(attrs),
    view: ({ attrs }) =>
      m(ModalPanel, {
        title: t('operation', 'createLinkTitle'),
        className: 'fm-dense-modal',
        description: m('.fm-create-directory-field', body(attrs)),
        isOpen: attrs.request !== undefined,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) cancel(attrs);
        },
        buttons: [
          { label: t('button', 'cancel'), onclick: () => cancel(attrs) },
          {
            label: t('button', 'create'),
            disabled: selectedOption() === undefined || validateEntryName(name) !== undefined,
            onclick: () => confirm(attrs),
          },
        ],
      }),
  };
};
