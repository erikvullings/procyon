import type { FindLinkReplacementsRequestDto } from '../api/generated/models/findLinkReplacementsRequestDto';
import type { FindLinkReplacementsResponseDto } from '../api/generated/models/findLinkReplacementsResponseDto';
import type { LinkDoctorPageDto } from '../api/generated/models/linkDoctorPageDto';
import type { LinkProblemDto } from '../api/generated/models/linkProblemDto';
import type { LinkReplacementCandidateDto } from '../api/generated/models/linkReplacementCandidateDto';
import type { Location } from './location';

/** A broken link found by the doctor (task 0266). */
export type LinkProblem = LinkProblemDto;
export type LinkDoctorPage = LinkDoctorPageDto;
export type LinkReplacementCandidate = LinkReplacementCandidateDto;
export type FindLinkReplacementsResult = FindLinkReplacementsResponseDto;
export type FindLinkReplacementsRequest = Omit<FindLinkReplacementsRequestDto, 'roots'> & {
  readonly roots: readonly Location[];
};

export interface StartLinkDoctorScanRequest {
  readonly roots: readonly Location[];
  readonly showHidden?: boolean;
}

export interface StartLinkDoctorScanResult {
  readonly scanId: string;
}
