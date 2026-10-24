import type {
  AttributeChange,
  AttributeEntry,
  AttributeScope,
  AttributeSupport,
} from '../../models';

/** Opens "Change attributes…" for the selection (task 0258). */
export const CHANGE_ATTRIBUTES_ACTION_ID = 'client.changeAttributes';

/** Bits the editor can set: rwx for user, group and other plus setuid, setgid and sticky. */
export const MODE_BITS = {
  userRead: 0o400,
  userWrite: 0o200,
  userExecute: 0o100,
  groupRead: 0o040,
  groupWrite: 0o020,
  groupExecute: 0o010,
  otherRead: 0o004,
  otherWrite: 0o002,
  otherExecute: 0o001,
  setuid: 0o4000,
  setgid: 0o2000,
  sticky: 0o1000,
} as const;

export const ALL_MODE_BITS: readonly number[] = Object.values(MODE_BITS);
const MODE_MASK = 0o7777;
/** Largest owner/group id the editor accepts; `u32::MAX` is the "unchanged" sentinel in `chown`. */
const MAX_ID = 0xffff_fffe;

/** A checkbox state: every selected entry has it, none has it, or they differ. */
export type TriState = 'on' | 'off' | 'mixed';

/** What the entries had (`initial`) and what the user chose (`value`). */
export interface TriValue {
  readonly initial: TriState;
  readonly value: TriState;
}

export interface AttributeForm {
  readonly bits: Readonly<Record<number, TriValue>>;
  /** Raw octal text the user typed; `undefined` means the field mirrors the checkboxes. */
  readonly octalInput?: string | undefined;
  /** New numeric owner; blank leaves it unchanged. */
  readonly uid: string;
  /** New numeric group; blank leaves it unchanged. */
  readonly gid: string;
  /** Owner shared by all entries, shown as a hint. */
  readonly currentUid?: number | undefined;
  readonly currentGid?: number | undefined;
  readonly readOnly: TriValue;
  readonly hidden: TriValue;
  readonly archive: TriValue;
  readonly recursive: boolean;
  readonly scope: AttributeScope;
}

export type FormProblem = 'invalidOctal' | 'invalidOwner' | 'invalidGroup';

function aggregate(values: readonly (boolean | null | undefined)[]): TriState {
  const known = values.filter((value): value is boolean => typeof value === 'boolean');
  if (known.length === 0) return 'off';
  if (known.every((value) => value === known[0])) return known[0] === true ? 'on' : 'off';
  return 'mixed';
}

function untouched(state: TriState): TriValue {
  return { initial: state, value: state };
}

function shared(values: readonly (number | null | undefined)[]): number | undefined {
  const known = values.filter((value): value is number => typeof value === 'number');
  const first = known[0];
  return first !== undefined && known.every((value) => value === first) ? first : undefined;
}

/** Builds the form from the current state of the selected entries; links expose nothing. */
export function initialForm(entries: readonly AttributeEntry[]): AttributeForm {
  const readable = entries.flatMap((entry) =>
    entry.attributes === undefined || entry.attributes === null ? [] : [entry.attributes],
  );
  const bits: Record<number, TriValue> = {};
  for (const bit of ALL_MODE_BITS) {
    bits[bit] = untouched(
      aggregate(readable.map(({ mode }) => (typeof mode === 'number' ? (mode & bit) !== 0 : null))),
    );
  }
  return {
    bits,
    uid: '',
    gid: '',
    currentUid: shared(readable.map(({ uid }) => uid)),
    currentGid: shared(readable.map(({ gid }) => gid)),
    readOnly: untouched(aggregate(readable.map(({ readOnly }) => readOnly))),
    hidden: untouched(aggregate(readable.map(({ hidden }) => hidden))),
    archive: untouched(aggregate(readable.map(({ archive }) => archive))),
    recursive: false,
    scope: 'both',
  };
}

export function modeBitState(form: AttributeForm, bit: number): TriState {
  return form.bits[bit]?.value ?? 'off';
}

function withBit(form: AttributeForm, bit: number, value: TriValue): AttributeForm {
  return { ...form, bits: { ...form.bits, [bit]: value }, octalInput: undefined };
}

/** Cycles on -> off -> on; a mixed bit becomes on first. */
export function toggleBit(form: AttributeForm, bit: number): AttributeForm {
  const current = form.bits[bit] ?? untouched('off');
  return withBit(form, bit, { ...current, value: current.value === 'on' ? 'off' : 'on' });
}

export function setBit(form: AttributeForm, bit: number, on: boolean): AttributeForm {
  const current = form.bits[bit] ?? untouched('off');
  return withBit(form, bit, { ...current, value: on ? 'on' : 'off' });
}

/** Cycles a Windows attribute the same way as a mode bit. */
export function toggleTri(value: TriValue): TriValue {
  return { ...value, value: value.value === 'on' ? 'off' : 'on' };
}

const OCTAL = /^[0-7]{1,4}$/;

/**
 * Applies typed octal text. A valid value overrides every bit (the entries' own bits no longer
 * matter), so all twelve count as chosen.
 */
export function applyOctal(form: AttributeForm, text: string): AttributeForm {
  const trimmed = text.trim();
  if (!OCTAL.test(trimmed)) return { ...form, octalInput: text };
  const mode = Number.parseInt(trimmed, 8);
  const bits: Record<number, TriValue> = {};
  for (const bit of ALL_MODE_BITS) {
    bits[bit] = { initial: 'mixed', value: (mode & bit) !== 0 ? 'on' : 'off' };
  }
  return { ...form, bits, octalInput: text };
}

/** Four-digit octal mode, or `''` while any bit is mixed. */
export function octalText(form: AttributeForm): string {
  let mode = 0;
  for (const bit of ALL_MODE_BITS) {
    const state = form.bits[bit]?.value ?? 'off';
    if (state === 'mixed') return '';
    if (state === 'on') mode |= bit;
  }
  return mode.toString(8).padStart(4, '0');
}

function parseId(text: string): number | undefined {
  const trimmed = text.trim();
  if (!/^\d{1,10}$/.test(trimmed)) return undefined;
  const id = Number(trimmed);
  return id <= MAX_ID ? id : undefined;
}

export function formProblems(
  form: AttributeForm,
  support: AttributeSupport,
): readonly FormProblem[] {
  const problems: FormProblem[] = [];
  if (support.mode && form.octalInput !== undefined && !OCTAL.test(form.octalInput.trim())) {
    problems.push('invalidOctal');
  }
  if (support.ownership) {
    if (form.uid.trim() !== '' && parseId(form.uid) === undefined) problems.push('invalidOwner');
    if (form.gid.trim() !== '' && parseId(form.gid) === undefined) problems.push('invalidGroup');
  }
  return problems;
}

function chosen(value: TriValue): boolean | undefined {
  if (value.value === 'mixed') return undefined;
  if (value.initial === value.value) return undefined;
  return value.value === 'on';
}

/** The change to send: only what the user touched and the selection's providers support. */
export function buildChange(
  form: AttributeForm,
  support: AttributeSupport,
): AttributeChange | undefined {
  const change: AttributeChange = {};
  if (support.mode) {
    let mode = 0;
    let mask = 0;
    for (const bit of ALL_MODE_BITS) {
      const entry = form.bits[bit];
      if (entry === undefined || entry.value === 'mixed') continue;
      if (entry.value === entry.initial) continue;
      mask |= bit;
      if (entry.value === 'on') mode |= bit;
    }
    if (mask !== 0) {
      change.mode = mode & MODE_MASK;
      change.modeMask = mask;
    }
  }
  if (support.ownership) {
    const uid = parseId(form.uid);
    const gid = parseId(form.gid);
    if (uid !== undefined) change.uid = uid;
    if (gid !== undefined) change.gid = gid;
  }
  if (support.windowsAttributes) {
    const readOnly = chosen(form.readOnly);
    const hidden = chosen(form.hidden);
    const archive = chosen(form.archive);
    if (readOnly !== undefined) change.readOnly = readOnly;
    if (hidden !== undefined) change.hidden = hidden;
    if (archive !== undefined) change.archive = archive;
  }
  return Object.keys(change).length === 0 ? undefined : change;
}
