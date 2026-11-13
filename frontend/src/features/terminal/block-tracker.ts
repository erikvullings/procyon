import type { ShellMark } from './shell-integration';

/**
 * One command and its output, as the shell marked it. Marks are opaque handles (in the terminal,
 * xterm markers that follow their line as the buffer scrolls), so this module stays pure.
 */
export interface CommandBlock<Mark> {
  readonly id: number;
  readonly command: string;
  readonly promptMark: Mark;
  readonly outputMark: Mark;
  readonly endMark: Mark;
  readonly exitCode: number;
}

export interface BlockTrackerOptions<Mark> {
  /** Most finished blocks kept; older ones are dropped. */
  readonly maxBlocks: number;
  /** Called with each block dropped by `maxBlocks`, so the owner can release its marks. */
  readonly onEvict?: (block: CommandBlock<Mark>) => void;
}

export interface BlockTracker<Mark> {
  /** Applies one shell mark. Returns the block when this mark completes one. */
  apply(mark: ShellMark, handle: () => Mark, command: () => string): CommandBlock<Mark> | undefined;
  /** Finished blocks, oldest first. */
  readonly blocks: readonly CommandBlock<Mark>[];
}

interface OpenBlock<Mark> {
  readonly id: number;
  readonly promptMark: Mark;
  outputMark?: Mark;
  command: string;
}

/**
 * Turns the shell's marks into command blocks. A block is finished only when its output started
 * and then ended; a prompt with no command (an empty Enter) produces no block.
 */
export function createBlockTracker<Mark>(options: BlockTrackerOptions<Mark>): BlockTracker<Mark> {
  let nextId = 1;
  let open: OpenBlock<Mark> | undefined;
  const blocks: CommandBlock<Mark>[] = [];

  return {
    apply(mark, handle, command) {
      switch (mark.kind) {
        case 'prompt':
          open = { id: nextId, promptMark: handle(), command: '' };
          nextId += 1;
          return undefined;
        case 'output':
          if (open === undefined) return undefined;
          open.outputMark = handle();
          open.command = command();
          return undefined;
        case 'end': {
          const current = open;
          open = undefined;
          if (current === undefined || current.outputMark === undefined) return undefined;
          const block: CommandBlock<Mark> = {
            id: current.id,
            command: current.command,
            promptMark: current.promptMark,
            outputMark: current.outputMark,
            endMark: handle(),
            exitCode: mark.exitCode,
          };
          blocks.push(block);
          while (blocks.length > options.maxBlocks) {
            const evicted = blocks.shift();
            if (evicted !== undefined) options.onEvict?.(evicted);
          }
          return block;
        }
      }
    },
    get blocks() {
      return blocks;
    },
  };
}
