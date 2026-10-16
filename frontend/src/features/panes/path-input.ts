const WRAPPERS = ['```', '`', '"', "'"] as const;

function stripOneWrapper(value: string): string | undefined {
  for (const wrapper of WRAPPERS) {
    if (value.length < wrapper.length * 2 || !value.startsWith(wrapper)) continue;
    // Text copied from prose often carries sentence punctuation after the closing quote.
    const body = value.replace(/[.,;:]$/u, '');
    if (body.length >= wrapper.length * 2 && body.endsWith(wrapper)) {
      return body.slice(wrapper.length, body.length - wrapper.length).trim();
    }
  }
  return undefined;
}

/**
 * Cleans a path typed or pasted into the location editor: trims whitespace, removes surrounding
 * Markdown code fences/backticks and single or double quotes (as added by chat or LLM output),
 * and unescapes a Markdown-escaped leading `\~` so it still means the home directory.
 */
export function normalizeTypedPath(input: string): string {
  let value = input.trim();
  for (let unwrapped = stripOneWrapper(value); unwrapped !== undefined; ) {
    value = unwrapped;
    unwrapped = stripOneWrapper(value);
  }
  if (value === '\\~' || value.startsWith('\\~/')) {
    value = value.slice(1);
  }
  return value;
}
