export type ConceptCandidateStatus = 'pending' | 'accepted' | 'rejected';

export interface SkosConcept {
  readonly uri: string;
  readonly prefLabels: Readonly<Record<string, string>>;
  readonly altLabels: Readonly<Record<string, readonly string[]>>;
  readonly definitions: Readonly<Record<string, string>>;
  readonly scopeNotes: Readonly<Record<string, string>>;
  readonly broader: readonly string[];
  readonly narrower: readonly string[];
  readonly related: readonly string[];
  readonly extensions: Readonly<Record<string, unknown>>;
}

export interface ConceptCandidate {
  readonly id: string;
  readonly label: string;
  readonly synonyms: readonly string[];
  readonly supportingChunkIds: readonly string[];
  readonly confidence: number;
  readonly corpusFrequency: number;
  readonly status: ConceptCandidateStatus;
}

export interface SemanticVocabulary {
  readonly id: string;
  readonly name: string;
  readonly concepts: readonly SkosConcept[];
  readonly workspaceIds: readonly string[];
  readonly rootIds: readonly string[];
  readonly reviewQueue: readonly ConceptCandidate[];
  readonly revision: number;
}

export interface AttachSemanticVocabularyRequest {
  readonly vocabularyId: string;
  readonly workspaceId?: string;
  readonly rootId?: string;
}

export interface ReviewConceptCandidateRequest {
  readonly vocabularyId: string;
  readonly candidateId: string;
  readonly action: 'accept' | 'edit' | 'reject';
  readonly conceptUri?: string;
  readonly preferredLabel?: string;
}

export interface DeleteSemanticVocabularyImpact {
  readonly vocabularyId: string;
  readonly affectedWorkspaceIds: readonly string[];
  readonly affectedRootIds: readonly string[];
  readonly requiresConfirmation: boolean;
  readonly deleted: boolean;
}
