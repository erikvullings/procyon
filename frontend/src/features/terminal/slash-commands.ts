import type { LlmProfile, LlmRoleAssignment } from '../../models';
import type { MentionCompletion } from './terminal-assist';

/** The session commands typed after `#` (task 0343). */
export const SLASH_COMMANDS = [
  'new',
  'clear',
  'compact',
  'resume',
  'model',
  'auto',
  'help',
] as const;
export type SlashCommandName = (typeof SLASH_COMMANDS)[number];

/** What a `# /...` question asks for. */
export type SlashInput =
  | { readonly kind: 'command'; readonly name: SlashCommandName; readonly argument: string }
  | { readonly kind: 'skill'; readonly name: string; readonly rest: string };

function isCommand(name: string): name is SlashCommandName {
  return (SLASH_COMMANDS as readonly string[]).includes(name);
}

/**
 * A question that starts with `/name`: one of the commands, or a skill to use for the rest of the
 * question. `undefined` for an ordinary question (a path such as `/etc/hosts` is not a command).
 */
export function parseSlash(question: string): SlashInput | undefined {
  const match = /^\/([A-Za-z0-9][\w.:-]*)(?:\s+([\s\S]*))?$/.exec(question.trim());
  if (match === null) return undefined;
  const name = match[1] ?? '';
  const rest = (match[2] ?? '').trim();
  if (isCommand(name.toLowerCase()))
    return { kind: 'command', name: name.toLowerCase() as SlashCommandName, argument: rest };
  return { kind: 'skill', name, rest };
}

/** The `/name` being typed on a `#` line, when the line is only that. */
export function typedSlash(typed: string): string | undefined {
  return /^#\s*\/(\S*)$/.exec(typed)?.[1];
}

/**
 * Tab completion of `# /par` against command and skill names: a single match is completed with a
 * space after it, several extend to their common prefix. `undefined` when there is nothing to add.
 */
export function completeSlash(
  typed: string,
  names: readonly string[],
): MentionCompletion | undefined {
  const partial = typedSlash(typed);
  if (partial === undefined) return undefined;
  const lower = partial.toLowerCase();
  const matches = [...new Set(names)].filter((name) => name.toLowerCase().startsWith(lower));
  const first = matches[0];
  if (first === undefined) return undefined;
  if (matches.length === 1) return { erase: partial.length, insert: `${first} ` };
  let common = first;
  for (const name of matches) {
    let length = 0;
    while (length < common.length && common[length]?.toLowerCase() === name[length]?.toLowerCase())
      length += 1;
    common = common.slice(0, length);
  }
  return common.length > partial.length ? { erase: partial.length, insert: common } : undefined;
}

/** The names `/` completes, commands first. */
export function slashNames(skills: readonly string[]): string[] {
  return [...SLASH_COMMANDS, ...skills.filter((name) => !isCommand(name))];
}

/** A chat model `/model` can switch to. */
export interface ModelChoice {
  readonly profileId: string;
  readonly profileName: string;
  readonly model: string;
  readonly current: boolean;
}

/**
 * The models of every saved profile: each profile's own model, then the ones its endpoint
 * reported, without repeats; the chat role's binding is marked current.
 */
export function modelChoices(
  profiles: readonly LlmProfile[],
  chat: LlmRoleAssignment | null | undefined,
  discovered: ReadonlyMap<string, readonly string[]>,
): ModelChoice[] {
  const choices: ModelChoice[] = [];
  for (const profile of profiles) {
    const models = [profile.model, ...(discovered.get(profile.id) ?? [])].filter(
      (model, index, all) => model !== '' && all.indexOf(model) === index,
    );
    for (const model of models)
      choices.push({
        profileId: profile.id,
        profileName: profile.name,
        model,
        current: chat?.profileId === profile.id && chat.model === model,
      });
  }
  return choices;
}

/** One line per choice, numbered from 1, the current one marked. */
export function modelLines(choices: readonly ModelChoice[]): string[] {
  return choices.map(
    (choice, index) =>
      `${choice.current ? '*' : ' '} ${index + 1}. ${choice.model} (${choice.profileName})`,
  );
}

/** The choice `/model <argument>` names: its number, its model, or `profile/model`. */
export function pickModel(
  choices: readonly ModelChoice[],
  argument: string,
): ModelChoice | undefined {
  const wanted = argument.trim();
  if (wanted === '') return undefined;
  if (/^\d+$/.test(wanted)) return choices[Number(wanted) - 1];
  const lower = wanted.toLowerCase();
  return (
    choices.find((choice) => `${choice.profileName}/${choice.model}`.toLowerCase() === lower) ??
    choices.find((choice) => choice.model.toLowerCase() === lower)
  );
}
