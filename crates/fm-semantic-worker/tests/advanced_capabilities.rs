//! Optional acceleration, reranking, and Hybrid-mode acceptance tests.

use std::sync::Arc;

use fm_semantic_worker::advanced::{
    AcceleratedEmbedding, AccelerationBackend, AccelerationPolicy, AdvancedSemanticError,
    HybridCandidate, HybridFusionPolicy, RerankCandidate, RerankerBackend, RerankerPolicy,
    ScopedRetrieval, fuse_hybrid, rerank_candidates,
};
use tokio_util::sync::CancellationToken;

struct FixedAcceleration {
    result: Result<Vec<Vec<f32>>, String>,
}

impl AccelerationBackend for FixedAcceleration {
    fn embed(
        &self,
        _inputs: &[String],
        _cancellation: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, String> {
        self.result.clone()
    }
}

#[test]
fn acceleration_falls_back_to_cpu_and_rejects_silent_vector_drift() {
    let cpu = Arc::new(FixedAcceleration {
        result: Ok(vec![vec![1.0, 0.0]]),
    });
    let failed = AcceleratedEmbedding::new(
        Arc::clone(&cpu) as Arc<dyn AccelerationBackend>,
        Some(Arc::new(FixedAcceleration {
            result: Err("driver unavailable".into()),
        })),
        AccelerationPolicy::parity_required(0.000_1),
    )
    .expect("configuration");
    let fallback = failed
        .embed(&["query".into()], &CancellationToken::new())
        .expect("CPU fallback");
    assert!(fallback.used_cpu_fallback);
    assert_eq!(fallback.vectors, vec![vec![1.0, 0.0]]);

    let drifted = AcceleratedEmbedding::new(
        cpu,
        Some(Arc::new(FixedAcceleration {
            result: Ok(vec![vec![0.0, 1.0]]),
        })),
        AccelerationPolicy::parity_required(0.000_1),
    )
    .expect("configuration");
    assert!(matches!(
        drifted.embed(&["query".into()], &CancellationToken::new()),
        Err(AdvancedSemanticError::AccelerationParity { .. })
    ));

    let migrated = AcceleratedEmbedding::new(
        Arc::new(FixedAcceleration {
            result: Ok(vec![vec![1.0, 0.0]]),
        }),
        Some(Arc::new(FixedAcceleration {
            result: Ok(vec![vec![0.0, 1.0]]),
        })),
        AccelerationPolicy::migration_declared("cpu-space-v1", "gpu-space-v2")
            .expect("distinct model spaces"),
    )
    .expect("migration policy");
    let result = migrated
        .embed(&["query".into()], &CancellationToken::new())
        .expect("explicit migration");
    assert_eq!(
        result
            .model_space_migration
            .expect("migration identity")
            .candidate_fingerprint,
        "gpu-space-v2"
    );
}

struct ReverseReranker;

impl RerankerBackend for ReverseReranker {
    fn model_fingerprint(&self) -> &str {
        "reranker-v1"
    }

    fn rerank(
        &self,
        candidates: &[RerankCandidate],
        _cancellation: &CancellationToken,
    ) -> Result<Vec<(String, f32)>, String> {
        Ok(candidates
            .iter()
            .rev()
            .map(|candidate| (candidate.id.clone(), candidate.dense_score))
            .collect())
    }
}

struct MalformedReranker;

impl RerankerBackend for MalformedReranker {
    fn model_fingerprint(&self) -> &str {
        "malformed-reranker-v1"
    }

    fn rerank(
        &self,
        candidates: &[RerankCandidate],
        _cancellation: &CancellationToken,
    ) -> Result<Vec<(String, f32)>, String> {
        Ok(vec![(candidates[0].id.clone(), f32::NAN)])
    }
}

#[test]
fn reranker_is_quality_gated_bounded_and_cancellable() {
    let policy = RerankerPolicy::new(2, 0.02, 0.50, 0.54).expect("material gain");
    let candidates = vec![
        RerankCandidate::new("a", 0.9, "first").unwrap(),
        RerankCandidate::new("b", 0.8, "second").unwrap(),
    ];
    let result = rerank_candidates(
        &ReverseReranker,
        &policy,
        &candidates,
        &CancellationToken::new(),
    )
    .expect("rerank");
    assert_eq!(result.candidates[0].id, "b");
    assert_eq!(result.cost.candidate_count, 2);
    assert_eq!(result.cost.model_fingerprint, "reranker-v1");

    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(matches!(
        rerank_candidates(&ReverseReranker, &policy, &candidates, &cancelled),
        Err(AdvancedSemanticError::Cancelled)
    ));
    assert!(matches!(
        rerank_candidates(
            &ReverseReranker,
            &policy,
            &[
                candidates.clone(),
                vec![RerankCandidate::new("c", 0.7, "third").unwrap()]
            ]
            .concat(),
            &CancellationToken::new(),
        ),
        Err(AdvancedSemanticError::CandidateLimit { .. })
    ));
    assert!(matches!(
        rerank_candidates(
            &MalformedReranker,
            &policy,
            &candidates,
            &CancellationToken::new(),
        ),
        Err(AdvancedSemanticError::MalformedRerankerOutput)
    ));
}

#[test]
fn hybrid_fusion_is_named_stable_filtered_and_tenant_scoped() {
    let scope = ScopedRetrieval::new("tenant-a", "library-a").unwrap();
    let dense = vec![
        HybridCandidate::new(&scope, "doc-a", 0.9, true).unwrap(),
        HybridCandidate::new(&scope, "doc-b", 0.8, true).unwrap(),
    ];
    let lexical = vec![
        HybridCandidate::new(&scope, "filtered", 100.0, false).unwrap(),
        HybridCandidate::new(&scope, "doc-b", 12.0, true).unwrap(),
        HybridCandidate::new(&scope, "doc-a", 10.0, true).unwrap(),
    ];
    let policy = HybridFusionPolicy::new(0.7, 0.3, 60, 20).unwrap();
    let first = fuse_hybrid(&scope, &dense, &lexical, 10, policy).expect("hybrid");
    let second = fuse_hybrid(&scope, &dense, &lexical, 10, policy).expect("hybrid");
    let without_filtered = fuse_hybrid(&scope, &dense, &lexical[1..], 10, policy)
        .expect("hybrid without filtered item");
    assert_eq!(first, second);
    assert_eq!(first, without_filtered);
    assert_eq!(first.mode, "hybrid");
    assert_eq!(
        first
            .results
            .iter()
            .map(|candidate| candidate.id.as_str())
            .collect::<Vec<_>>(),
        vec!["doc-a", "doc-b"]
    );

    let foreign = ScopedRetrieval::new("tenant-b", "library-a").unwrap();
    assert!(matches!(
        fuse_hybrid(
            &scope,
            &[HybridCandidate::new(&foreign, "foreign", 1.0, true).unwrap()],
            &[],
            10,
            policy,
        ),
        Err(AdvancedSemanticError::ScopeMismatch)
    ));
}
