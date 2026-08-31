import m, { type FactoryComponent } from 'mithril';
import {
  FlatButton,
  ModalPanel,
  NumberInput,
  PasswordInput,
  Select,
  Switch,
  TextInput,
  toast,
} from 'mithril-materialized';

import './connection-editor.css';

import { t } from '../../i18n';
import type {
  BeginOneDriveAuthorizationResponse,
  Connection,
  ConnectionConfiguration,
  ConnectionId,
  ConnectionKind,
  ConnectionSecretInput,
  HostKeyPolicy,
  HostKeyProbe,
  OneDriveAuthorizationAttempt,
  OneDriveAuthorizationErrorCode,
  SshAuthenticationMethod,
  WebDavAuthenticationScheme,
} from '../../models';
import { defaultSshConfiguration } from '../../models/connection';
import {
  type ConnectionSaveDraft,
  type ConnectionSaveResult,
  connectionStatusGlyph,
  connectionStatusLabel,
  validateConnectionDraft,
} from './connections-model';

export interface ConnectionsManagerAttrs {
  readonly open: boolean;
  readonly connections: readonly Connection[];
  /** Reloads the current connection list from the backend on modal open. */
  readonly onRefresh: () => Promise<void>;
  readonly onClose: () => void;
  readonly onSave: (
    draft: ConnectionSaveDraft,
    editingId?: ConnectionId,
  ) => Promise<ConnectionSaveResult>;
  readonly onDelete: (id: ConnectionId) => Promise<void>;
  readonly onConnect: (id: ConnectionId) => Promise<Connection>;
  readonly onDisconnect: (id: ConnectionId) => Promise<Connection>;
  readonly onTest: (id: ConnectionId) => Promise<Connection>;
  /** Probes an SSH connection's presented host key (task 0104, spec §6.4). */
  readonly onProbeHostKey: (id: ConnectionId) => Promise<HostKeyProbe>;
  /** Accepts and persists a host-key fingerprint the user has just confirmed. */
  readonly onAcceptHostKey: (id: ConnectionId, fingerprint: string) => Promise<void>;
  readonly onBeginOneDriveAuthorization: (
    id: ConnectionId,
  ) => Promise<BeginOneDriveAuthorizationResponse>;
  readonly onGetOneDriveAuthorizationAttempt: (
    attemptId: string,
  ) => Promise<OneDriveAuthorizationAttempt>;
  readonly onCancelOneDriveAuthorization: (
    attemptId: string,
  ) => Promise<OneDriveAuthorizationAttempt>;
  readonly onOneDriveAuthorized: (connection: Connection, openAfterAuthorization: boolean) => void;
}

/**
 * Host/username/secret fields are technical identifiers, not prose: the
 * browser's autocorrect/autocapitalize/spell-check would otherwise silently
 * mangle values like `erik` -> `Erik` or flag `sftp.example.test` as
 * misspelled while the user is typing. `InputAttrs` extends Mithril's
 * `Attributes`, so these pass straight through to the underlying `<input>`.
 */
const TECHNICAL_TEXT_ATTRS = {
  autocomplete: 'off',
  autocapitalize: 'off',
  autocorrect: 'off',
  spellcheck: false,
} as const;

type ViewMode =
  | { readonly kind: 'list' }
  | { readonly kind: 'form'; readonly editingId?: ConnectionId };

interface FormState {
  name: string;
  configuration: ConnectionConfiguration;
  secretPassword: string;
  /** Whether the private-key field below is a filesystem path (the default,
   * matching how `ssh`'s own `IdentityFile` works and read fresh on every
   * dial - see `fm-application`'s `ssh.rs`) or pasted key content. */
  secretKeyMode: 'path' | 'paste';
  secretKeyPath: string;
  secretKey: string;
  secretPassphrase: string;
  secretAccessKey: string;
}

// Plain (non-`readonly`) arrays: `mithril-materialized`'s `Select` requires
// a mutable `InputOption<T>[]`, which a `readonly` array type is not
// assignable to.
function kindOptions(): { id: ConnectionKind; label: string }[] {
  return [
    { id: 'ssh', label: t('connections', 'kindSsh') },
    { id: 'ftp', label: t('connections', 'kindFtp') },
    { id: 'ftps', label: t('connections', 'kindFtps') },
    { id: 'oneDrive', label: t('connections', 'kindOneDrive') },
    { id: 'webDav', label: t('connections', 'kindWebDav') },
    { id: 's3', label: t('connections', 'kindS3') },
    { id: 'smb', label: t('connections', 'kindSmb') },
  ];
}

function authenticationOptions(): { id: SshAuthenticationMethod; label: string }[] {
  return [
    { id: 'password', label: t('connections', 'authPassword') },
    { id: 'privateKey', label: t('connections', 'authPrivateKey') },
    { id: 'agent', label: t('connections', 'authAgent') },
  ];
}

function webDavAuthenticationOptions(): { id: WebDavAuthenticationScheme; label: string }[] {
  return [
    { id: 'basic', label: t('connections', 'webDavAuthBasic') },
    { id: 'digest', label: t('connections', 'webDavAuthDigest') },
  ];
}

function hostKeyPolicyOptions(): { id: HostKeyPolicy; label: string }[] {
  return [
    { id: 'promptOnFirstUse', label: t('connections', 'hostKeyPromptOnFirstUse') },
    { id: 'requireKnownHost', label: t('connections', 'hostKeyRequireKnownHost') },
  ];
}

function defaultConfigurationFor(kind: ConnectionKind): ConnectionConfiguration {
  switch (kind) {
    case 'ssh':
      return defaultSshConfiguration();
    case 'ftp':
      return { kind: 'ftp', host: '', port: 21, username: '', startPath: null };
    case 'ftps':
      return { kind: 'ftps', host: '', port: 21, username: '', startPath: null };
    case 'oneDrive':
      return { kind: 'oneDrive', accountHint: null };
    case 'webDav':
      return {
        kind: 'webDav',
        baseUrl: '',
        username: '',
        authentication: 'basic',
        pathPrefix: null,
      };
    case 's3':
      return {
        kind: 's3',
        bucket: '',
        accessKeyId: '',
        region: null,
        endpoint: null,
        startPath: null,
      };
    case 'smb':
      return { kind: 'smb', server: '', share: '' };
    default: {
      const exhaustive: never = kind;
      return exhaustive;
    }
  }
}

function emptyForm(): FormState {
  return {
    name: '',
    configuration: defaultSshConfiguration(),
    secretPassword: '',
    secretKeyMode: 'path',
    secretKeyPath: '',
    secretKey: '',
    secretPassphrase: '',
    secretAccessKey: '',
  };
}

function formFromConnection(connection: Connection): FormState {
  return {
    name: connection.name,
    // Secret fields are always write-only and never pre-filled from a
    // stored connection (task 0103's explicit requirement).
    configuration: connection.configuration,
    secretPassword: '',
    secretKeyMode: 'path',
    secretKeyPath: '',
    secretKey: '',
    secretPassphrase: '',
    secretAccessKey: '',
  };
}

/** Builds the write-only secret input for the current form state, or `undefined` if none was entered. */
function secretInputFrom(form: FormState): ConnectionSecretInput | undefined {
  if (
    form.configuration.kind === 'ftp' ||
    form.configuration.kind === 'ftps' ||
    form.configuration.kind === 'webDav'
  ) {
    return form.secretPassword.length === 0
      ? undefined
      : { kind: 'password', password: form.secretPassword };
  }
  if (form.configuration.kind === 's3') {
    return form.secretAccessKey.length === 0
      ? undefined
      : {
          kind: 'accessKey',
          accessKeyId: form.configuration.accessKeyId,
          secretAccessKey: form.secretAccessKey,
        };
  }
  if (form.configuration.kind !== 'ssh') return undefined;
  switch (form.configuration.authentication) {
    case 'password':
      return form.secretPassword.length === 0
        ? undefined
        : { kind: 'password', password: form.secretPassword };
    case 'privateKey':
      if (form.secretKeyMode === 'path') {
        return form.secretKeyPath.trim().length === 0
          ? undefined
          : {
              kind: 'privateKeyPath',
              path: form.secretKeyPath.trim(),
              passphrase: form.secretPassphrase.length === 0 ? null : form.secretPassphrase,
            };
      }
      return form.secretKey.length === 0
        ? undefined
        : {
            kind: 'privateKey',
            key: form.secretKey,
            passphrase: form.secretPassphrase.length === 0 ? null : form.secretPassphrase,
          };
    case 'agent':
      return undefined;
    default:
      return undefined;
  }
}

/** Clears secret fields from in-memory form state, e.g. immediately after a successful save. */
function clearSecretFields(form: FormState): void {
  form.secretPassword = '';
  form.secretKeyPath = '';
  form.secretKey = '';
  form.secretPassphrase = '';
  form.secretAccessKey = '';
}

function isConnectedLikeStatus(status: Connection['status']): boolean {
  return status === 'connected' || status === 'connecting' || status === 'reconnecting';
}

function statusActionLabel(status: Connection['status']): string {
  return isConnectedLikeStatus(status)
    ? t('connections', 'disconnect')
    : t('connections', 'connect');
}

interface HostKeyPrompt {
  readonly connectionId: ConnectionId;
  readonly probe: HostKeyProbe;
  /** Which action to retry automatically once the fingerprint is accepted. */
  readonly retry: 'connect' | 'test';
}

type OneDriveAuthorizationUiState =
  | { readonly phase: 'opening' }
  | { readonly phase: 'pending'; readonly attemptId: string }
  | { readonly phase: 'succeeded' }
  | { readonly phase: 'cancelled' }
  | { readonly phase: 'failed'; readonly code?: OneDriveAuthorizationErrorCode };

function oneDriveAuthorizationError(code: OneDriveAuthorizationErrorCode | undefined): string {
  switch (code) {
    case 'accessDenied':
      return t('connections', 'oneDriveErrorAccessDenied');
    case 'invalidGrant':
      return t('connections', 'oneDriveErrorInvalidGrant');
    case 'interactionRequired':
      return t('connections', 'oneDriveErrorInteractionRequired');
    case 'tenantPolicyRejected':
      return t('connections', 'oneDriveErrorTenantPolicy');
    case 'conditionalAccessRequired':
      return t('connections', 'oneDriveErrorConditionalAccess');
    case 'insufficientScope':
      return t('connections', 'oneDriveErrorInsufficientScope');
    case 'timeout':
      return t('connections', 'oneDriveErrorTimeout');
    case 'networkError':
      return t('connections', 'oneDriveErrorNetwork');
    case 'internal':
    case undefined:
      return t('connections', 'oneDriveErrorInternal');
    default: {
      const exhaustive: never = code;
      return exhaustive;
    }
  }
}

/**
 * TC-style connections manager (task 0103): a flat list of saved
 * connections with a status glyph and Connect/Test/Edit/Delete actions, plus
 * an inline add/edit form. Purely presentational - every mutation is
 * delegated to the `on*` callbacks, which the caller wires to
 * `connections-model.ts` and its own list state (spec §3 rule 1: components
 * depend only on the shared client through callbacks, never call it
 * directly).
 */
export const ConnectionsManager: FactoryComponent<ConnectionsManagerAttrs> = () => {
  let mode: ViewMode = { kind: 'list' };
  let form: FormState = emptyForm();
  let busy = false;
  let error: string | undefined;
  let success: string | undefined;
  let hostKeyPrompt: HostKeyPrompt | undefined;
  let hostKeyBusy = false;
  let wasOpen = false;
  const oneDriveAuthorization = new Map<ConnectionId, OneDriveAuthorizationUiState>();
  const oneDrivePollTimers = new Map<ConnectionId, ReturnType<typeof setTimeout>>();
  const oneDriveAuthorizationGeneration = new Map<ConnectionId, number>();
  const newlyCreatedOneDriveConnections = new Set<ConnectionId>();

  function nextOneDriveAuthorizationGeneration(connectionId: ConnectionId): number {
    const generation = (oneDriveAuthorizationGeneration.get(connectionId) ?? 0) + 1;
    oneDriveAuthorizationGeneration.set(connectionId, generation);
    return generation;
  }

  function clearOneDrivePollTimer(connectionId: ConnectionId): void {
    const timer = oneDrivePollTimers.get(connectionId);
    if (timer !== undefined) clearTimeout(timer);
    oneDrivePollTimers.delete(connectionId);
  }

  function pollOneDriveAuthorization(
    attrs: ConnectionsManagerAttrs,
    connectionId: ConnectionId,
    attemptId: string,
  ): void {
    clearOneDrivePollTimer(connectionId);
    attrs.onGetOneDriveAuthorizationAttempt(attemptId).then(
      (attempt) => {
        const current = oneDriveAuthorization.get(connectionId);
        if (
          current?.phase !== 'pending' ||
          current.attemptId !== attemptId ||
          attempt.id !== attemptId
        ) {
          return;
        }
        switch (attempt.status.state) {
          case 'pending':
            oneDrivePollTimers.set(
              connectionId,
              setTimeout(() => pollOneDriveAuthorization(attrs, connectionId, attemptId), 1_000),
            );
            break;
          case 'succeeded':
            oneDriveAuthorization.set(connectionId, { phase: 'succeeded' });
            attrs.onOneDriveAuthorized(
              attempt.status.connection,
              newlyCreatedOneDriveConnections.delete(connectionId),
            );
            break;
          case 'failed':
            oneDriveAuthorization.set(connectionId, {
              phase: 'failed',
              code: attempt.status.code,
            });
            break;
          case 'cancelled':
            oneDriveAuthorization.set(connectionId, { phase: 'cancelled' });
            break;
        }
        m.redraw();
      },
      () => {
        const current = oneDriveAuthorization.get(connectionId);
        if (current?.phase === 'pending' && current.attemptId === attemptId) {
          oneDriveAuthorization.set(connectionId, { phase: 'failed', code: 'networkError' });
          m.redraw();
        }
      },
    );
  }

  function beginOneDriveAuthorization(
    attrs: ConnectionsManagerAttrs,
    connection: Connection,
  ): void {
    clearOneDrivePollTimer(connection.id);
    const generation = nextOneDriveAuthorizationGeneration(connection.id);
    oneDriveAuthorization.set(connection.id, { phase: 'opening' });
    attrs.onBeginOneDriveAuthorization(connection.id).then(
      (begun) => {
        if (
          oneDriveAuthorizationGeneration.get(connection.id) !== generation ||
          oneDriveAuthorization.get(connection.id)?.phase !== 'opening'
        ) {
          void attrs.onCancelOneDriveAuthorization(begun.attemptId);
          return;
        }
        oneDriveAuthorization.set(connection.id, {
          phase: 'pending',
          attemptId: begun.attemptId,
        });
        m.redraw();
        pollOneDriveAuthorization(attrs, connection.id, begun.attemptId);
      },
      () => {
        if (
          oneDriveAuthorizationGeneration.get(connection.id) === generation &&
          oneDriveAuthorization.get(connection.id)?.phase === 'opening'
        ) {
          oneDriveAuthorization.set(connection.id, { phase: 'failed' });
          m.redraw();
        }
      },
    );
  }

  function cancelOneDriveAuthorization(
    attrs: ConnectionsManagerAttrs,
    connectionId: ConnectionId,
  ): void {
    const current = oneDriveAuthorization.get(connectionId);
    if (current?.phase !== 'pending') return;
    clearOneDrivePollTimer(connectionId);
    const generation = nextOneDriveAuthorizationGeneration(connectionId);
    oneDriveAuthorization.set(connectionId, { phase: 'cancelled' });
    attrs.onCancelOneDriveAuthorization(current.attemptId).then(
      () => {
        m.redraw();
      },
      () => {
        if (oneDriveAuthorizationGeneration.get(connectionId) === generation) {
          oneDriveAuthorization.set(connectionId, { phase: 'failed', code: 'networkError' });
          m.redraw();
        }
      },
    );
  }

  function cancelPendingOneDriveAuthorizations(attrs: ConnectionsManagerAttrs): void {
    for (const [connectionId, state] of oneDriveAuthorization) {
      clearOneDrivePollTimer(connectionId);
      nextOneDriveAuthorizationGeneration(connectionId);
      if (state.phase === 'pending') {
        void attrs.onCancelOneDriveAuthorization(state.attemptId);
      }
    }
    oneDriveAuthorization.clear();
  }

  function closeManager(attrs: ConnectionsManagerAttrs): void {
    cancelPendingOneDriveAuthorizations(attrs);
    backToList();
    attrs.onClose();
  }

  function oneDriveAuthorizationMessage(
    state: OneDriveAuthorizationUiState | undefined,
  ): string | undefined {
    switch (state?.phase) {
      case 'opening':
        return t('connections', 'oneDriveOpeningBrowser');
      case 'pending':
        return t('connections', 'oneDriveWaiting');
      case 'succeeded':
        return t('connections', 'oneDriveConnected');
      case 'cancelled':
        return t('connections', 'oneDriveCancelled');
      case 'failed':
        return oneDriveAuthorizationError(state.code);
      case undefined:
        return undefined;
    }
  }

  function refreshConnections(attrs: ConnectionsManagerAttrs): void {
    busy = true;
    error = undefined;
    success = undefined;
    attrs.onRefresh().then(
      () => {
        busy = false;
        m.redraw();
      },
      (caught: unknown) => {
        busy = false;
        error = errorMessage(caught, t('connections', 'refreshFailed'));
        m.redraw();
      },
    );
  }

  function openCreateForm(): void {
    mode = { kind: 'form' };
    form = emptyForm();
    error = undefined;
    success = undefined;
  }

  function openEditForm(connection: Connection): void {
    mode = { kind: 'form', editingId: connection.id };
    form = formFromConnection(connection);
    error = undefined;
    success = undefined;
  }

  function backToList(): void {
    mode = { kind: 'list' };
    error = undefined;
    success = undefined;
  }

  function updateConfiguration(patch: Partial<ConnectionConfiguration>): void {
    form = {
      ...form,
      configuration: { ...form.configuration, ...patch } as ConnectionConfiguration,
    };
  }

  function errorMessage(caught: unknown, fallback: string): string {
    return caught instanceof Error ? caught.message : fallback;
  }

  function handleSave(attrs: ConnectionsManagerAttrs): void {
    if (mode.kind !== 'form') return;
    const validationErrors = validateConnectionDraft(form);
    if (validationErrors.length > 0) {
      error = validationErrors[0]?.message;
      return;
    }
    busy = true;
    error = undefined;
    success = undefined;
    const editingId = mode.editingId;
    attrs
      .onSave(
        {
          name: form.name,
          configuration: form.configuration,
          secret: secretInputFrom(form) ?? null,
        },
        editingId,
      )
      .then((result) => {
        busy = false;
        if (!result.ok) {
          error = result.message;
        } else {
          if (editingId === undefined && result.connection.kind === 'oneDrive') {
            newlyCreatedOneDriveConnections.add(result.connection.id);
          }
          clearSecretFields(form);
          backToList();
        }
        m.redraw();
      });
  }

  function handleDelete(attrs: ConnectionsManagerAttrs, connection: Connection): void {
    busy = true;
    success = undefined;
    attrs.onDelete(connection.id).then(
      () => {
        busy = false;
        m.redraw();
      },
      (caught: unknown) => {
        busy = false;
        error = errorMessage(caught, t('connections', 'deleteFailed'));
        m.redraw();
      },
    );
  }

  /**
   * A `connect`/`test` attempt against a host whose key was never accepted
   * (or has changed) does not throw - it comes back with a distinct
   * `hostKeyUnverified`/`hostKeyMismatch` status instead (spec §6.4's
   * mandatory explicit confirmation, never a silent accept or a silent
   * failure). Detect that here and fetch the fingerprint to show the user,
   * rather than treating the call as having simply "done nothing".
   */
  function checkForPendingHostKeyConfirmation(
    attrs: ConnectionsManagerAttrs,
    updated: Connection,
    retry: 'connect' | 'test',
  ): void {
    if (updated.status !== 'hostKeyUnverified' && updated.status !== 'hostKeyMismatch') return;
    attrs.onProbeHostKey(updated.id).then(
      (probe) => {
        hostKeyPrompt = { connectionId: updated.id, probe, retry };
        m.redraw();
      },
      (caught: unknown) => {
        error = errorMessage(caught, t('connections', 'hostKeyCheckFailed'));
        m.redraw();
      },
    );
  }

  function handleToggleConnection(attrs: ConnectionsManagerAttrs, connection: Connection): void {
    busy = true;
    error = undefined;
    success = undefined;
    const action = isConnectedLikeStatus(connection.status)
      ? attrs.onDisconnect(connection.id)
      : attrs.onConnect(connection.id);
    action.then(
      (updated) => {
        busy = false;
        checkForPendingHostKeyConfirmation(attrs, updated, 'connect');
        m.redraw();
      },
      (caught: unknown) => {
        busy = false;
        error = errorMessage(caught, t('connections', 'statusChangeFailed'));
        m.redraw();
      },
    );
  }

  function handleTest(attrs: ConnectionsManagerAttrs, connection: Connection): void {
    busy = true;
    error = undefined;
    success = undefined;
    attrs.onTest(connection.id).then(
      (updated) => {
        busy = false;
        checkForPendingHostKeyConfirmation(attrs, updated, 'test');
        if (updated.status === 'connected') {
          success = t('connections', 'testSucceeded', { name: updated.name });
          toast({ html: success });
        }
        m.redraw();
      },
      (caught: unknown) => {
        busy = false;
        error = errorMessage(caught, t('connections', 'testFailed'));
        m.redraw();
      },
    );
  }

  /**
   * Persists the fingerprint the user just confirmed, then automatically
   * retries the connect/test attempt that surfaced the prompt - the user
   * should not have to click Connect/Test a second time after trusting a
   * host key.
   */
  function handleAcceptHostKey(attrs: ConnectionsManagerAttrs): void {
    if (hostKeyPrompt === undefined) return;
    const { connectionId, probe, retry } = hostKeyPrompt;
    hostKeyBusy = true;
    attrs.onAcceptHostKey(connectionId, probe.fingerprint).then(
      () => {
        hostKeyBusy = false;
        hostKeyPrompt = undefined;
        m.redraw();
        const retryAction =
          retry === 'connect' ? attrs.onConnect(connectionId) : attrs.onTest(connectionId);
        retryAction.then(
          () => m.redraw(),
          (caught: unknown) => {
            error = errorMessage(caught, t('connections', 'hostKeyAcceptedButFailed'));
            m.redraw();
          },
        );
      },
      (caught: unknown) => {
        hostKeyBusy = false;
        error = errorMessage(caught, t('connections', 'hostKeyAcceptFailed'));
        m.redraw();
      },
    );
  }

  /** Dismisses the prompt without accepting anything - the connection stays unverified. */
  function handleCancelHostKey(): void {
    hostKeyPrompt = undefined;
    m.redraw();
  }

  function renderHostKeyPrompt(attrs: ConnectionsManagerAttrs, connection: Connection) {
    if (hostKeyPrompt === undefined || hostKeyPrompt.connectionId !== connection.id)
      return undefined;
    const { probe } = hostKeyPrompt;
    const isMismatch = probe.status === 'mismatch';
    return m('.fm-hostkey-prompt', { role: 'alertdialog' }, [
      isMismatch
        ? m(
            'p.fm-hostkey-warning',
            t('connections', 'hostKeyChangedWarning', { name: connection.name }),
          )
        : m('p', t('connections', 'hostKeyFirstWarning', { name: connection.name })),
      m('p.fm-hostkey-fingerprint', [t('connections', 'presented'), m('code', probe.fingerprint)]),
      isMismatch
        ? m('p.fm-hostkey-fingerprint', [
            t('connections', 'previouslyAccepted'),
            m('code', probe.expectedFingerprint),
          ])
        : undefined,
      m('.fm-hostkey-actions', [
        m(FlatButton, {
          label: isMismatch ? t('connections', 'trustNewKey') : t('connections', 'trustHostKey'),
          disabled: hostKeyBusy,
          onclick: () => handleAcceptHostKey(attrs),
        }),
        m(FlatButton, {
          label: t('button', 'cancel'),
          disabled: hostKeyBusy,
          onclick: handleCancelHostKey,
        }),
      ]),
    ]);
  }

  function renderList(attrs: ConnectionsManagerAttrs) {
    return m('.fm-connections-list', [
      attrs.connections.length === 0
        ? m('p.fm-connections-empty', t('connections', 'noConnections'))
        : m(
            'ul.fm-connections-rows',
            attrs.connections.map((connection) =>
              m('li.fm-connections-row', { key: connection.id }, [
                m(
                  'span.fm-connections-status',
                  {
                    title: connectionStatusLabel(connection.status),
                    'aria-label': connectionStatusLabel(connection.status),
                  },
                  connectionStatusGlyph(connection.status),
                ),
                m('span.fm-connections-name', connection.name),
                m('span.fm-connections-kind', connection.kind),
                m('span.fm-connections-status-label', connectionStatusLabel(connection.status)),
                connection.configuration.kind === 'oneDrive' &&
                (connection.configuration.displayName != null ||
                  connection.configuration.email != null)
                  ? m(
                      '.fm-onedrive-identity',
                      [
                        connection.configuration.displayName,
                        connection.configuration.email,
                        connection.configuration.driveType === null ||
                        connection.configuration.driveType === undefined
                          ? undefined
                          : t(
                              'connections',
                              connection.configuration.driveType === 'personal'
                                ? 'oneDrivePersonal'
                                : connection.configuration.driveType === 'business'
                                  ? 'oneDriveBusiness'
                                  : connection.configuration.driveType === 'documentLibrary'
                                    ? 'oneDriveDocumentLibrary'
                                    : 'oneDriveUnknown',
                            ),
                      ]
                        .filter((part): part is string => part !== null && part !== undefined)
                        .join(' · '),
                    )
                  : undefined,
                m('.fm-connections-actions', [
                  connection.kind === 'oneDrive'
                    ? oneDriveAuthorization.get(connection.id)?.phase === 'pending'
                      ? m(FlatButton, {
                          label: t('connections', 'oneDriveCancelSignIn'),
                          disabled: busy,
                          onclick: () => cancelOneDriveAuthorization(attrs, connection.id),
                        })
                      : m(FlatButton, {
                          label: connection.hasCredential
                            ? t('connections', 'oneDriveReauthorize')
                            : t('connections', 'oneDriveSignIn'),
                          disabled:
                            busy || oneDriveAuthorization.get(connection.id)?.phase === 'opening',
                          onclick: () => beginOneDriveAuthorization(attrs, connection),
                        })
                    : m(FlatButton, {
                        label: statusActionLabel(connection.status),
                        disabled: busy,
                        onclick: () => handleToggleConnection(attrs, connection),
                      }),
                  m(FlatButton, {
                    label: t('connections', 'test'),
                    disabled: busy,
                    onclick: () => handleTest(attrs, connection),
                  }),
                  m(FlatButton, {
                    label: t('action', 'edit'),
                    disabled: busy,
                    onclick: () => openEditForm(connection),
                  }),
                  m(FlatButton, {
                    label: t('action', 'delete'),
                    disabled: busy,
                    onclick: () => handleDelete(attrs, connection),
                  }),
                ]),
                connection.status === 'failed' && connection.lastError != null
                  ? m('.fm-field-error.fm-connections-row-error', connection.lastError)
                  : undefined,
                connection.kind === 'oneDrive' &&
                oneDriveAuthorizationMessage(oneDriveAuthorization.get(connection.id)) !== undefined
                  ? m(
                      oneDriveAuthorization.get(connection.id)?.phase === 'failed'
                        ? '.fm-field-error.fm-connections-row-error'
                        : '.fm-onedrive-authorization-status',
                      {
                        role:
                          oneDriveAuthorization.get(connection.id)?.phase === 'failed'
                            ? 'alert'
                            : 'status',
                      },
                      oneDriveAuthorizationMessage(oneDriveAuthorization.get(connection.id)),
                    )
                  : undefined,
                renderHostKeyPrompt(attrs, connection),
              ]),
            ),
          ),
      m(FlatButton, {
        className: 'fm-connections-add',
        label: t('connections', 'newConnection'),
        onclick: openCreateForm,
      }),
      success === undefined ? undefined : m('.fm-field-success.fm-connections-success', success),
      error === undefined ? undefined : m('.fm-field-error', error),
    ]);
  }

  function renderSshFields(configuration: Extract<ConnectionConfiguration, { kind: 'ssh' }>) {
    return [
      m('.row', [
        m(TextInput, {
          className: 'col s8',
          label: t('connections', 'host'),
          value: configuration.host,
          oninput: (value: string) => updateConfiguration({ host: value }),
          ...TECHNICAL_TEXT_ATTRS,
        }),
        m(NumberInput, {
          className: 'col s4',
          label: t('connections', 'port'),
          value: configuration.port,
          min: 1,
          max: 65_535,
          oninput: (value: number) => updateConfiguration({ port: value }),
        }),
      ]),
      m('.row', [
        m(TextInput, {
          label: t('connections', 'username'),
          value: configuration.username,
          oninput: (value: string) => updateConfiguration({ username: value }),
          ...TECHNICAL_TEXT_ATTRS,
        }),
      ]),
      m('.row', [
        m(TextInput, {
          label: t('connections', 'startFolder'),
          value: configuration.startPath ?? '',
          placeholder: configuration.username
            ? `/home/${configuration.username}`
            : '/home/username',
          helperText: t('connections', 'startFolderHelp'),
          oninput: (value: string) => {
            const trimmed = value.trim();
            updateConfiguration({ startPath: trimmed.length === 0 ? null : trimmed });
          },
          ...TECHNICAL_TEXT_ATTRS,
        }),
      ]),
      m('.row', [
        m(Select<SshAuthenticationMethod>, {
          className: 'col s6',
          label: t('connections', 'authentication'),
          options: authenticationOptions(),
          checkedId: configuration.authentication,
          onchange: (value: SshAuthenticationMethod[]) => {
            const next = value[0];
            if (next !== undefined) updateConfiguration({ authentication: next });
          },
        }),
        m(Select<HostKeyPolicy>, {
          className: 'col s6',
          label: t('connections', 'hostKeyPolicy'),
          options: hostKeyPolicyOptions(),
          checkedId: configuration.hostKeyPolicy,
          onchange: (value: HostKeyPolicy[]) => {
            const next = value[0];
            if (next !== undefined) updateConfiguration({ hostKeyPolicy: next });
          },
        }),
      ]),
      configuration.authentication === 'password'
        ? m('.row', [
            m(PasswordInput, {
              label: t('connections', 'password'),
              value: form.secretPassword,
              placeholder: t('connections', 'passwordPlaceholder'),
              oninput: (value: string) => {
                form.secretPassword = value;
              },
              ...TECHNICAL_TEXT_ATTRS,
            }),
          ])
        : undefined,
      configuration.authentication === 'privateKey'
        ? [
            m('.row', [
              m(Switch, {
                label: t('connections', 'provideKeyAs'),
                left: t('connections', 'keyAsPath'),
                right: t('connections', 'keyAsPaste'),
                checked: form.secretKeyMode === 'paste',
                onchange: (checked: boolean) => {
                  form.secretKeyMode = checked ? 'paste' : 'path';
                },
              }),
            ]),
            form.secretKeyMode === 'path'
              ? m('.row', [
                  m(TextInput, {
                    label: t('connections', 'privateKeyPath'),
                    // Read fresh from disk on every connect/test, like ssh's own
                    // `IdentityFile` - never stored, matching `fm-application`'s
                    // `ssh.rs`. A relative `~/...` path is expanded on whichever
                    // host runs the backend (this machine for the desktop app,
                    // the fm-server host for browser mode).
                    helperText: t('connections', 'privateKeyPathHelp'),
                    placeholder: t('connections', 'privateKeyPathPlaceholder'),
                    value: form.secretKeyPath,
                    oninput: (value: string) => {
                      form.secretKeyPath = value;
                    },
                    ...TECHNICAL_TEXT_ATTRS,
                  }),
                ])
              : m('.row', [
                  m(TextInput, {
                    label: t('connections', 'privateKeyContent'),
                    placeholder: t('connections', 'privateKeyContentPlaceholder'),
                    value: form.secretKey,
                    oninput: (value: string) => {
                      form.secretKey = value;
                    },
                    ...TECHNICAL_TEXT_ATTRS,
                  }),
                ]),
            m('.row', [
              m(PasswordInput, {
                label: t('connections', 'passphrase'),
                value: form.secretPassphrase,
                oninput: (value: string) => {
                  form.secretPassphrase = value;
                },
                ...TECHNICAL_TEXT_ATTRS,
              }),
            ]),
          ]
        : undefined,
    ];
  }

  function renderMinimalFields(configuration: ConnectionConfiguration) {
    switch (configuration.kind) {
      case 'ftp':
      case 'ftps':
        return m('.row', [
          m(TextInput, {
            className: 'col s8',
            label: t('connections', 'host'),
            value: configuration.host,
            oninput: (value: string) => updateConfiguration({ host: value }),
            ...TECHNICAL_TEXT_ATTRS,
          }),
          m(NumberInput, {
            className: 'col s4',
            label: t('connections', 'port'),
            value: configuration.port,
            min: 1,
            max: 65_535,
            oninput: (value: number) => updateConfiguration({ port: value }),
          }),
          m(TextInput, {
            label: t('connections', 'username'),
            value: configuration.username,
            oninput: (value: string) => updateConfiguration({ username: value }),
            ...TECHNICAL_TEXT_ATTRS,
          }),
          m(TextInput, {
            label: t('connections', 'startFolder'),
            value: configuration.startPath ?? '',
            oninput: (value: string) => updateConfiguration({ startPath: value || null }),
            ...TECHNICAL_TEXT_ATTRS,
          }),
          m(PasswordInput, {
            label: t('connections', 'password'),
            placeholder: t('connections', 'passwordPlaceholder'),
            value: form.secretPassword,
            oninput: (value: string) => {
              form.secretPassword = value;
            },
            ...TECHNICAL_TEXT_ATTRS,
          }),
          configuration.kind === 'ftp'
            ? m('.fm-field-warning', { role: 'note' }, t('connections', 'ftpInsecureWarning'))
            : undefined,
        ]);
      case 'oneDrive':
        return m('.row', [
          m(TextInput, {
            label: t('connections', 'account'),
            value: configuration.accountHint ?? '',
            oninput: (value: string) =>
              updateConfiguration({ accountHint: value.length === 0 ? null : value }),
            ...TECHNICAL_TEXT_ATTRS,
          }),
          m('p.fm-onedrive-help', t('connections', 'oneDriveAccountHelp')),
        ]);
      case 'webDav':
        return m('.row', [
          m(TextInput, {
            label: t('connections', 'baseUrl'),
            placeholder: t('connections', 'baseUrlPlaceholder'),
            value: configuration.baseUrl,
            oninput: (value: string) => updateConfiguration({ baseUrl: value }),
            ...TECHNICAL_TEXT_ATTRS,
          }),
          m(TextInput, {
            label: t('connections', 'username'),
            value: configuration.username,
            oninput: (value: string) => updateConfiguration({ username: value }),
            ...TECHNICAL_TEXT_ATTRS,
          }),
          m(Select<WebDavAuthenticationScheme>, {
            label: t('connections', 'authentication'),
            checkedId: configuration.authentication,
            options: webDavAuthenticationOptions(),
            onchange: (value: WebDavAuthenticationScheme[]) => {
              const next = value[0];
              if (next !== undefined) updateConfiguration({ authentication: next });
            },
          }),
          m(TextInput, {
            label: t('connections', 'startFolder'),
            value: configuration.pathPrefix ?? '',
            oninput: (value: string) =>
              updateConfiguration({ pathPrefix: value.length === 0 ? null : value }),
            ...TECHNICAL_TEXT_ATTRS,
          }),
          m(PasswordInput, {
            label: t('connections', 'password'),
            placeholder: t('connections', 'passwordPlaceholder'),
            value: form.secretPassword,
            oninput: (value: string) => {
              form.secretPassword = value;
            },
            ...TECHNICAL_TEXT_ATTRS,
          }),
        ]);
      case 's3':
        return m('.row', [
          m(TextInput, {
            className: 'col s6',
            label: t('connections', 'bucket'),
            value: configuration.bucket,
            oninput: (value: string) => updateConfiguration({ bucket: value }),
            ...TECHNICAL_TEXT_ATTRS,
          }),
          m(TextInput, {
            className: 'col s6',
            label: t('connections', 'accessKeyId'),
            value: configuration.accessKeyId,
            oninput: (value: string) => updateConfiguration({ accessKeyId: value }),
            ...TECHNICAL_TEXT_ATTRS,
          }),
          m(PasswordInput, {
            label: t('connections', 'secretAccessKey'),
            placeholder: t('connections', 'secretAccessKeyPlaceholder'),
            value: form.secretAccessKey,
            oninput: (value: string) => {
              form.secretAccessKey = value;
            },
            ...TECHNICAL_TEXT_ATTRS,
          }),
          m(TextInput, {
            label: t('connections', 'startFolder'),
            value: configuration.startPath ?? '',
            oninput: (value: string) =>
              updateConfiguration({ startPath: value.length === 0 ? null : value }),
            ...TECHNICAL_TEXT_ATTRS,
          }),
          m(TextInput, {
            className: 'col s6',
            label: t('connections', 'region'),
            value: configuration.region ?? '',
            oninput: (value: string) =>
              updateConfiguration({ region: value.length === 0 ? null : value }),
            ...TECHNICAL_TEXT_ATTRS,
          }),
          m(TextInput, {
            className: 'col s6',
            label: t('connections', 'endpoint'),
            value: configuration.endpoint ?? '',
            oninput: (value: string) =>
              updateConfiguration({ endpoint: value.length === 0 ? null : value }),
            ...TECHNICAL_TEXT_ATTRS,
          }),
        ]);
      case 'smb':
        return m('.row', [
          m(TextInput, {
            className: 'col s6',
            label: t('connections', 'server'),
            value: configuration.server,
            oninput: (value: string) => updateConfiguration({ server: value }),
            ...TECHNICAL_TEXT_ATTRS,
          }),
          m(TextInput, {
            className: 'col s6',
            label: t('connections', 'share'),
            value: configuration.share,
            oninput: (value: string) => updateConfiguration({ share: value }),
            ...TECHNICAL_TEXT_ATTRS,
          }),
        ]);
      default:
        return undefined;
    }
  }

  function renderForm(editingId: ConnectionId | undefined) {
    return m('.fm-connection-form', [
      m('.row', [
        m(TextInput, {
          label: t('connections', 'name'),
          value: form.name,
          oninput: (value: string) => {
            form.name = value;
          },
        }),
      ]),
      m('.row', [
        m(Select<ConnectionKind>, {
          label: t('connections', 'kind'),
          // The protocol can't change after creation. `mithril-materialized`'s
          // `disabled` already blocks opening/keyboard interaction and sets
          // `tabindex="-1"` on `.select-wrapper` - it just never sets the
          // underlying `disabled` HTML attribute, so `mithril-materialized-procyon.css`
          // styles that `tabindex` signal directly instead of `:disabled`.
          disabled: editingId !== undefined,
          options: kindOptions(),
          checkedId: form.configuration.kind,
          onchange: (value: ConnectionKind[]) => {
            const next = value[0];
            if (next !== undefined) {
              form = { ...form, configuration: defaultConfigurationFor(next) };
            }
          },
        }),
      ]),
      form.configuration.kind === 'ssh'
        ? renderSshFields(form.configuration)
        : renderMinimalFields(form.configuration),
      error === undefined ? undefined : m('.fm-field-error', error),
    ]);
  }

  return {
    onbeforeupdate: ({ attrs }) => {
      if (attrs.open && !wasOpen) {
        mode = { kind: 'list' };
        hostKeyPrompt = undefined;
        hostKeyBusy = false;
        refreshConnections(attrs);
      }
      wasOpen = attrs.open;
      return true;
    },
    onremove: ({ attrs }) => {
      cancelPendingOneDriveAuthorizations(attrs);
    },
    view: ({ attrs }) =>
      m(ModalPanel, {
        title: t('connections', 'title'),
        className: 'fm-connections-modal',
        description: mode.kind === 'list' ? renderList(attrs) : renderForm(mode.editingId),
        isOpen: attrs.open,
        closeOnEsc: true,
        onToggle: (open: boolean) => {
          if (!open) {
            closeManager(attrs);
          }
        },
        buttons:
          mode.kind === 'list'
            ? [{ label: t('button', 'close'), onclick: () => closeManager(attrs) }]
            : [
                { label: t('button', 'cancel'), onclick: backToList },
                { label: t('button', 'save'), disabled: busy, onclick: () => handleSave(attrs) },
              ],
      }),
  };
};
