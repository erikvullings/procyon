/**
 * Settings → Notes (task 0337): where notes live, and buttons to change the folder or to include
 * it in semantic search. Both open the dialogs that ask first; nothing changes from here directly.
 */
import m, { type FactoryComponent } from 'mithril';
import type { NotesClient } from '../../api/client/notes-client';
import { errorText, t } from '../../i18n';
import type { Location, NotesSetup } from '../../models';
import { pathInputFromLocation } from '../map/map-settings';

export interface NotesSettingsSectionAttrs {
  readonly client: Pick<NotesClient, 'getNotesSetup'>;
  /** Opens the notes setup dialog. */
  readonly onChangeFolder: () => void;
  /** Opens the semantic consent for the notes folder; absent when that is not possible. */
  readonly onIndexFolder?: ((root: Location) => void) | undefined;
}

export const NotesSettingsSection: FactoryComponent<NotesSettingsSectionAttrs> = () => {
  let setup: NotesSetup | undefined;
  let error: string | undefined;

  return {
    oninit: ({ attrs }) => {
      attrs.client.getNotesSetup().then(
        (loaded) => {
          setup = loaded;
          m.redraw();
        },
        (failure: unknown) => {
          error = errorText(failure);
          m.redraw();
        },
      );
    },
    view: ({ attrs }) => {
      const root = setup?.root ?? undefined;
      const status =
        error ??
        (setup === undefined
          ? ''
          : root === undefined
            ? t('notes', 'settingsNoRoot')
            : setup.rootExists
              ? pathInputFromLocation(root)
              : `${pathInputFromLocation(root)} — ${t('notes', 'settingsMissing')}`);
      return m('.row.fm-notes-settings', [
        m('h4.fm-settings-section-heading.col.s12', t('notes', 'settingsTitle')),
        m('p.col.s12.helper-text', t('notes', 'settingsIntro')),
        m('.col.s12.fm-notes-settings-root', [
          m('span.fm-notes-settings-label', t('notes', 'settingsRoot')),
          m('code.fm-notes-settings-path', { 'data-testid': 'notes-root' }, status),
        ]),
        m('.col.s12.fm-notes-settings-actions', [
          m(
            'button.btn-flat',
            { type: 'button', onclick: () => attrs.onChangeFolder() },
            t('notes', 'settingsChange'),
          ),
          root !== undefined && setup?.rootExists === true && attrs.onIndexFolder !== undefined
            ? m(
                'button.btn-flat',
                {
                  type: 'button',
                  title: t('notes', 'settingsIndexHelp'),
                  onclick: () => attrs.onIndexFolder?.(root),
                },
                t('notes', 'settingsIndex'),
              )
            : undefined,
        ]),
      ]);
    },
  };
};
