//! Public resource-limit tests.

use std::time::Duration;

use fm_semantic_protocol::{
    ConcurrencyLimiter, DeadlineKind, InvalidLimits, LimitError, LimitField,
    MAX_CONCURRENT_REQUESTS, MAX_MESSAGE_BYTES, MAX_STREAM_BYTES, NegotiatedLimitsError,
    ProtocolLimits, REQUEST_DEADLINE, SHUTDOWN_DEADLINE, STREAM_DEADLINE, StreamBudget, v1,
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
fn rejects_negotiated_limits_above_immutable_client_ceilings() {
    let defaults = v1::ProtocolLimits {
        maximum_message_bytes: MAX_MESSAGE_BYTES as u64,
        maximum_stream_bytes: MAX_STREAM_BYTES,
        maximum_concurrent_requests: MAX_CONCURRENT_REQUESTS as u32,
        request_deadline_ms: REQUEST_DEADLINE.as_millis() as u64,
        stream_deadline_ms: STREAM_DEADLINE.as_millis() as u64,
        shutdown_deadline_ms: SHUTDOWN_DEADLINE.as_millis() as u64,
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
