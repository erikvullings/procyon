import m, { type FactoryComponent } from 'mithril';

import { t } from '../../i18n';
import {
  type ChecksumAlgorithm,
  type ChecksumEntry,
  checksumAlgorithmLabel,
  type Location,
  type VerificationReport,
  type VerificationStatus,
} from '../../models';

export interface ChecksumResultsViewAttrs {
  readonly algorithms: readonly ChecksumAlgorithm[];
  readonly entries: readonly ChecksumEntry[];
  readonly totalEntries: number;
  readonly isComplete: boolean;
  readonly isCancelled: boolean;
  readonly verification?: VerificationReport;
  /** Where the results were last written, shown as a confirmation hint. */
  readonly savedTo?: Location;
  readonly error?: string;
  readonly onCopy: (algorithm: ChecksumAlgorithm) => void;
  /** Writes the results to `fileName` in the active pane's directory. */
  readonly onSave: (algorithm: ChecksumAlgorithm, fileName: string) => void;
  /** Default filename offered when the save form opens. */
  readonly suggestedFileName: (algorithm: ChecksumAlgorithm) => string;
  readonly onVerify: (content: string) => void;
  readonly onCancel: () => void;
  readonly onClose: () => void;
}

function verificationLabel(status: VerificationStatus): string {
  switch (status) {
    case 'match':
      return t('checksums', 'verificationMatch');
    case 'mismatch':
      return t('checksums', 'verificationMismatch');
    case 'missing':
      return t('checksums', 'verificationMissing');
  }
}

/** Shortens a digest for the table while keeping both ends recognisable. */
function abbreviate(digest: string): string {
  return digest.length <= 24 ? digest : `${digest.slice(0, 12)}…${digest.slice(-8)}`;
}

function progressLabel(attrs: ChecksumResultsViewAttrs): string {
  if (attrs.isCancelled)
    return t('checksums', 'cancelledAfter', {
      count: attrs.entries.length,
      total: attrs.totalEntries,
    });
  if (attrs.isComplete)
    return t('checksums', 'hashedOfTotal', {
      count: attrs.entries.length,
      total: attrs.totalEntries,
    });
  return t('checksums', 'hashingOfTotal', {
    count: attrs.entries.length,
    total: attrs.totalEntries,
  });
}

/**
 * Presents a checksum job's per-entry results with copy, save-to-file and
 * verify-against-file affordances (spec §18, task 0077).
 *
 * Saving and verifying deliberately go through the caller: this view only
 * asks for the rendered text, so file writing stays on the one audited path
 * (spec §35).
 */
export const ChecksumResultsView: FactoryComponent<ChecksumResultsViewAttrs> = () => {
  let selectedAlgorithm: ChecksumAlgorithm | undefined;
  let verifyText = '';
  // The save form is inline rather than a native dialog: saving goes through
  // the backend's provider WRITE path, so the destination is a name inside the
  // pane's current directory, not an arbitrary OS path (task 0077).
  let saveOpen = false;
  let saveFileName = '';

  return {
    view: ({ attrs }) => {
      const algorithm = selectedAlgorithm ?? attrs.algorithms[0];
      return m('section.checksum-results', { 'aria-label': t('checksums', 'ariaLabel') }, [
        m('header.checksum-results__header', [
          m('h2', t('checksums', 'title')),
          m('span.checksum-results__progress', progressLabel(attrs)),
          !attrs.isComplete &&
            m(
              'button.checksum-results__cancel',
              { type: 'button', onclick: () => attrs.onCancel() },
              t('button', 'cancel'),
            ),
          m(
            'button.checksum-results__close',
            {
              type: 'button',
              'aria-label': t('checksums', 'closeResults'),
              onclick: () => attrs.onClose(),
            },
            t('button', 'close'),
          ),
        ]),

        attrs.error !== undefined && m('p.checksum-results__error', { role: 'alert' }, attrs.error),

        attrs.algorithms.length > 1 &&
          m('label.checksum-results__algorithm', [
            t('checksums', 'algorithmLabel'),
            m(
              'select',
              {
                value: algorithm,
                onchange: (event: Event) => {
                  selectedAlgorithm = (event.target as HTMLSelectElement)
                    .value as ChecksumAlgorithm;
                },
              },
              attrs.algorithms.map((option) =>
                m('option', { value: option }, checksumAlgorithmLabel(option)),
              ),
            ),
          ]),

        m('div.checksum-results__actions', [
          m(
            'button',
            {
              type: 'button',
              disabled: algorithm === undefined || attrs.entries.length === 0,
              onclick: () => algorithm !== undefined && attrs.onCopy(algorithm),
            },
            t('checksums', 'copy'),
          ),
          m(
            'button.checksum-results__save-open',
            {
              type: 'button',
              disabled: algorithm === undefined || attrs.entries.length === 0,
              onclick: () => {
                if (algorithm === undefined) return;
                saveFileName = attrs.suggestedFileName(algorithm);
                saveOpen = true;
              },
            },
            t('checksums', 'saveFile'),
          ),
        ]),

        saveOpen &&
          algorithm !== undefined &&
          m(
            'form.checksum-results__save',
            {
              onsubmit: (event: Event) => {
                event.preventDefault();
                attrs.onSave(algorithm, saveFileName);
                saveOpen = false;
              },
            },
            [
              m('label', [
                t('checksums', 'saveAsInCurrentDirectory'),
                m('input.checksum-results__save-name', {
                  type: 'text',
                  value: saveFileName,
                  oninput: (event: Event) => {
                    saveFileName = (event.target as HTMLInputElement).value;
                  },
                }),
              ]),
              m(
                'button.checksum-results__save-confirm',
                { type: 'submit', disabled: saveFileName.trim() === '' },
                t('button', 'save'),
              ),
              m(
                'button.checksum-results__save-cancel',
                {
                  type: 'button',
                  onclick: () => {
                    saveOpen = false;
                  },
                },
                t('button', 'cancel'),
              ),
            ],
          ),

        attrs.savedTo !== undefined &&
          m(
            'p.checksum-results__saved',
            { role: 'status' },
            t('checksums', 'savedTo', { location: decodeURIComponent(attrs.savedTo.uri) }),
          ),

        m('table.checksum-results__table', [
          m(
            'thead',
            m('tr', [
              m('th', t('checksums', 'columnFile')),
              m('th', t('checksums', 'columnSize')),
              m('th', t('checksums', 'columnDigest')),
            ]),
          ),
          m(
            'tbody',
            attrs.entries.map((entry) =>
              m('tr', { key: entry.location.uri }, [
                m('td', entry.relativePath),
                m('td', t('checksums', 'sizeBytes', { size: entry.size })),
                m(
                  'td.checksum-results__digest',
                  entry.error !== undefined
                    ? m('span.checksum-results__entry-error', entry.error)
                    : m(
                        'code',
                        {
                          title: algorithm === undefined ? '' : (entry.checksums[algorithm] ?? ''),
                        },
                        algorithm === undefined
                          ? ''
                          : abbreviate(entry.checksums[algorithm] ?? '—'),
                      ),
                ),
              ]),
            ),
          ),
        ]),

        m('div.checksum-results__verify', [
          m('h3', t('checksums', 'verifyAgainstFile')),
          m('textarea.checksum-results__verify-input', {
            'aria-label': t('checksums', 'checksumFileContents'),
            rows: 4,
            placeholder: t('checksums', 'verifyPlaceholder'),
            value: verifyText,
            oninput: (event: Event) => {
              verifyText = (event.target as HTMLTextAreaElement).value;
            },
          }),
          m(
            'button',
            {
              type: 'button',
              disabled: verifyText.trim() === '',
              onclick: () => attrs.onVerify(verifyText),
            },
            t('checksums', 'verify'),
          ),
          attrs.verification !== undefined &&
            m('div.checksum-results__verification', [
              m(
                'p.checksum-results__verification-summary',
                t('checksums', 'verificationSummary', {
                  matched: attrs.verification.matched,
                  mismatched: attrs.verification.mismatched,
                  missing: attrs.verification.missing,
                }),
              ),
              m(
                'ul',
                attrs.verification.results.map((result) =>
                  m(
                    'li',
                    {
                      key: result.path,
                      class: `checksum-results__verification--${result.status}`,
                    },
                    t('checksums', 'verificationResultLine', {
                      path: result.path,
                      label: verificationLabel(result.status),
                    }),
                  ),
                ),
              ),
            ]),
        ]),
      ]);
    },
  };
};
