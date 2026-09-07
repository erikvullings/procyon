import m, { type FactoryComponent, type Vnode } from 'mithril';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  SemanticOcrJob,
  SemanticOcrOutcome,
  SemanticOcrStatus,
  SemanticOcrTarget,
  SemanticOcrUnavailableReason,
  StartSemanticOcrRemediationRequest,
} from '../../models';

export interface SemanticOcrManagementAttrs {
  readonly client: FileManagerClient;
}

type BusyAction = 'consent' | 'start' | 'cancel';

const INSTALLATION_DOCUMENTATION = 'https://ocrmypdf.readthedocs.io/en/latest/installation.html';
const VISIBLE_FILE_LIMIT = 200;
const VISIBLE_JOB_LIMIT = 20;
const VISIBLE_OUTCOME_LIMIT = 100;

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof error.message === 'string'
  ) {
    return error.message;
  }
  return t('semanticOcr', 'unknownError');
}

function targetKey(target: SemanticOcrTarget): string {
  return `${target.rootId}\u0000${target.location.providerId}\u0000${target.location.uri}`;
}

function decodedLocation(uri: string): string {
  try {
    return decodeURIComponent(uri);
  } catch {
    return uri;
  }
}

function unavailableReason(reason: SemanticOcrUnavailableReason): string {
  switch (reason.code) {
    case 'hostUnavailable':
      return t('semanticOcr', 'hostUnavailable');
    case 'missing':
      return t('semanticOcr', 'missing');
    case 'nonExecutable':
      return t('semanticOcr', 'nonExecutable', { path: reason.path });
    case 'couldNotExecute':
      return t('semanticOcr', 'couldNotExecute');
    case 'malformedVersion':
      return t('semanticOcr', 'malformedVersion');
    case 'unsupportedVersion':
      return t('semanticOcr', 'unsupportedVersion', { version: reason.version });
    case 'timedOut':
      return t('semanticOcr', 'timedOut');
    case 'outputTooLarge':
      return t('semanticOcr', 'outputTooLarge', { limit: reason.limit });
  }
}

function jobStateLabel(job: SemanticOcrJob): string {
  switch (job.state) {
    case 'queued':
      return t('semanticOcr', 'stateQueued');
    case 'running':
      return t('semanticOcr', 'stateRunning');
    case 'completed':
      return t('semanticOcr', 'stateCompleted');
    case 'failed':
      return t('semanticOcr', 'stateFailed');
    case 'cancelled':
      return t('semanticOcr', 'stateCancelled');
  }
}

function outcomeView(outcome: SemanticOcrOutcome): Vnode {
  switch (outcome.outcome) {
    case 'succeeded':
      return m('span.fm-semantic-ocr-success', t('semanticOcr', 'outcomeSucceeded'));
    case 'postOcrNoText':
      return m('span.fm-semantic-ocr-warning', [
        t('semanticOcr', 'outcomePostOcrNoText'),
        m('small', outcome.detail),
      ]);
    case 'executionFailure':
      return m('span.fm-semantic-ocr-error', [
        t('semanticOcr', 'outcomeExecutionFailure'),
        m('small', outcome.detail),
      ]);
    case 'skipped':
      return m('span.fm-semantic-ocr-warning', [
        t('semanticOcr', 'outcomeSkipped'),
        m('small', outcome.detail),
      ]);
  }
}

/** Desktop OCRmyPDF discovery, consent, remediation scopes, and durable jobs. */
export const SemanticOcrManagement: FactoryComponent<SemanticOcrManagementAttrs> = () => {
  let status: SemanticOcrStatus | undefined;
  let loading = true;
  let busy: BusyAction | undefined;
  let error: string | undefined;
  let pollTimer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;
  const selected = new Set<string>();

  function applyStatus(next: SemanticOcrStatus): void {
    status = next;
    const reported = new Set(next.reportedFiles.map(targetKey));
    for (const key of selected) {
      if (!reported.has(key)) selected.delete(key);
    }
  }

  function schedulePoll(attrs: SemanticOcrManagementAttrs): void {
    if (pollTimer !== undefined) clearTimeout(pollTimer);
    pollTimer = undefined;
    if (
      disposed ||
      status?.jobs.some((job) => job.state === 'queued' || job.state === 'running') !== true
    ) {
      return;
    }
    pollTimer = setTimeout(() => void load(attrs, false), 750);
  }

  async function load(attrs: SemanticOcrManagementAttrs, initial = true): Promise<void> {
    if (initial) loading = true;
    try {
      applyStatus(await attrs.client.getSemanticOcrStatus());
      error = undefined;
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      loading = false;
      if (!disposed) {
        schedulePoll(attrs);
        m.redraw();
      }
    }
  }

  async function updateConsent(attrs: SemanticOcrManagementAttrs, enabled: boolean): Promise<void> {
    busy = 'consent';
    error = undefined;
    try {
      applyStatus(await attrs.client.setSemanticOcrConsent(enabled));
    } catch (cause) {
      error = errorMessage(cause);
      await load(attrs, false);
    } finally {
      busy = undefined;
      schedulePoll(attrs);
      m.redraw();
    }
  }

  async function start(
    attrs: SemanticOcrManagementAttrs,
    request: StartSemanticOcrRemediationRequest,
  ): Promise<void> {
    busy = 'start';
    error = undefined;
    try {
      await attrs.client.startSemanticOcrRemediation(request);
      selected.clear();
      await load(attrs, false);
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      busy = undefined;
      schedulePoll(attrs);
      m.redraw();
    }
  }

  async function cancel(attrs: SemanticOcrManagementAttrs, jobId: string): Promise<void> {
    busy = 'cancel';
    error = undefined;
    try {
      await attrs.client.cancelSemanticOcrRemediation(jobId);
      await load(attrs, false);
    } catch (cause) {
      error = errorMessage(cause);
    } finally {
      busy = undefined;
      schedulePoll(attrs);
      m.redraw();
    }
  }

  function remediationControls(
    attrs: SemanticOcrManagementAttrs,
    current: SemanticOcrStatus,
  ): Vnode {
    if (current.reportedFiles.length === 0) {
      return m('p.fm-semantic-ocr-empty', t('semanticOcr', 'noReportedFiles'));
    }
    const groups = new Map<string, SemanticOcrTarget[]>();
    for (const target of current.reportedFiles.slice(0, VISIBLE_FILE_LIMIT)) {
      const files = groups.get(target.rootId) ?? [];
      files.push(target);
      groups.set(target.rootId, files);
    }
    const controlsDisabled = !current.enabled || busy !== undefined;
    const selectedFiles = current.reportedFiles.filter((target) => selected.has(targetKey(target)));
    return m('section.fm-semantic-ocr-reported', { 'aria-labelledby': 'semantic-ocr-files' }, [
      m('h5#semantic-ocr-files', t('semanticOcr', 'reportedFiles')),
      ...[...groups.entries()].map(([rootId, files], rootIndex) =>
        m('fieldset.fm-semantic-ocr-root', [
          m('legend', t('semanticOcr', 'rootLabel', { root: rootId })),
          m(
            'ul',
            files.map((file, fileIndex) => {
              const key = targetKey(file);
              const inputId = `fm-semantic-ocr-file-${rootIndex}-${fileIndex}`;
              return m('li.fm-semantic-ocr-file', { key }, [
                m('label', { for: inputId }, [
                  m('input', {
                    id: inputId,
                    type: 'checkbox',
                    checked: selected.has(key),
                    disabled: controlsDisabled,
                    onchange: (event: Event) => {
                      if ((event.target as HTMLInputElement).checked) selected.add(key);
                      else selected.delete(key);
                    },
                  }),
                  m('span', decodedLocation(file.location.uri)),
                ]),
                m(
                  'button.fm-semantic-ocr-action',
                  {
                    type: 'button',
                    'aria-label': t('semanticOcr', 'runFileFor', {
                      file: decodedLocation(file.location.uri),
                    }),
                    disabled: controlsDisabled,
                    onclick: () => void start(attrs, { scope: 'oneFile', file }),
                  },
                  t('semanticOcr', 'runFile'),
                ),
              ]);
            }),
          ),
          m(
            'button.fm-semantic-ocr-action',
            {
              type: 'button',
              'data-ocr-root': rootId,
              'aria-label': t('semanticOcr', 'runRootFor', { root: rootId }),
              disabled: controlsDisabled,
              onclick: () => void start(attrs, { scope: 'enrolledRoot', rootId }),
            },
            t('semanticOcr', 'runRoot'),
          ),
        ]),
      ),
      current.reportedFiles.length > VISIBLE_FILE_LIMIT
        ? m(
            'p.fm-semantic-ocr-note',
            t('semanticOcr', 'moreFiles', {
              count: current.reportedFiles.length - VISIBLE_FILE_LIMIT,
            }),
          )
        : undefined,
      m('.fm-semantic-ocr-bulk-actions', [
        m(
          'button.fm-semantic-ocr-action',
          {
            type: 'button',
            disabled: controlsDisabled || selectedFiles.length === 0,
            onclick: () => void start(attrs, { scope: 'selectedFiles', files: selectedFiles }),
          },
          t('semanticOcr', 'runSelected'),
        ),
        m(
          'button.fm-semantic-ocr-action',
          {
            type: 'button',
            disabled: controlsDisabled,
            onclick: () => void start(attrs, { scope: 'allReported' }),
          },
          t('semanticOcr', 'runAll'),
        ),
      ]),
    ]);
  }

  function jobsView(attrs: SemanticOcrManagementAttrs, current: SemanticOcrStatus): Vnode {
    return m(
      'section.fm-semantic-ocr-jobs',
      { 'aria-labelledby': 'semantic-ocr-jobs', 'aria-live': 'polite' },
      [
        m('h5#semantic-ocr-jobs', t('semanticOcr', 'jobs')),
        current.jobs.length === 0
          ? m('p', t('semanticOcr', 'noJobs'))
          : m(
              'ol',
              current.jobs.slice(0, VISIBLE_JOB_LIMIT).map((job) =>
                m('li.fm-semantic-ocr-job', { key: job.id }, [
                  m('strong', jobStateLabel(job)),
                  m(
                    'span',
                    t('semanticOcr', 'progress', {
                      completed: job.processedFiles,
                      total: job.totalFiles,
                    }),
                  ),
                  job.availabilityFailure === null
                    ? undefined
                    : m('p.fm-semantic-ocr-error', unavailableReason(job.availabilityFailure)),
                  job.files.length === 0
                    ? undefined
                    : m(
                        'ul',
                        job.files
                          .slice(0, VISIBLE_OUTCOME_LIMIT)
                          .map((file) =>
                            m('li', { key: targetKey(file) }, [
                              m('span.fm-semantic-ocr-path', decodedLocation(file.location.uri)),
                              outcomeView(file.outcome),
                            ]),
                          ),
                      ),
                  job.files.length > VISIBLE_OUTCOME_LIMIT
                    ? m(
                        'p.fm-semantic-ocr-note',
                        t('semanticOcr', 'moreOutcomes', {
                          count: job.files.length - VISIBLE_OUTCOME_LIMIT,
                        }),
                      )
                    : undefined,
                  job.state === 'queued' || job.state === 'running'
                    ? m(
                        'button.fm-semantic-ocr-action',
                        {
                          type: 'button',
                          'aria-label': t('semanticOcr', 'cancelJobNamed', { job: job.id }),
                          disabled: busy !== undefined,
                          onclick: () => void cancel(attrs, job.id),
                        },
                        t('semanticOcr', 'cancelJob'),
                      )
                    : undefined,
                ]),
              ),
            ),
        current.jobs.length > VISIBLE_JOB_LIMIT
          ? m(
              'p.fm-semantic-ocr-note',
              t('semanticOcr', 'moreJobs', {
                count: current.jobs.length - VISIBLE_JOB_LIMIT,
              }),
            )
          : undefined,
      ],
    );
  }

  return {
    oninit: ({ attrs }) => void load(attrs),
    onremove: () => {
      disposed = true;
      if (pollTimer !== undefined) clearTimeout(pollTimer);
    },
    view: ({ attrs }) => {
      if (loading && status === undefined) {
        return m('p.fm-semantic-ocr-loading', { role: 'status' }, t('semanticOcr', 'loading'));
      }
      if (status === undefined) {
        return m('.fm-semantic-ocr-management', [
          m(
            'p.fm-semantic-ocr-error',
            { role: 'alert' },
            error ?? t('semanticOcr', 'unknownError'),
          ),
          m(
            'button.fm-semantic-ocr-action',
            { type: 'button', onclick: () => void load(attrs) },
            t('semanticOcr', 'retry'),
          ),
        ]);
      }
      const current = status;
      const available = current.availability.state === 'available';
      return m(
        'section.fm-semantic-ocr-management.col.s12',
        {
          'aria-labelledby': 'fm-semantic-ocr-heading',
          'aria-busy': busy === undefined ? 'false' : 'true',
        },
        [
          m('h4.fm-settings-section-heading#fm-semantic-ocr-heading', t('semanticOcr', 'title')),
          m('p.fm-semantic-ocr-description', t('semanticOcr', 'description')),
          available
            ? m('dl.fm-semantic-ocr-availability', [
                m('dt', t('semanticOcr', 'available')),
                m(
                  'dd',
                  t('semanticOcr', 'availableVersion', {
                    version: current.availability.version,
                  }),
                ),
                m('dt', t('semanticOcr', 'executable')),
                m('dd.fm-semantic-ocr-path', current.availability.executable),
              ])
            : m('.fm-semantic-ocr-unavailable', { role: 'status' }, [
                m('strong', unavailableReason(current.availability.reason)),
                m('p', current.availability.guidance),
                current.availability.reason.code === 'hostUnavailable'
                  ? undefined
                  : m(
                      'button.fm-semantic-ocr-action',
                      {
                        type: 'button',
                        onclick: () =>
                          void attrs.client
                            .openExternalUrl(INSTALLATION_DOCUMENTATION)
                            .catch((cause: unknown) => {
                              error = errorMessage(cause);
                              m.redraw();
                            }),
                      },
                      t('semanticOcr', 'installationDocs'),
                    ),
              ]),
          m('p#fm-semantic-ocr-consent-help', t('semanticOcr', 'consentDisclosure')),
          m('label.fm-semantic-ocr-consent', { for: 'fm-semantic-ocr-consent' }, [
            m('input', {
              id: 'fm-semantic-ocr-consent',
              type: 'checkbox',
              checked: current.enabled,
              disabled: busy !== undefined || (!available && !current.enabled),
              'aria-describedby': 'fm-semantic-ocr-consent-help',
              onchange: (event: Event) =>
                void updateConsent(attrs, (event.target as HTMLInputElement).checked),
            }),
            m('span', t('semanticOcr', 'consentLabel')),
          ]),
          remediationControls(attrs, current),
          jobsView(attrs, current),
          error === undefined ? undefined : m('p.fm-semantic-ocr-error', { role: 'alert' }, error),
        ],
      );
    },
  };
};
