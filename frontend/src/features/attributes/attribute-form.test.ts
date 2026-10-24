import { describe, expect, it } from 'vitest';
import type { AttributeEntry, AttributeSupport } from '../../models';
import {
  applyOctal,
  buildChange,
  formProblems,
  initialForm,
  modeBitState,
  octalText,
  setBit,
  toggleBit,
} from './attribute-form';

const FULL: AttributeSupport = { mode: true, ownership: true, windowsAttributes: false };
const WINDOWS: AttributeSupport = { mode: false, ownership: false, windowsAttributes: true };

function entry(
  name: string,
  attributes: AttributeEntry['attributes'],
  kind: AttributeEntry['kind'] = 'file',
): AttributeEntry {
  return {
    location: { providerId: 'local', uri: `file:///tmp/${name}` },
    kind,
    ...(attributes === undefined ? {} : { attributes }),
  };
}

const USER_WRITE = 0o200;
const GROUP_READ = 0o040;
const OTHER_EXECUTE = 0o001;
const SETUID = 0o4000;

describe('attribute form', () => {
  it('shows a bit as on or off when every entry agrees and mixed otherwise', () => {
    const form = initialForm([
      entry('a', { mode: 0o644, uid: 1, gid: 2 }),
      entry('b', { mode: 0o755, uid: 1, gid: 2 }),
      entry('c', { mode: 0o600, uid: 1, gid: 2 }),
    ]);
    expect(modeBitState(form, USER_WRITE)).toBe('on');
    expect(modeBitState(form, GROUP_READ)).toBe('mixed');
    expect(modeBitState(form, OTHER_EXECUTE)).toBe('mixed');
    expect(modeBitState(form, 0o001 << 3)).toBe('mixed');
    expect(modeBitState(form, SETUID)).toBe('off');
    expect(octalText(form)).toBe('');
  });

  it('sends nothing until the user touches something', () => {
    const form = initialForm([entry('a', { mode: 0o644 })]);
    expect(buildChange(form, FULL)).toBeUndefined();
    expect(octalText(form)).toBe('0644');
  });

  it('toggling a mixed bit turns it on, then off, and only touched bits enter the mask', () => {
    let form = initialForm([entry('a', { mode: 0o644 }), entry('b', { mode: 0o755 })]);
    form = toggleBit(form, GROUP_READ); // agreed on
    expect(modeBitState(form, GROUP_READ)).toBe('off');
    form = toggleBit(form, OTHER_EXECUTE); // mixed -> on
    expect(modeBitState(form, OTHER_EXECUTE)).toBe('on');
    form = toggleBit(form, OTHER_EXECUTE); // on -> off
    expect(modeBitState(form, OTHER_EXECUTE)).toBe('off');
    expect(buildChange(form, FULL)).toEqual({
      mode: 0,
      modeMask: GROUP_READ | OTHER_EXECUTE,
    });
  });

  it('collects set bits into the mode and special bits into the same mask', () => {
    let form = initialForm([entry('a', { mode: 0o600 })]);
    form = setBit(form, GROUP_READ, true);
    form = setBit(form, SETUID, true);
    expect(buildChange(form, FULL)).toEqual({
      mode: GROUP_READ | SETUID,
      modeMask: GROUP_READ | SETUID,
    });
  });

  it('an octal value marks all twelve bits as chosen', () => {
    const form = applyOctal(
      initialForm([entry('a', { mode: 0o600 }), entry('b', { mode: 0o755 })]),
      '2750',
    );
    expect(octalText(form)).toBe('2750');
    expect(buildChange(form, FULL)).toEqual({ mode: 0o2750, modeMask: 0o7777 });
    expect(formProblems(applyOctal(form, '9'), FULL)).toContain('invalidOctal');
    expect(formProblems(applyOctal(form, '77777'), FULL)).toContain('invalidOctal');
    expect(formProblems(applyOctal(form, '755'), FULL)).toEqual([]);
  });

  it('owner and group are numeric ids and blank means unchanged', () => {
    let form = initialForm([entry('a', { mode: 0o644, uid: 501, gid: 20 })]);
    form = { ...form, uid: '0', gid: '' };
    expect(buildChange(form, FULL)).toEqual({ uid: 0 });
    expect(formProblems({ ...form, uid: 'root' }, FULL)).toContain('invalidOwner');
    expect(formProblems({ ...form, gid: '-1' }, FULL)).toContain('invalidGroup');
    expect(formProblems({ ...form, gid: '4294967295' }, FULL)).toContain('invalidGroup');
  });

  it('ignores fields the provider cannot edit', () => {
    let form = initialForm([entry('a', { mode: 0o644, uid: 1, gid: 1 })]);
    form = setBit(form, USER_WRITE, false);
    form = { ...form, uid: '5' };
    expect(buildChange(form, WINDOWS)).toBeUndefined();
    expect(buildChange(form, { ...FULL, ownership: false })).toEqual({
      mode: 0,
      modeMask: USER_WRITE,
    });
  });

  it('windows attributes are tri-state and only touched ones are sent', () => {
    let form = initialForm([
      entry('a', { readOnly: true, hidden: false, archive: false }),
      entry('b', { readOnly: false, hidden: false, archive: true }),
    ]);
    expect(form.readOnly).toEqual({ initial: 'mixed', value: 'mixed' });
    expect(form.hidden.value).toBe('off');
    form = {
      ...form,
      readOnly: { ...form.readOnly, value: 'on' },
      hidden: { initial: 'off', value: 'on' },
    };
    expect(buildChange(form, WINDOWS)).toEqual({ readOnly: true, hidden: true });
  });

  it('entries without readable attributes do not make bits mixed', () => {
    const form = initialForm([entry('a', { mode: 0o644 }), entry('link', null, 'symlink')]);
    expect(modeBitState(form, USER_WRITE)).toBe('on');
  });
});
