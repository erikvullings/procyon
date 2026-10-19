import { toast } from 'mithril-materialized';
import { t } from '../../i18n';

/** Copies `value` to the clipboard and reports success/failure via toast - the same feedback
 * mechanism used elsewhere in the app (e.g. diagnostics' "Copy for Bug Report"), so the F3 viewer
 * doesn't invent a second, silent copy affordance. */
export async function copyWithToast(
  action: () => Promise<void>,
  successMessage: string,
): Promise<void> {
  try {
    await action();
    toast({ html: successMessage });
  } catch {
    toast({ html: t('viewer', 'copyFailed') });
  }
}
