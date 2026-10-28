use tempfile::tempdir;

use super::*;
use crate::embedding::EmbeddingCacheKey;
use crate::semantic_storage::{
    DistanceMetric, LibraryIndexManifest, Occurrence, StagedGeneration, StagedRecord,
};

const DIMENSIONS: usize = 8;
const TENANT: &str = "tenant-a";
const LIBRARY: &str = "library-a";

fn identity() -> EmbeddingModelIdentity {
    EmbeddingModelIdentity {
        model_id: "embed-model".into(),
        model_revision: "embed-revision".into(),
        tokenizer: "tokenizer-revision".into(),
        dimensions: DIMENSIONS,
        max_input_tokens: 8_192,
    }
}

/// Deterministic pseudo-noise in `[-0.05, 0.05]`.
fn noise(seed: u64) -> f32 {
    let mixed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    ((mixed >> 33) % 1_000) as f32 / 10_000.0 - 0.05
}

/// Unit vector near `topic`'s axis.
fn topic_vector(topic: usize, seed: u64) -> Vec<f32> {
    let mut vector = (0..DIMENSIONS)
        .map(|dimension| noise(seed * 31 + dimension as u64))
        .collect::<Vec<_>>();
    vector[topic] += 1.0;
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    vector.iter().map(|value| value / norm).collect()
}

#[derive(Clone, Copy)]
struct ChunkSpec {
    topic: usize,
    word: &'static str,
    /// Overrides the structural heading of the chunk.
    section: Option<&'static str>,
}

const TOPIC_WORDS: [&str; 4] = ["harbour", "mortgage", "volcano", "violin"];

fn spec(topic: usize) -> ChunkSpec {
    ChunkSpec {
        topic,
        word: TOPIC_WORDS[topic],
        section: None,
    }
}

fn open_catalog() -> SemanticCatalog {
    let directory = tempdir().unwrap().keep();
    let catalog = SemanticCatalog::open(directory.join("catalog.sqlite")).unwrap();
    let identity = identity();
    catalog
        .register_library(
            TENANT,
            LIBRARY,
            &LibraryIndexManifest {
                zvec_schema_version: 1,
                dimensions: DIMENSIONS,
                distance_metric: DistanceMetric::Cosine,
                model_revision: identity.model_revision.clone(),
                tokenizer: identity.tokenizer.clone(),
                embedding_preprocessing: "unicode-default-case-fold/1".into(),
                normalization: VectorNormalization::L2,
                chunker_version: "structural/2".into(),
                converter_version: "text/1".into(),
            },
        )
        .unwrap();
    catalog
}

fn stage_document(
    catalog: &SemanticCatalog,
    document: &str,
    generation: u64,
    chunks: &[ChunkSpec],
) {
    let identity = identity();
    let occurrence = Occurrence {
        occurrence_id: format!("occurrence-{document}"),
        source_id: format!("source-{document}"),
        root_id: "root-a".into(),
        workspace_id: Some("workspace-a".into()),
        media_type: "text/plain".into(),
        modified_at_ms: 1_000,
        available: true,
        provenance: "source".into(),
    };
    let records = chunks
        .iter()
        .enumerate()
        .map(|(position, chunk)| {
            let content = format!(
                "{} {} notes {document} part {position} generation {generation}",
                chunk.word, chunk.word
            );
            StagedRecord {
                record_id: format!("{document}-g{generation}-c{position}"),
                occurrence_id: occurrence.occurrence_id.clone(),
                cache_key: EmbeddingCacheKey::calculate(
                    &content,
                    &identity,
                    &identity.tokenizer,
                    "structural/2",
                ),
                vector: topic_vector(
                    chunk.topic,
                    document.bytes().map(u64::from).sum::<u64>() * 97
                        + position as u64
                        + generation * 7,
                ),
                record_kind: "chunk".into(),
                excerpt: content.clone(),
                content,
                token_count: 10,
                section_path: vec![
                    chunk
                        .section
                        .map_or_else(|| format!("section {position}"), str::to_owned),
                ],
                structural_role: "body".into(),
                provenance: format!(
                    "{{\"kind\":\"textLines\",\"start_line\":{},\"end_line\":{}}}",
                    position + 1,
                    position + 1
                ),
                source_position: u32::try_from(position).unwrap(),
                generated: false,
                concept_id: None,
            }
        })
        .collect();
    catalog
        .stage_generation(&StagedGeneration {
            tenant_id: TENANT.into(),
            library_id: LIBRARY.into(),
            document_id: document.into(),
            content_hash: format!("hash-{document}-{generation}"),
            source_complete: true,
            pdf_image_skipped_pages: None,
            generation,
            occurrences: vec![occurrence],
            records,
        })
        .unwrap();
    catalog
        .publish_generation(TENANT, LIBRARY, document, generation)
        .unwrap();
}

/// Single-topic and two-topic files over four topics, staged in `order`.
fn stage_corpus(catalog: &SemanticCatalog, order: &[&str]) {
    for document in order {
        let topics: &[usize] = match *document {
            "doc-a" => &[0, 0, 0, 0],
            "doc-b" => &[1, 1, 1, 1],
            "doc-c" => &[2, 2, 2, 2],
            "doc-d" => &[3, 3, 3, 3],
            "doc-e" => &[0, 1, 0, 1, 0, 1],
            "doc-f" => &[2, 3, 2, 3, 2, 3],
            "doc-g" => &[0, 0, 1, 1],
            _ => &[2, 2, 3, 3],
        };
        let chunks = topics.iter().map(|topic| spec(*topic)).collect::<Vec<_>>();
        stage_document(catalog, document, 1, &chunks);
    }
}

const ALL_DOCS: [&str; 8] = [
    "doc-a", "doc-b", "doc-c", "doc-d", "doc-e", "doc-f", "doc-g", "doc-h",
];

fn scope_of(documents: &[&str]) -> CorpusScope {
    CorpusScope {
        tenant_id: TENANT.into(),
        library_id: LIBRARY.into(),
        source_ids: documents
            .iter()
            .map(|document| format!("source-{document}"))
            .collect(),
    }
}

fn analyze(service: &CorpusThemeService, scope: &CorpusScope) -> CorpusThemes {
    service
        .analyze(
            scope,
            &CorpusThemeConfig::default(),
            &CancellationToken::new(),
        )
        .unwrap()
}

fn theme_for(themes: &CorpusThemes, word: &str) -> u32 {
    themes
        .themes
        .iter()
        .find(|theme| theme.keywords.iter().any(|keyword| keyword.term == word))
        .unwrap_or_else(|| panic!("no theme labelled {word}: {themes:#?}"))
        .id
}

#[test]
fn recovers_known_topics_from_interleaved_multi_topic_documents() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let service = CorpusThemeService::new(catalog, identity());
    let themes = analyze(&service, &scope_of(&ALL_DOCS));

    assert_eq!(themes.themes.len(), 4, "{themes:#?}");
    assert_eq!(themes.no_theme.chunk_count, 0);
    assert_eq!(themes.chunk_count, 4 * 4 + 6 + 6 + 4 + 4);
    let ids = TOPIC_WORDS.map(|word| theme_for(&themes, word));
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        4
    );
    for theme in &themes.themes {
        assert!(!theme.passages.is_empty());
        assert!(theme.passages.iter().all(|passage| {
            passage.provenance.contains("start_line") && !passage.excerpt.is_empty()
        }));
    }
    assert!(
        themes
            .themes
            .windows(2)
            .all(|pair| pair[0].chunk_count >= pair[1].chunk_count)
    );

    let mixture = |document: &str| {
        themes
            .document_mixtures
            .iter()
            .find(|mixture| mixture.document_id == document)
            .unwrap()
    };
    let single = mixture("doc-a");
    assert_eq!(single.shares.len(), 1);
    assert_eq!(single.shares[0].theme, Some(theme_for(&themes, "harbour")));
    let mixed = mixture("doc-e");
    assert_eq!(mixed.shares.len(), 2);
    assert!((mixed.shares.iter().map(|share| share.share).sum::<f32>() - 1.0).abs() < 1e-5);
    let mixed_themes = mixed
        .shares
        .iter()
        .map(|share| share.theme)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(mixed_themes.contains(&Some(theme_for(&themes, "harbour"))));
    assert!(mixed_themes.contains(&Some(theme_for(&themes, "mortgage"))));
}

#[test]
fn result_is_independent_of_insertion_order_and_repeatable() {
    let forward = open_catalog();
    stage_corpus(&forward, &ALL_DOCS);
    let reversed = open_catalog();
    let mut backwards = ALL_DOCS;
    backwards.reverse();
    stage_corpus(&reversed, &backwards);

    let scope = scope_of(&ALL_DOCS);
    let mut shuffled_scope = scope.clone();
    shuffled_scope.source_ids.reverse();
    let first = analyze(&CorpusThemeService::new(forward, identity()), &scope);
    let second = analyze(
        &CorpusThemeService::new(reversed, identity()),
        &shuffled_scope,
    );
    assert_eq!(first, second);
}

#[test]
fn repeated_request_is_served_from_the_persisted_run() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let service = CorpusThemeService::new(catalog, identity());
    let scope = scope_of(&ALL_DOCS);
    let first = analyze(&service, &scope);
    assert!(!first.from_cache);
    let second = analyze(&service, &scope);
    assert!(second.from_cache);
    assert_eq!(first.themes, second.themes);
}

#[test]
fn far_away_chunks_fall_into_the_no_theme_bucket() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &["doc-a", "doc-b", "doc-c", "doc-d"]);
    // A chunk on an axis none of the four topics use.
    let identity = identity();
    let content =
        "stray remark about nothing in particular, entirely unrelated to the rest".to_owned();
    catalog
        .stage_generation(&StagedGeneration {
            tenant_id: TENANT.into(),
            library_id: LIBRARY.into(),
            document_id: "doc-stray".into(),
            content_hash: "hash-stray".into(),
            source_complete: true,
            pdf_image_skipped_pages: None,
            generation: 1,
            occurrences: vec![Occurrence {
                occurrence_id: "occurrence-doc-stray".into(),
                source_id: "source-doc-stray".into(),
                root_id: "root-a".into(),
                workspace_id: None,
                media_type: "text/plain".into(),
                modified_at_ms: 1_000,
                available: true,
                provenance: "source".into(),
            }],
            records: vec![StagedRecord {
                record_id: "stray-0".into(),
                occurrence_id: "occurrence-doc-stray".into(),
                cache_key: EmbeddingCacheKey::calculate(
                    &content,
                    &identity,
                    &identity.tokenizer,
                    "structural/2",
                ),
                vector: topic_vector(6, 5),
                record_kind: "chunk".into(),
                excerpt: content.clone(),
                content,
                token_count: 5,
                section_path: vec![],
                structural_role: "body".into(),
                provenance: "{}".into(),
                source_position: 0,
                generated: false,
                concept_id: None,
            }],
        })
        .unwrap();
    catalog
        .publish_generation(TENANT, LIBRARY, "doc-stray", 1)
        .unwrap();
    let service = CorpusThemeService::new(catalog, identity);
    let themes = analyze(
        &service,
        &scope_of(&["doc-a", "doc-b", "doc-c", "doc-d", "doc-stray"]),
    );
    assert_eq!(themes.themes.len(), 4, "{themes:#?}");
    assert_eq!(themes.no_theme.chunk_count, 1);
    assert_eq!(themes.no_theme.passages[0].chunk_id, "stray-0");
    let stray = themes
        .document_mixtures
        .iter()
        .find(|mixture| mixture.document_id == "doc-stray")
        .unwrap();
    assert_eq!(stray.shares[0].theme, None);
}

#[test]
fn compact_hashing_profile_gets_a_typed_refusal() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let mut weak = identity();
    weak.model_id = TOKEN_HASHING_MODEL_ID.into();
    let service = CorpusThemeService::new(catalog, weak);
    let error = service
        .analyze(
            &scope_of(&ALL_DOCS),
            &CorpusThemeConfig::default(),
            &CancellationToken::new(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        CorpusThemeError::ProfileTooWeak { ref model_id } if model_id == TOKEN_HASHING_MODEL_ID
    ));
}

#[test]
fn vectors_of_another_profile_are_refused_not_mixed() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let mut other = identity();
    other.model_revision = "another-revision".into();
    let service = CorpusThemeService::new(catalog.clone(), other);
    assert!(matches!(
        service.analyze(
            &scope_of(&ALL_DOCS),
            &CorpusThemeConfig::default(),
            &CancellationToken::new()
        ),
        Err(CorpusThemeError::ProfileMismatch)
    ));
    // A centroid written under one profile is invisible to another.
    let native = CorpusThemeService::new(catalog, identity());
    let scope = scope_of(&ALL_DOCS);
    analyze(&native, &scope);
    assert!(native.document_centroid(&scope, "doc-a").unwrap().is_some());
    assert!(
        service
            .document_centroid(&scope, "doc-a")
            .unwrap()
            .is_none()
    );
}

#[test]
fn empty_tiny_and_invalid_scopes_are_typed() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &["doc-a"]);
    let service = CorpusThemeService::new(catalog, identity());
    let config = CorpusThemeConfig::default();
    let token = CancellationToken::new();
    assert!(matches!(
        service.analyze(&scope_of(&[]), &config, &token),
        Err(CorpusThemeError::EmptyScope)
    ));
    let mut few = config.clone();
    few.min_chunks = 4;
    assert!(service.analyze(&scope_of(&["doc-a"]), &few, &token).is_ok());
    few.min_chunks = 9;
    assert!(matches!(
        service.analyze(&scope_of(&["doc-a"]), &few, &token),
        Err(CorpusThemeError::InsufficientContent { chunks: 4 })
    ));
    let mut invalid = config;
    invalid.max_themes = 1;
    invalid.min_themes = 2;
    assert!(matches!(
        service.analyze(&scope_of(&["doc-a"]), &invalid, &token),
        Err(CorpusThemeError::InvalidConfiguration)
    ));
}

#[test]
fn cancelled_run_leaves_no_run_and_a_rerun_matches_an_uninterrupted_one() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let service = CorpusThemeService::new(catalog.clone(), identity());
    let scope = scope_of(&ALL_DOCS);
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(matches!(
        service.analyze(&scope, &CorpusThemeConfig::default(), &cancelled),
        Err(CorpusThemeError::Cancelled)
    ));
    assert!(catalog.corpus_theme_run(&scope).unwrap().is_none());
    let rerun = analyze(&service, &scope);

    let fresh_catalog = open_catalog();
    stage_corpus(&fresh_catalog, &ALL_DOCS);
    let fresh = analyze(&CorpusThemeService::new(fresh_catalog, identity()), &scope);
    assert_eq!(rerun, fresh);
}

#[test]
fn large_corpora_fit_a_sample_but_assign_every_chunk() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let service = CorpusThemeService::new(catalog, identity());
    let config = CorpusThemeConfig {
        max_fit_chunks: 20,
        ..CorpusThemeConfig::default()
    };
    let themes = service
        .analyze(&scope_of(&ALL_DOCS), &config, &CancellationToken::new())
        .unwrap();
    assert!(themes.sampled);
    assert!(themes.fitted_chunks <= 20);
    let assigned = themes
        .themes
        .iter()
        .map(|theme| theme.chunk_count)
        .sum::<u64>()
        + themes.no_theme.chunk_count;
    assert_eq!(assigned, themes.chunk_count);
    assert_eq!(themes.themes.len(), 4, "{themes:#?}");
}

#[test]
fn centroids_are_normalized_means_invalidated_per_changed_document() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let service = CorpusThemeService::new(catalog.clone(), identity());
    let scope = scope_of(&ALL_DOCS);
    let first = analyze(&service, &scope);
    assert_eq!(first.centroids_recomputed, 8);
    assert_eq!(first.centroids_reused, 0);

    let centroid = service.document_centroid(&scope, "doc-b").unwrap().unwrap();
    let norm = centroid
        .vector
        .iter()
        .map(|value| value * value)
        .sum::<f32>();
    assert!((norm - 1.0).abs() < 1e-4);
    assert_eq!(centroid.model_tag, service.model_tag());
    assert_eq!(centroid.chunk_count, 4);
    assert!(centroid.vector[1] > 0.9, "doc-b is the topic-1 document");

    // A different seed bypasses the run cache; nothing changed, so every
    // centroid is reused.
    let reseeded = CorpusThemeConfig {
        seed: 7,
        ..CorpusThemeConfig::default()
    };
    let second = service
        .analyze(&scope, &reseeded, &CancellationToken::new())
        .unwrap();
    assert_eq!(second.centroids_recomputed, 0);
    assert_eq!(second.centroids_reused, 8);

    // One document changes topic: only its centroid is recomputed.
    stage_document(&catalog, "doc-b", 2, &[spec(3), spec(3), spec(3), spec(3)]);
    let third = analyze(&service, &scope);
    assert_eq!(third.centroids_recomputed, 1);
    assert_eq!(third.centroids_reused, 7);
    let changed = service.document_centroid(&scope, "doc-b").unwrap().unwrap();
    assert_eq!(changed.generation, 2);
    assert!(changed.vector[3] > 0.9);
}

#[test]
fn excluded_sources_stay_out_and_deleted_occurrences_drop_derived_state() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let service = CorpusThemeService::new(catalog.clone(), identity());
    let everything = scope_of(&ALL_DOCS);
    analyze(&service, &everything);

    // Consent narrowed: the excluded document never appears.
    let remaining = ALL_DOCS
        .iter()
        .copied()
        .filter(|document| *document != "doc-d")
        .collect::<Vec<_>>();
    let narrowed = analyze(&service, &scope_of(&remaining));
    assert!(
        narrowed
            .document_mixtures
            .iter()
            .all(|mixture| mixture.document_id != "doc-d")
    );
    assert!(
        narrowed
            .themes
            .iter()
            .flat_map(|theme| &theme.passages)
            .all(|passage| passage.document_id != "doc-d")
    );

    // Revocation deletes the occurrence: centroid and cached runs go too.
    assert!(
        service
            .document_centroid(&everything, "doc-d")
            .unwrap()
            .is_some()
    );
    catalog.delete_occurrence("occurrence-doc-d").unwrap();
    assert!(
        service
            .document_centroid(&everything, "doc-d")
            .unwrap()
            .is_none()
    );
    assert!(catalog.corpus_theme_run(&everything).unwrap().is_none());
    assert!(
        catalog
            .corpus_theme_run(&scope_of(&remaining))
            .unwrap()
            .is_none()
    );
    let after = analyze(&service, &everything);
    assert!(
        after
            .document_mixtures
            .iter()
            .all(|mixture| mixture.document_id != "doc-d")
    );
}

#[test]
fn index_revision_tracks_the_visible_documents() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let service = CorpusThemeService::new(catalog.clone(), identity());
    let documents = catalog.corpus_documents(&scope_of(&ALL_DOCS)).unwrap();
    assert_eq!(documents.len(), 8);
    assert!(
        documents
            .windows(2)
            .all(|pair| pair[0].document_id < pair[1].document_id)
    );
    let before = service.index_revision(&scope_of(&ALL_DOCS)).unwrap();
    assert_eq!(before.len(), 64);
    assert_ne!(
        before,
        service.index_revision(&scope_of(&["doc-a"])).unwrap()
    );
    stage_document(&catalog, "doc-a", 2, &[spec(0), spec(0)]);
    assert_ne!(
        before,
        service.index_revision(&scope_of(&ALL_DOCS)).unwrap()
    );
}

fn ids_by_word(themes: &CorpusThemes) -> Vec<u32> {
    TOPIC_WORDS.map(|word| theme_for(themes, word)).to_vec()
}

#[test]
fn small_changes_keep_the_fit_and_the_theme_ids_stable() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let service = CorpusThemeService::new(catalog.clone(), identity());
    let scope = scope_of(&ALL_DOCS);
    let first = analyze(&service, &scope);
    assert!(!first.incremental);

    // doc-g (4 of 40 chunks) is rewritten with the same topics.
    stage_document(&catalog, "doc-g", 2, &[spec(0), spec(0), spec(1), spec(1)]);
    let second = analyze(&service, &scope);
    assert!(second.incremental, "a 10% change keeps the fitted themes");
    assert!(!second.from_cache);
    assert_ne!(second.index_revision, first.index_revision);
    assert_eq!(ids_by_word(&second), ids_by_word(&first));
    assert_eq!(second.fitted_chunks, first.fitted_chunks);
    assert_eq!(second.centroids_recomputed, 1);
    assert_eq!(second.centroids_reused, 7);
    assert_eq!(second.chunk_count, first.chunk_count);
    // The new generation is what the labels and mixtures describe.
    assert!(
        second
            .themes
            .iter()
            .flat_map(|theme| &theme.passages)
            .all(|passage| !passage.chunk_id.starts_with("doc-g-g1"))
    );
}

#[test]
fn large_changes_or_a_different_configuration_refit_from_scratch() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let service = CorpusThemeService::new(catalog.clone(), identity());
    let scope = scope_of(&ALL_DOCS);
    analyze(&service, &scope);

    for document in ["doc-a", "doc-b", "doc-c", "doc-d", "doc-e"] {
        stage_document(&catalog, document, 2, &[spec(0), spec(1), spec(2), spec(3)]);
    }
    let refit = analyze(&service, &scope);
    assert!(!refit.incremental, "most chunks changed");

    stage_document(&catalog, "doc-g", 2, &[spec(0), spec(0), spec(1), spec(1)]);
    let reseeded = CorpusThemeConfig {
        seed: 3,
        ..CorpusThemeConfig::default()
    };
    let other = service
        .analyze(&scope, &reseeded, &CancellationToken::new())
        .unwrap();
    assert!(!other.incremental, "another configuration is another fit");
}

#[test]
fn theme_sources_feed_search_with_the_contributing_sources_only() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let service = CorpusThemeService::new(catalog.clone(), identity());
    let scope = scope_of(&ALL_DOCS);
    let themes = analyze(&service, &scope);
    let token = CancellationToken::new();
    let harbour = theme_for(&themes, "harbour");

    let sources = service
        .theme_sources(&scope, Some(harbour), &themes.fingerprint, 50, &token)
        .unwrap();
    // doc-a (4 chunks) leads; the mixed docs follow by their harbour share.
    assert_eq!(sources.source_ids[0], "source-doc-a");
    let found = sources
        .source_ids
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        found,
        ["source-doc-a", "source-doc-e", "source-doc-g"]
            .into_iter()
            .collect()
    );
    assert!(!sources.truncated);

    let limited = service
        .theme_sources(&scope, Some(harbour), &themes.fingerprint, 2, &token)
        .unwrap();
    assert_eq!(limited.source_ids.len(), 2);
    assert!(limited.truncated);

    let none = service
        .theme_sources(&scope, None, &themes.fingerprint, 50, &token)
        .unwrap();
    assert!(none.source_ids.is_empty());
}

#[test]
fn theme_sources_refuse_stale_or_foreign_analyses_and_revoked_sources() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let service = CorpusThemeService::new(catalog.clone(), identity());
    let scope = scope_of(&ALL_DOCS);
    let themes = analyze(&service, &scope);
    let token = CancellationToken::new();
    assert!(matches!(
        service.theme_sources(&scope, Some(0), "someone-elses-fingerprint", 10, &token),
        Err(CorpusThemeError::ThemesStale)
    ));
    // A narrower scope has no analysis of its own.
    assert!(matches!(
        service.theme_sources(
            &scope_of(&["doc-a"]),
            Some(0),
            &themes.fingerprint,
            10,
            &token
        ),
        Err(CorpusThemeError::ThemesStale)
    ));
    // The index moved on after the analysis.
    stage_document(&catalog, "doc-a", 2, &[spec(0), spec(0)]);
    assert!(matches!(
        service.theme_sources(&scope, Some(0), &themes.fingerprint, 10, &token),
        Err(CorpusThemeError::ThemesStale)
    ));
    // Revocation drops the cached run altogether.
    let fresh = analyze(&service, &scope);
    catalog.delete_occurrence("occurrence-doc-b").unwrap();
    assert!(matches!(
        service.theme_sources(&scope, Some(0), &fresh.fingerprint, 10, &token),
        Err(CorpusThemeError::ThemesStale)
    ));
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let again = analyze(&service, &scope);
    assert!(matches!(
        service.theme_sources(&scope, Some(0), &again.fingerprint, 10, &cancelled),
        Err(CorpusThemeError::Cancelled)
    ));
}

fn mean_point(themes: &CorpusThemes, theme: u32) -> (f32, f32) {
    let points = themes
        .map_points
        .iter()
        .filter(|point| point.theme == Some(theme))
        .collect::<Vec<_>>();
    assert!(!points.is_empty(), "theme {theme} has no map points");
    (
        points.iter().map(|point| point.x).sum::<f32>() / points.len() as f32,
        points.iter().map(|point| point.y).sum::<f32>() / points.len() as f32,
    )
}

#[test]
fn map_projects_every_chunk_within_bounds_and_separates_themes() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let service = CorpusThemeService::new(catalog, identity());
    let themes = analyze(&service, &scope_of(&ALL_DOCS));
    assert_eq!(themes.map_points.len() as u64, themes.chunk_count);
    assert!(!themes.map_sampled);
    assert!(
        themes
            .map_points
            .iter()
            .all(|point| point.x.abs() <= 1.0 && point.y.abs() <= 1.0)
    );
    assert!(themes.map_points.iter().all(|point| {
        point.passage.provenance.contains("start_line")
            && point.passage.excerpt.chars().count() <= 120
    }));
    for left in &themes.themes {
        for right in &themes.themes {
            if left.id < right.id {
                let a = mean_point(&themes, left.id);
                let b = mean_point(&themes, right.id);
                let distance = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
                assert!(
                    distance > 0.2,
                    "themes {} and {} overlap",
                    left.id,
                    right.id
                );
            }
        }
    }
    assert_eq!(
        themes.map_points,
        analyze(&service, &scope_of(&ALL_DOCS)).map_points
    );
}

#[test]
fn map_is_bounded_by_a_stride_sample_that_still_covers_every_theme() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let service = CorpusThemeService::new(catalog, identity());
    let config = CorpusThemeConfig {
        max_map_points: 12,
        ..CorpusThemeConfig::default()
    };
    let themes = service
        .analyze(&scope_of(&ALL_DOCS), &config, &CancellationToken::new())
        .unwrap();
    assert!(themes.map_sampled);
    assert!(themes.map_points.len() <= 12, "{}", themes.map_points.len());
    assert!(themes.map_points.len() >= 6);
    // Counts still describe all chunks, only the map is thinned.
    assert_eq!(themes.chunk_count, 4 * 4 + 6 + 6 + 4 + 4);
    let covered = themes
        .map_points
        .iter()
        .filter_map(|point| point.theme)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(covered.len(), themes.themes.len());
    let invalid = CorpusThemeConfig {
        max_map_points: 0,
        ..CorpusThemeConfig::default()
    };
    assert!(matches!(
        service.analyze(&scope_of(&ALL_DOCS), &invalid, &CancellationToken::new()),
        Err(CorpusThemeError::InvalidConfiguration)
    ));
}

#[test]
fn labels_ignore_stopwords_and_rank_distinctive_terms() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let service = CorpusThemeService::new(catalog, identity());
    let themes = analyze(&service, &scope_of(&ALL_DOCS));
    for theme in &themes.themes {
        assert!(theme.keywords.len() <= 6);
        assert!(
            theme
                .keywords
                .windows(2)
                .all(|pair| pair[0].score >= pair[1].score)
        );
        assert!(theme.keywords.iter().all(|keyword| keyword.term != "the"));
    }
    assert!(terms("The harbour and 1234 of 42 harbours").eq(["harbour", "harbours"]));
}

#[test]
fn references_and_contents_do_not_shape_the_themes() {
    let catalog = open_catalog();
    stage_corpus(&catalog, &ALL_DOCS);
    let reference = ChunkSpec {
        section: Some("7. References"),
        ..spec(0)
    };
    let contents = ChunkSpec {
        section: Some("Contents"),
        ..spec(1)
    };
    stage_document(
        &catalog,
        "doc-a",
        2,
        &[spec(0), spec(0), spec(0), spec(0), reference, contents],
    );
    let service = CorpusThemeService::new(catalog, identity());
    let themes = analyze(&service, &scope_of(&ALL_DOCS));
    // doc-a keeps its four body chunks; the two back-matter chunks are not counted.
    assert_eq!(
        themes.chunk_count,
        4 + 4 + 4 + 4 + 6 + 6 + 4 + 4,
        "{themes:#?}"
    );
}
