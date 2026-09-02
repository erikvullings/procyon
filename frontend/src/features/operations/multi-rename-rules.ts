import type { MultiRenameCaseTransform, MultiRenameRules, MultiRenameSequence } from '../../models';
import { validateEntryName } from './create-directory-dialog';

/** How the whole proposed name is cased after every other rule has been applied. */
export type CaseTransform = MultiRenameCaseTransform;
/** A counter available to the `[C]` mask token, in selection order. */
export type SequenceRule = MultiRenameSequence;
export type { MultiRenameRules };

export const EMPTY_MULTI_RENAME_RULES: MultiRenameRules = {
  search: '',
  replace: '',
  useRegex: false,
  nameMask: '[N]',
  extensionMask: '[E]',
  sequence: { start: 1, step: 1, padding: 1 },
  caseTransform: 'unchanged',
};

/** A minimal view of a selected entry, sufficient to compute a rename proposal. */
export interface RenameTarget {
  readonly id: string;
  readonly name: string;
}

/** Why a proposed name collides with another one. */
export type RenameCollisionKind = 'plan' | 'existing';

/** One row of the multi-rename preview table. */
export interface RenameProposal {
  readonly id: string;
  readonly oldName: string;
  readonly newName: string;
  readonly changed: boolean;
  readonly invalidNameReason?: string;
  readonly collision?: RenameCollisionKind;
}

/** Validates a search pattern before it is used, so a bad regex never throws mid-preview. */
export function validateSearchPattern(pattern: string, useRegex: boolean): string | undefined {
  if (!useRegex || pattern.length === 0) return undefined;
  try {
    // eslint-disable-next-line no-new -- validation only, the instance itself is unused
    new RegExp(pattern);
    return undefined;
  } catch {
    return 'That search pattern is not a valid regular expression.';
  }
}

function splitExtension(name: string): { stem: string; extension: string } {
  const lastDot = name.lastIndexOf('.');
  // No extension, or a leading dot (dotfile) with nothing before it: keep the whole name as the
  // stem so a leading dot is never mistaken for an empty-stem extension.
  if (lastDot <= 0) return { stem: name, extension: '' };
  return { stem: name.slice(0, lastDot), extension: name.slice(lastDot + 1) };
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, '\\$&');
}

function applySearchReplace(stem: string, rules: MultiRenameRules): string {
  if (rules.search.length === 0) return stem;
  if (rules.useRegex) {
    if (validateSearchPattern(rules.search, true) !== undefined) return stem;
    return stem.replace(new RegExp(rules.search, 'gu'), rules.replace);
  }
  return stem.replace(new RegExp(escapeRegExp(rules.search), 'gu'), rules.replace);
}

function formatSequence(index: number, sequence: SequenceRule): string {
  const value = sequence.start + index * sequence.step;
  return String(Math.abs(value)).padStart(sequence.padding, '0');
}

function formatDate(now: Date): string {
  const year = String(now.getFullYear()).padStart(4, '0');
  const month = String(now.getMonth() + 1).padStart(2, '0');
  const day = String(now.getDate()).padStart(2, '0');
  return `${year}${month}${day}`;
}

function formatTime(now: Date): string {
  const hours = String(now.getHours()).padStart(2, '0');
  const minutes = String(now.getMinutes()).padStart(2, '0');
  const seconds = String(now.getSeconds()).padStart(2, '0');
  return `${hours}${minutes}${seconds}`;
}

/** A 1-based, inclusive `start-end` slice of `value`; out-of-range bounds yield an empty string. */
function sliceRange(value: string, start: number, end: number): string {
  if (start < 1 || end < start) return '';
  return value.slice(start - 1, end);
}

interface MaskContext {
  readonly stem: string;
  readonly extension: string;
  readonly index: number;
  readonly sequence: SequenceRule;
  readonly now: Date;
}

// Total Commander-style mask tokens: [N]/[N#-#] the (search & replace'd) stem, [E]/[E#-#] the
// extension, [C] the counter, [YMD] today's date, [hms] the current time. Both mask fields accept
// every token, so e.g. a counter can be used in either the name or the extension mask.
const MASK_TOKEN = /\[N(?:(\d+)-(\d+))?\]|\[E(?:(\d+)-(\d+))?\]|\[C\]|\[YMD\]|\[hms\]/gu;

function applyMask(mask: string, ctx: MaskContext): string {
  return mask.replace(
    MASK_TOKEN,
    (match, nStart?: string, nEnd?: string, eStart?: string, eEnd?: string) => {
      if (match.startsWith('[N')) {
        return nStart !== undefined && nEnd !== undefined
          ? sliceRange(ctx.stem, Number(nStart), Number(nEnd))
          : ctx.stem;
      }
      if (match.startsWith('[E')) {
        return eStart !== undefined && eEnd !== undefined
          ? sliceRange(ctx.extension, Number(eStart), Number(eEnd))
          : ctx.extension;
      }
      if (match === '[C]') return formatSequence(ctx.index, ctx.sequence);
      if (match === '[YMD]') return formatDate(ctx.now);
      return formatTime(ctx.now); // [hms]
    },
  );
}

function applyCaseTransform(
  namePart: string,
  extensionPart: string,
  transform: CaseTransform,
): { namePart: string; extensionPart: string } {
  switch (transform) {
    case 'upper':
      return { namePart: namePart.toUpperCase(), extensionPart: extensionPart.toUpperCase() };
    case 'lower':
      return { namePart: namePart.toLowerCase(), extensionPart: extensionPart.toLowerCase() };
    case 'title':
      // Title-casing only makes sense on the words in the name itself; the extension is left
      // as-is (an all-caps ".JPG" wouldn't become ".Jpg").
      return {
        namePart: namePart.replace(/\b\w/gu, (letter) => letter.toUpperCase()),
        extensionPart,
      };
    case 'unchanged':
      return { namePart, extensionPart };
  }
}

/** Composes every rule into a single proposed name for one entry. */
export function proposeName(
  entry: RenameTarget,
  index: number,
  rules: MultiRenameRules,
  now: Date = new Date(),
): string {
  const { stem, extension } = splitExtension(entry.name);
  const ctx: MaskContext = {
    stem: applySearchReplace(stem, rules),
    extension,
    index,
    sequence: rules.sequence,
    now,
  };
  const { namePart, extensionPart } = applyCaseTransform(
    applyMask(rules.nameMask, ctx),
    applyMask(rules.extensionMask, ctx),
    rules.caseTransform,
  );
  return extensionPart.length > 0 ? `${namePart}.${extensionPart}` : namePart;
}

function foldForCollision(name: string): string {
  return name.toLocaleLowerCase();
}

/**
 * Computes a rename proposal for every entry, including collision and invalid-name detection.
 *
 * `existingSiblingNames` must exclude the entries being renamed themselves, so that a case-only
 * self-rename is never mistaken for a collision with its own original name.
 */
export function proposeRenames(
  entries: readonly RenameTarget[],
  rules: MultiRenameRules,
  existingSiblingNames: ReadonlySet<string>,
  now: Date = new Date(),
): RenameProposal[] {
  const foldedExisting = new Set(Array.from(existingSiblingNames, foldForCollision));
  const newNames = entries.map((entry, index) => proposeName(entry, index, rules, now));
  const foldedCounts = new Map<string, number>();
  for (const name of newNames) {
    const folded = foldForCollision(name);
    foldedCounts.set(folded, (foldedCounts.get(folded) ?? 0) + 1);
  }

  return entries.map((entry, index) => {
    const newName = newNames[index] ?? entry.name;
    const changed = newName !== entry.name;
    const folded = foldForCollision(newName);
    const invalidNameReason = validateEntryName(newName);
    const collision: RenameCollisionKind | undefined =
      (foldedCounts.get(folded) ?? 0) > 1
        ? 'plan'
        : foldedExisting.has(folded)
          ? 'existing'
          : undefined;
    return {
      id: entry.id,
      oldName: entry.name,
      newName,
      changed,
      ...(invalidNameReason === undefined ? {} : { invalidNameReason }),
      ...(collision === undefined ? {} : { collision }),
    };
  });
}

/** Whether the current plan is safe to apply: at least one real change, no blockers. */
export function canApplyRenamePlan(proposals: readonly RenameProposal[]): boolean {
  const changed = proposals.filter((proposal) => proposal.changed);
  if (changed.length === 0) return false;
  return changed.every(
    (proposal) => proposal.invalidNameReason === undefined && proposal.collision === undefined,
  );
}
