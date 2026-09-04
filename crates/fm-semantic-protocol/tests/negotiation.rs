//! Public protocol-version negotiation tests.

use fm_semantic_protocol::{
    CompatibilityError, Negotiator, ProtocolLimits, ProtocolVersion, VersionRange,
    negotiate_version, v1,
};

#[test]
fn negotiates_rolling_versions_and_reports_required_updates() {
    let worker = VersionRange::new(ProtocolVersion::new(1), ProtocolVersion::new(2)).unwrap();

    assert_eq!(
        negotiate_version(
            VersionRange::new(ProtocolVersion::new(2), ProtocolVersion::new(3)).unwrap(),
            worker,
        ),
        Ok(ProtocolVersion::new(2))
    );
    assert_eq!(
        negotiate_version(
            VersionRange::new(ProtocolVersion::new(0), ProtocolVersion::new(0)).unwrap(),
            worker,
        ),
        Err(CompatibilityError::ClientUpdateRequired {
            minimum_supported: ProtocolVersion::new(1),
        })
    );
    assert_eq!(
        negotiate_version(
            VersionRange::new(ProtocolVersion::new(3), ProtocolVersion::new(4)).unwrap(),
            worker,
        ),
        Err(CompatibilityError::WorkerUpdateRequired {
            maximum_supported: ProtocolVersion::new(2),
            minimum_required: ProtocolVersion::new(3),
        })
    );

    let error = negotiate_version(
        VersionRange::new(ProtocolVersion::new(3), ProtocolVersion::new(4)).unwrap(),
        worker,
    )
    .unwrap_err();
    let wire_error = error.to_wire();
    assert_eq!(
        wire_error.code,
        fm_semantic_protocol::v1::ErrorCode::WorkerUpdateRequired as i32
    );
    assert_eq!(wire_error.required_protocol_version, 3);
    assert!(!wire_error.retryable);
}

#[test]
fn negotiates_requested_capabilities_and_advertises_limits() {
    let negotiator = Negotiator::new(
        VersionRange::new(ProtocolVersion::new(1), ProtocolVersion::new(2)).unwrap(),
        [
            v1::Capability::Ingestion,
            v1::Capability::Query,
            v1::Capability::Cancellation,
        ],
        ProtocolLimits::default(),
    );
    let response = negotiator
        .negotiate(&v1::NegotiateRequest {
            minimum_version: 1,
            maximum_version: 3,
            requested_capabilities: vec![
                v1::Capability::Query as i32,
                v1::Capability::Events as i32,
                v1::Capability::Cancellation as i32,
            ],
        })
        .unwrap();

    assert_eq!(response.selected_version, 2);
    assert_eq!(
        response.capabilities,
        vec![
            v1::Capability::Query as i32,
            v1::Capability::Cancellation as i32,
        ]
    );
    let limits = response.limits.unwrap();
    assert_eq!(
        limits.maximum_stream_bytes,
        ProtocolLimits::default().max_stream_bytes()
    );
}
