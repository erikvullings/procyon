/** Disable unsolicited edits in commands, paths, file names, and search queries. */
export const technicalTextInputAttrs = {
  autocomplete: 'off',
  autocapitalize: 'off',
  autocorrect: 'off',
  spellcheck: 'false',
} as const;
