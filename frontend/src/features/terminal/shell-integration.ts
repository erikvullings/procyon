/**
 * Shell marks for command blocks (task 0333). The desktop shell is started with wrappers that
 * print OSC 133 sequences: `A` at each prompt, `C` when a command's output starts, and
 * `D;<code>` when it ends with its exit status. Those wrappers live in the Rust host.
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
