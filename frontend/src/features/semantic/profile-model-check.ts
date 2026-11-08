import type { SemanticClient } from '../../api/client/semantic-client';
import { t } from '../../i18n';
import type { LlmProfile } from '../../models';

/** What is known about a profile's model on its AI service. */
export type ProfileModelStatus = 'available' | 'missing' | 'unreachable' | 'unverified';

/** Time allowed for the model list before the profile is reported as unreachable. */
const CHECK_TIMEOUT_MS = 8000;

/**
 * Whether `model` is among the `listed` ids. Ollama reports `name:latest` for a model pulled
 * without a tag, so a configured name without a tag matches that too.
 */
export function modelIsListed(model: string, listed: readonly string[]): boolean {
  const wanted = model.trim();
  if (wanted === '') return false;
  return listed.some((candidate) => candidate === wanted || candidate === `${wanted}:latest`);
}

/**
 * Asks the profile's AI service which models it offers and compares with the configured one.
 * Services that cannot list models (Azure, custom endpoints) are `unverified`, never `missing`.
 */
export async function checkProfileModel(
  client: Pick<SemanticClient, 'discoverLlmProfileModels'>,
  profile: LlmProfile,
): Promise<ProfileModelStatus> {
  if (!profile.capabilities.includes('modelDiscovery') || profile.preset === 'azureOpenAi') {
    return 'unverified';
  }
  try {
    const listed = await client.discoverLlmProfileModels(
      profile.id,
      AbortSignal.timeout(CHECK_TIMEOUT_MS),
    );
    if (listed.length === 0) return 'unverified';
    return modelIsListed(profile.model, listed) ? 'available' : 'missing';
  } catch {
    return 'unreachable';
  }
}

/** The warning to show for a profile, if its model is missing or its service cannot be reached. */
export function profileModelNotice(
  profile: LlmProfile,
  status: ProfileModelStatus | undefined,
): string | undefined {
  if (status === 'missing') {
    return t('llmProfileCheck', 'modelMissing', { model: profile.model, name: profile.name });
  }
  if (status === 'unreachable') {
    return t('llmProfileCheck', 'unreachable', { name: profile.name });
  }
  return undefined;
}
