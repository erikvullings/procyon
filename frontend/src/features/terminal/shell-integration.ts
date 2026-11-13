/**
 * Shell integration for command blocks (task 0333, phase 2). The snippets make the shell mark
 * each prompt (`A`), each command's start of output (`C`) and each command's end with its exit
 * status (`D;<code>`) using OSC 133 sequences. The person adds one snippet to their shell's
 * startup file; without it the terminal simply shows no blocks.
 */

/** One shell mark, as the terminal parser receives it. */
export type ShellMark =
  | { readonly kind: 'prompt' }
  | { readonly kind: 'output' }
  | { readonly kind: 'end'; readonly exitCode: number };

/** Reads the payload of an OSC 133 sequence. Unknown payloads are ignored. */
export function parseShellMark(payload: string): ShellMark | undefined {
  if (payload === 'A') return { kind: 'prompt' };
  if (payload === 'C') return { kind: 'output' };
  if (payload === 'D' || payload.startsWith('D;')) {
    const code = Number.parseInt(payload.slice(2), 10);
    return { kind: 'end', exitCode: Number.isFinite(code) ? code : 0 };
  }
  return undefined;
}

export type ShellName = 'zsh' | 'bash' | 'fish';

/** The snippet for each supported shell. Paste it into the shell's startup file. */
export const SHELL_INTEGRATION: Readonly<Record<ShellName, string>> = {
  zsh: String.raw`# Procyon shell integration (command blocks)
__procyon_precmd() {
  local exit_code=$?
  printf '\e]133;D;%d\a' "$exit_code"
  printf '\e]133;A\a'
}
__procyon_preexec() {
  printf '\e]133;C\a'
}
autoload -Uz add-zsh-hook
add-zsh-hook precmd __procyon_precmd
add-zsh-hook preexec __procyon_preexec`,
  bash: [
    '# Procyon shell integration (command blocks)',
    '__procyon_begin() { __procyon_status=$?; }',
    '__procyon_end() {',
    '  printf \'\\e]133;D;%d\\a\' "$__procyon_status"',
    "  printf '\\e]133;A\\a'",
    '  __procyon_at_prompt=1',
    '}',
    '__procyon_at_prompt=1',
    '__procyon_preexec() {',
    '  if [ "$__procyon_at_prompt" = 1 ]; then',
    '    __procyon_at_prompt=0',
    "    printf '\\e]133;C\\a'",
    '  fi',
    '}',
    "trap '__procyon_preexec' DEBUG",
    // biome-ignore lint/suspicious/noTemplateCurlyInString: shell parameter expansion, not a JS template
    'PROMPT_COMMAND="__procyon_begin${PROMPT_COMMAND:+; $PROMPT_COMMAND}; __procyon_end"',
  ].join('\n'),
  fish: String.raw`# Procyon shell integration (command blocks)
functions -q __procyon_original_prompt; or functions -c fish_prompt __procyon_original_prompt
function fish_prompt
    printf '\e]133;A\a'
    __procyon_original_prompt
end
function __procyon_preexec --on-event fish_preexec
    printf '\e]133;C\a'
end
function __procyon_postexec --on-event fish_postexec
    printf '\e]133;D;%d\a' $status
end`,
};
