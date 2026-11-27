import m, { type FactoryComponent } from 'mithril';
import type { FileManagerClient } from '../../api/client/file-manager-client';
import type { Location } from '../../models/location';
import type {
  ApprovedMove,
  FilingRule,
  OrganizerPreview,
  OrganizerRequest,
  WatchedFolder,
} from '../../models/watched-folders';

interface Attrs {
  client: FileManagerClient;
  activeLocation?: Location | undefined;
}

function name(location: Location): string {
  const segment = location.uri.replace(/\/$/, '').split('/').at(-1) ?? location.uri;
  try {
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
}

function extensionOf(location: Location): string | undefined {
  const filename = name(location);
  const dot = filename.lastIndexOf('.');
  return dot > 0 && dot < filename.length - 1 ? filename.slice(dot + 1).toLowerCase() : undefined;
}

function suggestRules(
  assignments: readonly ApprovedMove[],
  excluded: readonly Location[],
): FilingRule[] {
  const ambiguous = new Set(excluded.map(extensionOf));
  const byExtension = new Map<string, Set<string>>();
  for (const item of assignments) {
    const extension = extensionOf(item.source);
    if (!extension || ambiguous.has(extension)) continue;
    const destinations = byExtension.get(extension) ?? new Set<string>();
    destinations.add(item.folder);
    byExtension.set(extension, destinations);
  }
  return [...byExtension].flatMap(([extension, destinations]) => {
    const folder = [...destinations][0];
    return destinations.size === 1 && folder !== undefined ? [{ extension, folder }] : [];
  });
}

export const WatchedFoldersSettings: FactoryComponent<Attrs> = () => {
  let folders: WatchedFolder[] = [];
  let root: Location | undefined;
  let paused = false;
  let hours = 48;
  let rules: FilingRule[] = [];
  let useSuggestedRules = false;
  let preview: OrganizerPreview | undefined;
  let assignments: ApprovedMove[] = [];
  let omitted = new Set<string>();
  let job: string | undefined;
  let busy = false;
  let error: string | undefined;
  let alive = true;
  let timer: number | undefined;

  function open(location: Location): void {
    root = location;
    const saved = folders.find((folder) => folder.root.uri === location.uri);
    hours = saved?.delayHours ?? 48;
    paused = saved?.paused ?? false;
    rules = saved?.rules.map((rule) => ({ ...rule })) ?? [];
    preview = undefined;
    assignments = [];
    omitted = new Set();
    useSuggestedRules = false;
    error = undefined;
  }

  async function run(client: FileManagerClient, request: OrganizerRequest): Promise<void> {
    busy = true;
    error = undefined;
    try {
      const response = await client.manageWatchedFolders(request);
      folders = response.folders;
      if (response.jobId) {
        if (!alive) {
          await client.manageWatchedFolders({ action: 'cancel', jobId: response.jobId });
        } else {
          job = response.jobId;
        }
      }
      if (request.action === 'save') {
        preview = undefined;
        assignments = [];
        omitted.clear();
        useSuggestedRules = false;
      }
      if (request.action === 'remove') root = undefined;
    } catch (failure) {
      error = failure instanceof Error ? failure.message : String(failure);
    } finally {
      busy = false;
      if (alive) m.redraw();
    }
  }

  function input(
    label: string,
    value: string,
    change: (value: string) => void,
    attrs: Record<string, unknown> = {},
  ) {
    return m('label', [
      label,
      m('input', {
        ...attrs,
        value,
        oninput: (event: InputEvent) => change((event.target as HTMLInputElement).value),
      }),
    ]);
  }

  function button(label: string, onclick: () => void, disabled = false) {
    return m('button.btn-flat', { type: 'button', onclick, disabled }, label);
  }

  return {
    oninit({ attrs }) {
      void run(attrs.client, { action: 'list' });
    },
    oncreate({ attrs }) {
      const client = attrs.client;
      timer = window.setInterval(async () => {
        if (busy) return;
        if (!job) {
          if (folders.length > 0) {
            try {
              const response = await client.manageWatchedFolders({ action: 'list' });
              if (alive && !busy) {
                folders = response.folders;
                m.redraw();
              }
            } catch {
              // Keep the last known state; an explicit command surfaces failures.
            }
          }
          return;
        }
        busy = true;
        const jobId = job;
        try {
          const response = await client.manageWatchedFolders({ action: 'collect', jobId });
          if (!alive) return;
          if (response.preview) {
            preview = response.preview;
            assignments = preview.moves.map((item) => ({ ...item }));
            omitted = new Set();
            job = undefined;
          }
        } catch (failure) {
          error = failure instanceof Error ? failure.message : String(failure);
          job = undefined;
        } finally {
          busy = false;
          if (alive) m.redraw();
        }
      }, 1500);
    },
    onremove({ attrs }) {
      alive = false;
      window.clearInterval(timer);
      if (job)
        void attrs.client.manageWatchedFolders({ action: 'cancel', jobId: job }).catch(() => {});
    },
    view({ attrs }) {
      const { client, activeLocation } = attrs;
      const activeRoot = root;
      const watched = folders.find((folder) => folder.root.uri === activeRoot?.uri);
      const selected = assignments.filter((item) => !omitted.has(item.source.uri));
      const suggested = suggestRules(selected, [
        ...(preview?.unmatched ?? []),
        ...(preview?.unprocessed ?? []),
        ...assignments.filter((item) => omitted.has(item.source.uri)).map((item) => item.source),
      ]);
      return m('section', [
        m('h4.fm-settings-section-heading', 'Watched folders'),
        error ? m('p[role=alert]', error) : undefined,
        folders.map((folder) =>
          button(name(folder.root), () => open(folder.root), busy || Boolean(job)),
        ),
        activeLocation
          ? button('Use active folder', () => open(activeLocation), busy || Boolean(job))
          : undefined,
        activeRoot
          ? [
              m('p', activeRoot.uri),
              input(
                'Keep files in place for (hours)',
                String(hours),
                (value) => {
                  hours = Number(value);
                },
                { type: 'number', min: 0, max: 8760, step: 1 },
              ),
              m('label', [
                m('input[type=checkbox]', {
                  checked: paused,
                  onchange: () => {
                    paused = !paused;
                  },
                }),
                m('span', 'Pause filing'),
              ]),
              m('h5', 'Rules for new files'),
              rules.map((rule, index) =>
                m('.row', [
                  input('Extension', rule.extension ?? '', (value) => {
                    rule.extension = value || null;
                  }),
                  input('MIME type', rule.contentType ?? '', (value) => {
                    rule.contentType = value || null;
                  }),
                  input(
                    'Minimum bytes',
                    String(rule.minimumBytes ?? ''),
                    (value) => {
                      rule.minimumBytes = value ? Number(value) : null;
                    },
                    { type: 'number', min: 0 },
                  ),
                  input('Destination folder', rule.folder, (value) => {
                    rule.folder = value;
                  }),
                  button(
                    'Move rule up',
                    () => {
                      const previous = rules[index - 1];
                      if (previous) rules.splice(index - 1, 2, rule, previous);
                    },
                    index === 0,
                  ),
                  button('Remove rule', () => {
                    rules.splice(index, 1);
                  }),
                ]),
              ),
              button('Add rule', () => {
                rules.push({ folder: '' });
              }),
              m('h5', 'Initial cleanup'),
              m(
                'p',
                'The configured chat model receives file names and types to propose folders. Review the proposed moves before saving.',
              ),
              button(
                'Suggest a folder structure',
                () => {
                  void run(client, { action: 'preview', root: activeRoot });
                },
                busy || Boolean(job),
              ),
              job
                ? [
                    m('p[role=status]', 'Reviewing the folder…'),
                    button(
                      'Cancel scan',
                      () => {
                        const jobId = job;
                        if (jobId) {
                          job = undefined;
                          void run(client, { action: 'cancel', jobId });
                        }
                      },
                      busy,
                    ),
                  ]
                : undefined,
              preview
                ? [
                    assignments.length === 0 ? m('p', 'No moves suggested.') : undefined,
                    assignments.map((item) =>
                      m('.row', [
                        m('label', [
                          m('input[type=checkbox]', {
                            checked: !omitted.has(item.source.uri),
                            onchange: () => {
                              if (!omitted.delete(item.source.uri)) omitted.add(item.source.uri);
                            },
                          }),
                          m('span', name(item.source)),
                        ]),
                        input('Destination folder', item.folder, (value) => {
                          item.folder = value;
                        }),
                      ]),
                    ),
                    suggested.length > 0
                      ? [
                          m('label', [
                            m('input[type=checkbox]', {
                              checked: useSuggestedRules,
                              onchange: () => {
                                useSuggestedRules = !useSuggestedRules;
                              },
                            }),
                            m('span', 'Use these rules for future arrivals'),
                          ]),
                          m(
                            'ul',
                            suggested.map((rule) => m('li', `.${rule.extension} → ${rule.folder}`)),
                          ),
                        ]
                      : undefined,
                    preview.unmatched.length
                      ? m('details', [
                          m('summary', `No match (${preview.unmatched.length})`),
                          m(
                            'ul',
                            preview.unmatched.map((item) => m('li', name(item))),
                          ),
                        ])
                      : undefined,
                    preview.unprocessed.length
                      ? m('details', [
                          m(
                            'summary',
                            `Outside the model’s input budget (${preview.unprocessed.length})`,
                          ),
                          m(
                            'ul',
                            preview.unprocessed.map((item) => m('li', name(item))),
                          ),
                        ])
                      : undefined,
                  ]
                : undefined,
              button(
                selected.length ? 'Approve moves and save' : 'Save watched folder',
                () => {
                  void run(client, {
                    action: 'save',
                    root: activeRoot,
                    delayHours: hours,
                    paused,
                    rules: useSuggestedRules ? [...rules, ...suggested] : rules,
                    approved: selected,
                  });
                },
                busy || Boolean(job) || !Number.isInteger(hours) || hours < 0 || hours > 8760,
              ),
              watched
                ? button(
                    'Stop watching',
                    () => {
                      void run(client, { action: 'remove', root: activeRoot });
                    },
                    busy || Boolean(job),
                  )
                : undefined,
              watched?.error ? m('p[role=alert]', watched.error) : undefined,
              watched?.pending.length
                ? [
                    m('h5', 'Pending filing'),
                    m(
                      'ul',
                      watched.pending.map((item) =>
                        m('li', [
                          `${name(item.assignment.source)} → ${item.assignment.folder} · `,
                          item.error ?? new Date(item.dueAt).toLocaleString(),
                        ]),
                      ),
                    ),
                  ]
                : undefined,
            ]
          : undefined,
      ]);
    },
  };
};
