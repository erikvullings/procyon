const autoSuffix = /-auto\.\d+$/u;

/**
 * `semantic-v0.7.5` -> `semantic-v0.7.6`; anything without a trailing number gets `.1` appended.
 * A `-auto.<run>` suffix (a main-push qualification tag, see `autoQualificationTag`) is dropped
 * first, so the next manual tag continues the numbered series.
 */
export function nextSemanticTag(tag) {
  const base = tag.replace(autoSuffix, '');
  const match = /^(.*?)(\d+)$/u.exec(base);
  return match === null ? `${base}.1` : `${match[1]}${Number(match[2]) + 1}`;
}

/**
 * Unique, valid immutable tag for a qualification started by a push to main. The tag is baked into
 * the signed catalog URLs, so the later publication must reuse exactly this tag.
 */
export function autoQualificationTag(currentTag, runNumber) {
  if (!Number.isSafeInteger(runNumber) || runNumber <= 0) {
    throw new Error('workflow run number must be a positive integer');
  }
  const tag = `${nextSemanticTag(currentTag)}-auto.${runNumber}`;
  if (!/^semantic-v[0-9][0-9A-Za-z.-]*$/u.test(tag)) {
    throw new Error(`derived qualification tag ${tag} is not a valid semantic component tag`);
  }
  return tag;
}
