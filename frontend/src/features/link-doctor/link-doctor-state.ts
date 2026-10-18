import type { LinkProblem, LinkReplacementCandidate } from '../../models';

/** Replacement candidates for one problem, keyed by `problemKey`. */
export interface CandidateState {
  readonly loading: boolean;
  readonly candidates: readonly LinkReplacementCandidate[];
  readonly truncated: boolean;
}

/** Complete state of the broken-link review panel (task 0266). */
export interface LinkDoctorState {
  readonly scanId: string | undefined;
  readonly problems: readonly LinkProblem[];
  readonly entriesScanned: number;
  readonly linksChecked: number;
  readonly isComplete: boolean;
  readonly isCancelled: boolean;
  readonly candidates: Readonly<Record<string, CandidateState>>;
  /** Key of the problem currently being repaired. */
  readonly repairing: string | undefined;
  readonly error: string | undefined;
}

export const INITIAL_LINK_DOCTOR_STATE: LinkDoctorState = {
  scanId: undefined,
  problems: [],
  entriesScanned: 0,
  linksChecked: 0,
  isComplete: false,
  isCancelled: false,
  candidates: {},
  repairing: undefined,
  error: undefined,
};

/** Stable identity of a problem within one scan. */
export function problemKey(problem: LinkProblem): string {
  return `${problem.kind}|${problem.source.uri}|${problem.line ?? ''}|${problem.target}`;
}

/** The file name the missing target had, used to search for a replacement. */
export function missingTargetName(problem: LinkProblem): string {
  const text = (problem.resolved?.uri ?? problem.target).replace(/[\\/]+$/, '');
  const last = text.split(/[\\/]/).pop() ?? text;
  try {
    return decodeURIComponent(last);
  } catch {
    return last;
  }
}

/** The last segment of the problem's source, decoded. */
export function sourceName(problem: LinkProblem): string {
  const last = problem.source.uri.split('/').pop() ?? problem.source.uri;
  try {
    return decodeURIComponent(last);
  } catch {
    return last;
  }
}

export function withoutProblem(state: LinkDoctorState, key: string): LinkDoctorState {
  const { [key]: _removed, ...candidates } = state.candidates;
  return {
    ...state,
    problems: state.problems.filter((problem) => problemKey(problem) !== key),
    candidates,
  };
}
