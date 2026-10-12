//! Public resource-limit tests.

use std::time::Duration;

use fm_semantic_protocol::{
    ConcurrencyLimiter, DeadlineKind, InvalidLimits, LimitError, LimitField,
    MAX_CONCURRENT_REQUESTS, MAX_MESSAGE_BYTES, MAX_STREAM_BYTES, NegotiatedLimitsError,
    ProtocolLimits, REQUEST_DEADLINE, RequestValidationError, SHUTDOWN_DEADLINE, STREAM_DEADLINE,
    StreamBudget, v1, validate_ingestion_with_limits,
};

#[test]
fn enforces_message_stream_concurrency_and_time_limits() {
    let limits = ProtocolLimits::default();
    assert_eq!(limits.max_message_bytes(), MAX_MESSAGE_BYTES);
    assert_eq!(limits.max_stream_bytes(), MAX_STREAM_BYTES);
    assert_eq!(limits.max_concurrent_requests(), MAX_CONCURRENT_REQUESTS);
    assert_eq!(limits.deadline(DeadlineKind::Request), REQUEST_DEADLINE);
    assert_eq!(limits.deadline(DeadlineKind::Stream), STREAM_DEADLINE);

    assert_eq!(
        limits.check_message_bytes(MAX_MESSAGE_BYTES + 1),
        Err(LimitError::MessageTooLarge {
            actual: MAX_MESSAGE_BYTES + 1,
            maximum: MAX_MESSAGE_BYTES,
        })
    );

    let mut stream = StreamBudget::new(8);
    assert_eq!(stream.consume(5), Ok(()));
    assert_eq!(
        stream.consume(4),
        Err(LimitError::StreamTooLarge {
            attempted: 9,
            maximum: 8,
        })
    );

    let limiter = ConcurrencyLimiter::new(1).unwrap();
    let permit = limiter.try_acquire().unwrap();
    assert!(matches!(
        limiter.try_acquire(),
        Err(LimitError::TooManyConcurrentRequests { maximum: 1 })
    ));
    drop(permit);
    assert!(limiter.try_acquire().is_ok());

    assert_eq!(
        limits.check_deadline(DeadlineKind::Request, Duration::from_secs(31)),
        Err(LimitError::DeadlineTooLong {
            requested: Duration::from_secs(31),
            maximum: REQUEST_DEADLINE,
        })
    );
}

#[test]
fn pdf_ingestion_limit_requires_explicit_negotiation_and_never_widens_other_media() {
    let ordinary = ProtocolLimits::default();
    assert_eq!(
        ordinary.max_ingestion_bytes("application/pdf"),
        MAX_STREAM_BYTES
    );
    let pdf = ordinary
        .with_pdf_ingestion_bytes(200 * 1024 * 1024)
        .expect("bounded PDF-specific offer");
    assert_eq!(
        pdf.max_ingestion_bytes("application/pdf"),
        200 * 1024 * 1024
    );
    let negotiated = ProtocolLimits::from_negotiated(&v1::ProtocolLimits {
        maximum_message_bytes: MAX_MESSAGE_BYTES as u64,
        maximum_stream_bytes: MAX_STREAM_BYTES,
        maximum_concurrent_requests: MAX_CONCURRENT_REQUESTS as u32,
        request_deadline_ms: REQUEST_DEADLINE.as_millis() as u64,
        stream_deadline_ms: STREAM_DEADLINE.as_millis() as u64,
        shutdown_deadline_ms: SHUTDOWN_DEADLINE.as_millis() as u64,
        maximum_pdf_ingestion_bytes: 200 * 1024 * 1024,
    })
    .expect("bounded PDF-specific negotiation");
    assert_eq!(
        negotiated.max_ingestion_bytes("application/pdf"),
        200 * 1024 * 1024
    );
    assert_eq!(
        negotiated.max_ingestion_bytes("image/png"),
        MAX_STREAM_BYTES
    );
    assert_eq!(
        negotiated.max_ingestion_bytes("application/octet-stream"),
        MAX_STREAM_BYTES
    );
    assert_eq!(negotiated.max_message_bytes(), MAX_MESSAGE_BYTES);
    assert_eq!(negotiated.max_stream_bytes(), MAX_STREAM_BYTES);
    assert!(matches!(
        ordinary.with_pdf_ingestion_bytes(200 * 1024 * 1024 + 1),
        Err(InvalidLimits::ExceedsGlobalCeiling {
            field: LimitField::PdfIngestionBytes,
            ..
        })
    ));
    assert_eq!(
        ordinary.with_pdf_ingestion_bytes(MAX_STREAM_BYTES - 1),
        Err(InvalidLimits::PdfBelowStream)
    );
    assert_eq!(
        ProtocolLimits::from_negotiated(&v1::ProtocolLimits {
            maximum_pdf_ingestion_bytes: 0,
            maximum_message_bytes: MAX_MESSAGE_BYTES as u64,
            maximum_stream_bytes: MAX_STREAM_BYTES,
            maximum_concurrent_requests: MAX_CONCURRENT_REQUESTS as u32,
            request_deadline_ms: REQUEST_DEADLINE.as_millis() as u64,
            stream_deadline_ms: STREAM_DEADLINE.as_millis() as u64,
            shutdown_deadline_ms: SHUTDOWN_DEADLINE.as_millis() as u64,
        })
        .expect("older worker without PDF extension")
        .max_ingestion_bytes("application/pdf"),
        MAX_STREAM_BYTES
    );
}

#[test]
fn declared_pdf_can_use_negotiated_budget_but_identical_non_pdf_cannot() {
    let limits = ProtocolLimits::default()
        .with_pdf_ingestion_bytes(200 * 1024 * 1024)
        .unwrap();
    let mut request = v1::IngestionRequest {
        session: Some(v1::SessionContext {
            session_id: "session".into(),
            session_token: vec![1],
        }),
        scope: Some(v1::ResourceScope {
            tenant_id: "tenant".into(),
            library_id: "library".into(),
        }),
        request_id: "request".into(),
        payload: Some(v1::ingestion_request::Payload::Start(v1::IngestionStart {
            document_id: "document".into(),
            metadata: vec![],
            media_type: "application/pdf".into(),
            expected_content_bytes: 200 * 1024 * 1024,
            pdf_images_consented: false,
        })),
    };
    assert_eq!(validate_ingestion_with_limits(&request, limits), Ok(()));
    if let Some(v1::ingestion_request::Payload::Start(start)) = &mut request.payload {
        start.media_type = "image/png".into();
    }
    assert_eq!(
        validate_ingestion_with_limits(&request, limits),
        Err(RequestValidationError::DeclaredContentTooLarge)
    );
}

#[test]
fn rejects_negotiated_limits_above_immutable_client_ceilings() {
    let defaults = v1::ProtocolLimits {
        maximum_message_bytes: MAX_MESSAGE_BYTES as u64,
        maximum_stream_bytes: MAX_STREAM_BYTES,
        maximum_concurrent_requests: MAX_CONCURRENT_REQUESTS as u32,
        request_deadline_ms: REQUEST_DEADLINE.as_millis() as u64,
        stream_deadline_ms: STREAM_DEADLINE.as_millis() as u64,
        shutdown_deadline_ms: SHUTDOWN_DEADLINE.as_millis() as u64,
        maximum_pdf_ingestion_bytes: 0,
    };
    let malicious = [
        (
            v1::ProtocolLimits {
                maximum_message_bytes: u64::from(u32::MAX),
                ..defaults
            },
            LimitField::MessageBytes,
        ),
        (
            v1::ProtocolLimits {
                maximum_stream_bytes: MAX_STREAM_BYTES + 1,
                ..defaults
            },
            LimitField::StreamBytes,
        ),
        (
            v1::ProtocolLimits {
                maximum_pdf_ingestion_bytes: 200 * 1024 * 1024 + 1,
                ..defaults
            },
            LimitField::PdfIngestionBytes,
        ),
        (
            v1::ProtocolLimits {
                maximum_concurrent_requests: MAX_CONCURRENT_REQUESTS as u32 + 1,
                ..defaults
            },
            LimitField::ConcurrentRequests,
        ),
        (
            v1::ProtocolLimits {
                request_deadline_ms: REQUEST_DEADLINE.as_millis() as u64 + 1,
                ..defaults
            },
            LimitField::RequestDeadline,
        ),
        (
            v1::ProtocolLimits {
                stream_deadline_ms: STREAM_DEADLINE.as_millis() as u64 + 1,
                ..defaults
            },
            LimitField::StreamDeadline,
        ),
        (
            v1::ProtocolLimits {
                shutdown_deadline_ms: SHUTDOWN_DEADLINE.as_millis() as u64 + 1,
                ..defaults
            },
            LimitField::ShutdownDeadline,
        ),
    ];

    for (limits, field) in malicious {
        assert!(matches!(
            ProtocolLimits::from_negotiated(&limits),
            Err(NegotiatedLimitsError::ExceedsClientCeiling {
                field: actual,
                ..
            }) if actual == field
        ));
    }
}

#[test]
fn constructor_rejects_limits_above_immutable_global_ceilings() {
    let excessive = [
        (
            ProtocolLimits::new(
                MAX_MESSAGE_BYTES + 1,
                MAX_STREAM_BYTES,
                MAX_CONCURRENT_REQUESTS,
                REQUEST_DEADLINE,
                STREAM_DEADLINE,
                SHUTDOWN_DEADLINE,
            ),
            LimitField::MessageBytes,
        ),
        (
            ProtocolLimits::new(
                MAX_MESSAGE_BYTES,
                MAX_STREAM_BYTES + 1,
                MAX_CONCURRENT_REQUESTS,
                REQUEST_DEADLINE,
                STREAM_DEADLINE,
                SHUTDOWN_DEADLINE,
            ),
            LimitField::StreamBytes,
        ),
        (
            ProtocolLimits::new(
                MAX_MESSAGE_BYTES,
                MAX_STREAM_BYTES,
                MAX_CONCURRENT_REQUESTS + 1,
                REQUEST_DEADLINE,
                STREAM_DEADLINE,
                SHUTDOWN_DEADLINE,
            ),
            LimitField::ConcurrentRequests,
        ),
        (
            ProtocolLimits::new(
                MAX_MESSAGE_BYTES,
                MAX_STREAM_BYTES,
                MAX_CONCURRENT_REQUESTS,
                REQUEST_DEADLINE + Duration::from_millis(1),
                STREAM_DEADLINE,
                SHUTDOWN_DEADLINE,
            ),
            LimitField::RequestDeadline,
        ),
        (
            ProtocolLimits::new(
                MAX_MESSAGE_BYTES,
                MAX_STREAM_BYTES,
                MAX_CONCURRENT_REQUESTS,
                REQUEST_DEADLINE,
                STREAM_DEADLINE + Duration::from_millis(1),
                SHUTDOWN_DEADLINE,
            ),
            LimitField::StreamDeadline,
        ),
        (
            ProtocolLimits::new(
                MAX_MESSAGE_BYTES,
                MAX_STREAM_BYTES,
                MAX_CONCURRENT_REQUESTS,
                REQUEST_DEADLINE,
                STREAM_DEADLINE,
                Duration::MAX,
            ),
            LimitField::ShutdownDeadline,
        ),
    ];

    for (result, field) in excessive {
        assert!(matches!(
            result,
            Err(InvalidLimits::ExceedsGlobalCeiling {
                field: actual,
                ..
            }) if actual == field
        ));
    }
}

#[test]
fn constructor_rejects_deadlines_that_serialize_to_zero() {
    for (request, stream, shutdown, field) in [
        (
            Duration::from_nanos(1),
            STREAM_DEADLINE,
            SHUTDOWN_DEADLINE,
            LimitField::RequestDeadline,
        ),
        (
            REQUEST_DEADLINE,
            Duration::from_micros(999),
            SHUTDOWN_DEADLINE,
            LimitField::StreamDeadline,
        ),
        (
            REQUEST_DEADLINE,
            STREAM_DEADLINE,
            Duration::from_nanos(999_999),
            LimitField::ShutdownDeadline,
        ),
    ] {
        assert_eq!(
            ProtocolLimits::new(
                MAX_MESSAGE_BYTES,
                MAX_STREAM_BYTES,
                MAX_CONCURRENT_REQUESTS,
                request,
                stream,
                shutdown,
            ),
            Err(InvalidLimits::BelowWireResolution { field })
        );
    }
}
