//! Versioned protobuf contract for communication with an isolated semantic worker.
//!
//! The public API contains protocol DTOs and boundary validation only. It does
//! not expose filesystem paths, credentials, action registries, or networking.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use prost::Message;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Maximum encoded size of one protobuf message (one mebibyte).
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// Maximum cumulative file-content or result stream size (64 mebibytes).
pub const MAX_STREAM_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum number of requests handled concurrently by one worker.
pub const MAX_CONCURRENT_REQUESTS: usize = 16;
/// Maximum duration of a unary request.
pub const REQUEST_DEADLINE: Duration = Duration::from_secs(30);
/// Maximum duration of an ingestion, result, or event stream.
pub const STREAM_DEADLINE: Duration = Duration::from_secs(5 * 60);
/// Maximum duration allowed for graceful shutdown.
pub const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(15);

/// Protocol version one protobuf DTOs.
#[allow(missing_docs)]
pub mod v1 {
    include!(concat!(env!("OUT_DIR"), "/procyon.semantic.v1.rs"));
}

/// Failure while reading or writing one length-delimited protobuf frame.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// The transport ended or failed.
    #[error("local transport I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// The declared or encoded frame is larger than policy permits.
    #[error("frame is {actual} bytes; maximum is {maximum}")]
    TooLarge {
        /// Declared or encoded frame size.
        actual: usize,
        /// Configured maximum frame size.
        maximum: usize,
    },
    /// The payload was not valid protobuf.
    #[error("invalid protobuf frame: {0}")]
    Decode(#[from] prost::DecodeError),
}

/// Reads one big-endian length-delimited protobuf frame.
///
/// The declared size is checked before allocating the payload buffer.
///
/// # Errors
///
/// Returns a typed transport, size, or protobuf decoding failure.
pub async fn read_frame<R, M>(reader: &mut R, maximum: usize) -> Result<M, FrameError>
where
    R: AsyncRead + Unpin,
    M: Message + Default,
{
    let declared = reader.read_u32().await?;
    let size = usize::try_from(declared).unwrap_or(usize::MAX);
    if size > maximum {
        return Err(FrameError::TooLarge {
            actual: size,
            maximum,
        });
    }
    let mut payload = vec![0; size];
    reader.read_exact(&mut payload).await?;
    Ok(M::decode(payload.as_slice())?)
}

/// Writes one big-endian length-delimited protobuf frame.
///
/// # Errors
///
/// Returns a typed transport or size failure.
pub async fn write_frame<W, M>(
    writer: &mut W,
    message: &M,
    maximum: usize,
) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
    M: Message,
{
    let size = message.encoded_len();
    if size > maximum || size > u32::MAX as usize {
        return Err(FrameError::TooLarge {
            actual: size,
            maximum,
        });
    }
    writer
        .write_u32(u32::try_from(size).unwrap_or(u32::MAX))
        .await?;
    let payload = message.encode_to_vec();
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

/// A semantic-worker protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion(u32);

impl ProtocolVersion {
    /// Creates a protocol version from its wire value.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the wire value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// An inclusive range of protocol versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionRange {
    minimum: ProtocolVersion,
    maximum: ProtocolVersion,
}

impl VersionRange {
    /// Creates a range containing exactly one version.
    #[must_use]
    pub const fn exact(version: ProtocolVersion) -> Self {
        Self {
            minimum: version,
            maximum: version,
        }
    }

    /// Creates an ordered version range.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidVersionRange`] when `minimum` exceeds `maximum`.
    pub fn new(
        minimum: ProtocolVersion,
        maximum: ProtocolVersion,
    ) -> Result<Self, InvalidVersionRange> {
        if minimum > maximum {
            return Err(InvalidVersionRange { minimum, maximum });
        }
        Ok(Self { minimum, maximum })
    }

    /// Returns the oldest accepted version.
    #[must_use]
    pub const fn minimum(self) -> ProtocolVersion {
        self.minimum
    }

    /// Returns the newest accepted version.
    #[must_use]
    pub const fn maximum(self) -> ProtocolVersion {
        self.maximum
    }
}

/// An unordered version range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "minimum protocol version {} exceeds maximum version {}",
    minimum.get(),
    maximum.get()
)]
pub struct InvalidVersionRange {
    minimum: ProtocolVersion,
    maximum: ProtocolVersion,
}

/// An actionable protocol compatibility failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CompatibilityError {
    /// The caller is older than every worker-supported version.
    #[error(
        "client update required: worker requires protocol version {minimum_supported} or newer",
        minimum_supported = .minimum_supported.get()
    )]
    ClientUpdateRequired {
        /// Oldest version accepted by the worker.
        minimum_supported: ProtocolVersion,
    },
    /// The worker is older than every caller-supported version.
    #[error(
        "worker update required: worker only supports protocol version {maximum_supported} or older",
        maximum_supported = .maximum_supported.get()
    )]
    WorkerUpdateRequired {
        /// Newest version accepted by the worker.
        maximum_supported: ProtocolVersion,
        /// Oldest version required by the caller.
        minimum_required: ProtocolVersion,
    },
}

impl CompatibilityError {
    /// Converts the compatibility failure to its stable protobuf error form.
    #[must_use]
    pub fn to_wire(self) -> v1::ProtocolError {
        let (code, required_protocol_version) = match self {
            Self::ClientUpdateRequired { minimum_supported } => {
                (v1::ErrorCode::ClientUpdateRequired, minimum_supported.get())
            }
            Self::WorkerUpdateRequired {
                minimum_required, ..
            } => (v1::ErrorCode::WorkerUpdateRequired, minimum_required.get()),
        };
        v1::ProtocolError {
            code: code.into(),
            message: self.to_string(),
            retryable: false,
            required_protocol_version,
        }
    }
}

/// Selects the highest mutually supported protocol version.
///
/// # Errors
///
/// Returns an actionable update requirement when the ranges do not overlap.
pub fn negotiate_version(
    caller: VersionRange,
    worker: VersionRange,
) -> Result<ProtocolVersion, CompatibilityError> {
    if caller.maximum < worker.minimum {
        return Err(CompatibilityError::ClientUpdateRequired {
            minimum_supported: worker.minimum,
        });
    }
    if caller.minimum > worker.maximum {
        return Err(CompatibilityError::WorkerUpdateRequired {
            maximum_supported: worker.maximum,
            minimum_required: caller.minimum,
        });
    }
    Ok(caller.maximum.min(worker.maximum))
}

/// Capability, version, and resource-policy negotiator.
#[derive(Debug, Clone)]
pub struct Negotiator {
    versions: VersionRange,
    capabilities: BTreeSet<v1::Capability>,
    limits: ProtocolLimits,
}

impl Negotiator {
    /// Creates a worker negotiator.
    pub fn new(
        versions: VersionRange,
        capabilities: impl IntoIterator<Item = v1::Capability>,
        limits: ProtocolLimits,
    ) -> Self {
        Self {
            versions,
            capabilities: capabilities
                .into_iter()
                .filter(|capability| *capability != v1::Capability::Unspecified)
                .collect(),
            limits,
        }
    }

    /// Selects a rolling-compatible version and requested worker capabilities.
    ///
    /// # Errors
    ///
    /// Returns [`NegotiationError::InvalidVersionRange`] for an unordered
    /// caller range or [`NegotiationError::Compatibility`] when an update is
    /// required.
    pub fn negotiate(
        &self,
        request: &v1::NegotiateRequest,
    ) -> Result<v1::NegotiateResponse, NegotiationError> {
        let caller = VersionRange::new(
            ProtocolVersion::new(request.minimum_version),
            ProtocolVersion::new(request.maximum_version),
        )?;
        let selected_version = negotiate_version(caller, self.versions)?;
        let capabilities = request
            .requested_capabilities
            .iter()
            .filter_map(|value| v1::Capability::try_from(*value).ok())
            .filter(|capability| self.capabilities.contains(capability))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(i32::from)
            .collect();

        Ok(v1::NegotiateResponse {
            selected_version: selected_version.get(),
            capabilities,
            limits: Some(self.limits.to_wire()),
        })
    }
}

/// Failure to negotiate a worker session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NegotiationError {
    /// The caller supplied an unordered version range.
    #[error(transparent)]
    InvalidVersionRange(#[from] InvalidVersionRange),
    /// Caller and worker versions do not overlap.
    #[error(transparent)]
    Compatibility(#[from] CompatibilityError),
}

/// An opaque authentication token used only at the worker boundary.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionToken(Vec<u8>);

impl SessionToken {
    /// Wraps a non-empty opaque token.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidSessionToken`] when the token is empty.
    pub fn new(value: Vec<u8>) -> Result<Self, InvalidSessionToken> {
        if value.is_empty() {
            return Err(InvalidSessionToken);
        }
        Ok(Self(value))
    }

    /// Returns the token bytes for protobuf transport.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for SessionToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionToken([REDACTED])")
    }
}

/// An empty authentication token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("session token must not be empty")]
pub struct InvalidSessionToken;

/// Session authentication failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthenticationError {
    /// No token was supplied.
    #[error("authentication token is required")]
    Missing,
    /// The supplied token was not accepted.
    #[error("authentication token was rejected")]
    Rejected,
}

/// Verifies session authentication without exposing the expected secret.
pub struct Authenticator {
    expected: SessionToken,
}

impl Authenticator {
    /// Creates an authenticator for a provisioned token.
    #[must_use]
    pub fn new(expected: SessionToken) -> Self {
        Self { expected }
    }

    /// Checks that a request carries the provisioned token.
    ///
    /// # Errors
    ///
    /// Returns [`AuthenticationError::Missing`] or
    /// [`AuthenticationError::Rejected`] without disclosing token contents.
    pub fn authenticate(
        &self,
        presented: Option<&SessionToken>,
    ) -> Result<(), AuthenticationError> {
        let presented = presented.ok_or(AuthenticationError::Missing)?;
        if constant_time_eq(self.expected.as_bytes(), presented.as_bytes()) {
            Ok(())
        } else {
            Err(AuthenticationError::Rejected)
        }
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

/// Class of operation governed by a protocol deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadlineKind {
    /// Unary request.
    Request,
    /// File-content, result, or event stream.
    Stream,
    /// Graceful worker shutdown.
    Shutdown,
}

/// One independently negotiated resource limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitField {
    /// Encoded protobuf message bytes.
    MessageBytes,
    /// Cumulative stream bytes.
    StreamBytes,
    /// Concurrent request count.
    ConcurrentRequests,
    /// Unary request duration.
    RequestDeadline,
    /// Stream duration.
    StreamDeadline,
    /// Graceful shutdown duration.
    ShutdownDeadline,
}

/// Invalid or unsafe limits advertised by an untrusted worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NegotiatedLimitsError {
    /// A required limit was zero.
    #[error("negotiated {field:?} limit must be greater than zero")]
    Zero {
        /// Invalid limit.
        field: LimitField,
    },
    /// A worker-advertised limit exceeded the immutable client ceiling.
    #[error("negotiated {field:?} limit {offered} exceeds immutable client ceiling {maximum}")]
    ExceedsClientCeiling {
        /// Unsafe limit.
        field: LimitField,
        /// Worker-advertised value.
        offered: u64,
        /// Immutable client ceiling.
        maximum: u64,
    },
    /// The message limit could not fit inside the stream limit.
    #[error("negotiated message limit exceeds negotiated stream limit")]
    MessageExceedsStream,
}

/// Explicit resource policy applied at the protocol boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolLimits {
    max_message_bytes: usize,
    max_stream_bytes: u64,
    max_concurrent_requests: usize,
    request_deadline: Duration,
    stream_deadline: Duration,
    shutdown_deadline: Duration,
}

impl Default for ProtocolLimits {
    fn default() -> Self {
        Self {
            max_message_bytes: MAX_MESSAGE_BYTES,
            max_stream_bytes: MAX_STREAM_BYTES,
            max_concurrent_requests: MAX_CONCURRENT_REQUESTS,
            request_deadline: REQUEST_DEADLINE,
            stream_deadline: STREAM_DEADLINE,
            shutdown_deadline: SHUTDOWN_DEADLINE,
        }
    }
}

impl ProtocolLimits {
    /// Validates untrusted worker-advertised limits against immutable client
    /// ceilings before they can affect allocation or timeout behavior.
    ///
    /// # Errors
    ///
    /// Returns a typed error for a zero, internally inconsistent, or
    /// over-ceiling value.
    pub fn from_negotiated(limits: &v1::ProtocolLimits) -> Result<Self, NegotiatedLimitsError> {
        fn bounded(
            field: LimitField,
            offered: u64,
            maximum: u64,
        ) -> Result<u64, NegotiatedLimitsError> {
            if offered == 0 {
                return Err(NegotiatedLimitsError::Zero { field });
            }
            if offered > maximum {
                return Err(NegotiatedLimitsError::ExceedsClientCeiling {
                    field,
                    offered,
                    maximum,
                });
            }
            Ok(offered)
        }

        let maximum_message_bytes = bounded(
            LimitField::MessageBytes,
            limits.maximum_message_bytes,
            MAX_MESSAGE_BYTES as u64,
        )?;
        let maximum_stream_bytes = bounded(
            LimitField::StreamBytes,
            limits.maximum_stream_bytes,
            MAX_STREAM_BYTES,
        )?;
        let maximum_concurrent_requests = bounded(
            LimitField::ConcurrentRequests,
            u64::from(limits.maximum_concurrent_requests),
            MAX_CONCURRENT_REQUESTS as u64,
        )?;
        let request_deadline_ms = bounded(
            LimitField::RequestDeadline,
            limits.request_deadline_ms,
            duration_millis(REQUEST_DEADLINE),
        )?;
        let stream_deadline_ms = bounded(
            LimitField::StreamDeadline,
            limits.stream_deadline_ms,
            duration_millis(STREAM_DEADLINE),
        )?;
        let shutdown_deadline_ms = bounded(
            LimitField::ShutdownDeadline,
            limits.shutdown_deadline_ms,
            duration_millis(SHUTDOWN_DEADLINE),
        )?;

        if maximum_message_bytes > maximum_stream_bytes {
            return Err(NegotiatedLimitsError::MessageExceedsStream);
        }
        Ok(Self {
            max_message_bytes: usize::try_from(maximum_message_bytes).map_err(|_| {
                NegotiatedLimitsError::ExceedsClientCeiling {
                    field: LimitField::MessageBytes,
                    offered: maximum_message_bytes,
                    maximum: MAX_MESSAGE_BYTES as u64,
                }
            })?,
            max_stream_bytes: maximum_stream_bytes,
            max_concurrent_requests: usize::try_from(maximum_concurrent_requests).map_err(
                |_| NegotiatedLimitsError::ExceedsClientCeiling {
                    field: LimitField::ConcurrentRequests,
                    offered: maximum_concurrent_requests,
                    maximum: MAX_CONCURRENT_REQUESTS as u64,
                },
            )?,
            request_deadline: Duration::from_millis(request_deadline_ms),
            stream_deadline: Duration::from_millis(stream_deadline_ms),
            shutdown_deadline: Duration::from_millis(shutdown_deadline_ms),
        })
    }

    /// Creates a validated resource policy.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidLimits`] if a limit is zero, cannot be represented on
    /// the wire, exceeds its immutable global ceiling, or a message can exceed
    /// its containing stream.
    pub fn new(
        max_message_bytes: usize,
        max_stream_bytes: u64,
        max_concurrent_requests: usize,
        request_deadline: Duration,
        stream_deadline: Duration,
        shutdown_deadline: Duration,
    ) -> Result<Self, InvalidLimits> {
        fn bounded(field: LimitField, offered: u64, maximum: u64) -> Result<(), InvalidLimits> {
            if offered == 0 {
                return Err(InvalidLimits::Zero { field });
            }
            if offered > maximum {
                return Err(InvalidLimits::ExceedsGlobalCeiling {
                    field,
                    offered,
                    maximum,
                });
            }
            Ok(())
        }

        fn bounded_duration(
            field: LimitField,
            offered: Duration,
            maximum: Duration,
        ) -> Result<(), InvalidLimits> {
            if offered.is_zero() {
                return Err(InvalidLimits::Zero { field });
            }
            if offered.as_millis() == 0 {
                return Err(InvalidLimits::BelowWireResolution { field });
            }
            if offered > maximum {
                return Err(InvalidLimits::ExceedsGlobalCeiling {
                    field,
                    offered: duration_millis(offered),
                    maximum: duration_millis(maximum),
                });
            }
            Ok(())
        }

        let message_bytes =
            u64::try_from(max_message_bytes).map_err(|_| InvalidLimits::ExceedsGlobalCeiling {
                field: LimitField::MessageBytes,
                offered: u64::MAX,
                maximum: MAX_MESSAGE_BYTES as u64,
            })?;
        let concurrent_requests = u64::try_from(max_concurrent_requests).map_err(|_| {
            InvalidLimits::ExceedsGlobalCeiling {
                field: LimitField::ConcurrentRequests,
                offered: u64::MAX,
                maximum: MAX_CONCURRENT_REQUESTS as u64,
            }
        })?;
        bounded(
            LimitField::MessageBytes,
            message_bytes,
            MAX_MESSAGE_BYTES as u64,
        )?;
        bounded(LimitField::StreamBytes, max_stream_bytes, MAX_STREAM_BYTES)?;
        bounded(
            LimitField::ConcurrentRequests,
            concurrent_requests,
            MAX_CONCURRENT_REQUESTS as u64,
        )?;
        bounded_duration(
            LimitField::RequestDeadline,
            request_deadline,
            REQUEST_DEADLINE,
        )?;
        bounded_duration(LimitField::StreamDeadline, stream_deadline, STREAM_DEADLINE)?;
        bounded_duration(
            LimitField::ShutdownDeadline,
            shutdown_deadline,
            SHUTDOWN_DEADLINE,
        )?;
        if message_bytes > max_stream_bytes {
            return Err(InvalidLimits::MessageExceedsStream);
        }
        Ok(Self {
            max_message_bytes,
            max_stream_bytes,
            max_concurrent_requests,
            request_deadline,
            stream_deadline,
            shutdown_deadline,
        })
    }

    /// Maximum encoded protobuf message size.
    #[must_use]
    pub const fn max_message_bytes(self) -> usize {
        self.max_message_bytes
    }

    /// Maximum cumulative byte count of one stream.
    #[must_use]
    pub const fn max_stream_bytes(self) -> u64 {
        self.max_stream_bytes
    }

    /// Maximum number of concurrent requests.
    #[must_use]
    pub const fn max_concurrent_requests(self) -> usize {
        self.max_concurrent_requests
    }

    /// Maximum duration for an operation class.
    #[must_use]
    pub const fn deadline(self, kind: DeadlineKind) -> Duration {
        match kind {
            DeadlineKind::Request => self.request_deadline,
            DeadlineKind::Stream => self.stream_deadline,
            DeadlineKind::Shutdown => self.shutdown_deadline,
        }
    }

    fn to_wire(self) -> v1::ProtocolLimits {
        v1::ProtocolLimits {
            maximum_message_bytes: u64::try_from(self.max_message_bytes).unwrap_or(u64::MAX),
            maximum_stream_bytes: self.max_stream_bytes,
            maximum_concurrent_requests: u32::try_from(self.max_concurrent_requests)
                .unwrap_or(u32::MAX),
            request_deadline_ms: duration_millis(self.request_deadline),
            stream_deadline_ms: duration_millis(self.stream_deadline),
            shutdown_deadline_ms: duration_millis(self.shutdown_deadline),
        }
    }

    /// Verifies an encoded message size.
    ///
    /// # Errors
    ///
    /// Returns [`LimitError::MessageTooLarge`] when `actual` exceeds the policy.
    pub fn check_message_bytes(self, actual: usize) -> Result<(), LimitError> {
        if actual > self.max_message_bytes {
            return Err(LimitError::MessageTooLarge {
                actual,
                maximum: self.max_message_bytes,
            });
        }
        Ok(())
    }

    /// Verifies that a requested deadline is non-zero and within the policy.
    ///
    /// # Errors
    ///
    /// Returns a deadline error when `requested` is zero or exceeds the policy.
    pub fn check_deadline(self, kind: DeadlineKind, requested: Duration) -> Result<(), LimitError> {
        if requested.is_zero() {
            return Err(LimitError::ZeroDeadline);
        }
        let maximum = self.deadline(kind);
        if requested > maximum {
            return Err(LimitError::DeadlineTooLong { requested, maximum });
        }
        Ok(())
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Invalid protocol resource policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidLimits {
    /// A required limit was zero.
    #[error("{field:?} limit must be greater than zero")]
    Zero {
        /// Invalid limit.
        field: LimitField,
    },
    /// A non-zero duration was shorter than the protocol's millisecond wire
    /// resolution.
    #[error("{field:?} limit must be at least one millisecond")]
    BelowWireResolution {
        /// Invalid duration limit.
        field: LimitField,
    },
    /// A configured value exceeded the immutable process-wide ceiling.
    #[error("{field:?} limit {offered} exceeds immutable global ceiling {maximum}")]
    ExceedsGlobalCeiling {
        /// Unsafe limit.
        field: LimitField,
        /// Configured value, in bytes, requests, or milliseconds.
        offered: u64,
        /// Immutable ceiling in the same unit.
        maximum: u64,
    },
    /// The message limit could not fit inside the stream limit.
    #[error("message limit exceeds stream limit")]
    MessageExceedsStream,
}

/// A protocol resource limit violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LimitError {
    /// One encoded message exceeds its limit.
    #[error("message is {actual} bytes; maximum is {maximum}")]
    MessageTooLarge {
        /// Actual encoded size.
        actual: usize,
        /// Configured maximum size.
        maximum: usize,
    },
    /// A stream would exceed its cumulative limit.
    #[error("stream would reach {attempted} bytes; maximum is {maximum}")]
    StreamTooLarge {
        /// Cumulative size after the attempted message.
        attempted: u64,
        /// Configured maximum size.
        maximum: u64,
    },
    /// The worker is at its concurrent-request limit.
    #[error("worker already has the maximum {maximum} concurrent requests")]
    TooManyConcurrentRequests {
        /// Configured maximum concurrent requests.
        maximum: usize,
    },
    /// A requested deadline was zero.
    #[error("deadline must be greater than zero")]
    ZeroDeadline,
    /// A requested deadline exceeds the configured maximum.
    #[error("requested deadline {requested:?} exceeds maximum {maximum:?}")]
    DeadlineTooLong {
        /// Requested duration.
        requested: Duration,
        /// Configured maximum duration.
        maximum: Duration,
    },
    /// A byte count overflowed its representation.
    #[error("stream byte count overflowed")]
    ByteCountOverflow,
}

/// Tracks the cumulative bytes accepted for one bounded stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamBudget {
    consumed: u64,
    maximum: u64,
}

impl StreamBudget {
    /// Creates an unused stream budget.
    #[must_use]
    pub const fn new(maximum: u64) -> Self {
        Self {
            consumed: 0,
            maximum,
        }
    }

    /// Charges bytes to the stream without advancing on failure.
    ///
    /// # Errors
    ///
    /// Returns [`LimitError::StreamTooLarge`] or
    /// [`LimitError::ByteCountOverflow`] if the bytes cannot be accepted.
    pub fn consume(&mut self, bytes: u64) -> Result<(), LimitError> {
        let attempted = self
            .consumed
            .checked_add(bytes)
            .ok_or(LimitError::ByteCountOverflow)?;
        if attempted > self.maximum {
            return Err(LimitError::StreamTooLarge {
                attempted,
                maximum: self.maximum,
            });
        }
        self.consumed = attempted;
        Ok(())
    }

    /// Returns the accepted cumulative byte count.
    #[must_use]
    pub const fn consumed(self) -> u64 {
        self.consumed
    }
}

/// Lock-free concurrent-request limiter.
#[derive(Debug)]
pub struct ConcurrencyLimiter {
    active: AtomicUsize,
    maximum: usize,
}

impl ConcurrencyLimiter {
    /// Creates a limiter with a non-zero maximum.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidLimits`] if `maximum` is zero.
    pub fn new(maximum: usize) -> Result<Self, InvalidLimits> {
        if maximum == 0 {
            return Err(InvalidLimits::Zero {
                field: LimitField::ConcurrentRequests,
            });
        }
        Ok(Self {
            active: AtomicUsize::new(0),
            maximum,
        })
    }

    /// Acquires a request slot until the returned permit is dropped.
    ///
    /// # Errors
    ///
    /// Returns [`LimitError::TooManyConcurrentRequests`] when full.
    pub fn try_acquire(&self) -> Result<ConcurrencyPermit<'_>, LimitError> {
        self.active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < self.maximum).then_some(active + 1)
            })
            .map_err(|_| LimitError::TooManyConcurrentRequests {
                maximum: self.maximum,
            })?;
        Ok(ConcurrencyPermit { limiter: self })
    }
}

/// A held concurrent-request slot, released when dropped.
#[derive(Debug)]
pub struct ConcurrencyPermit<'a> {
    limiter: &'a ConcurrencyLimiter,
}

impl Drop for ConcurrencyPermit<'_> {
    fn drop(&mut self) {
        self.limiter.active.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Invalid required data in a scoped protocol request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RequestValidationError {
    /// Authenticated session context is absent or incomplete.
    #[error("authenticated session context is required")]
    MissingSession,
    /// Tenant/library scope is absent or incomplete.
    #[error("tenant and library scope is required")]
    MissingScope,
    /// Request identifier is empty.
    #[error("request identifier is required")]
    MissingRequestId,
    /// Ingestion payload is absent.
    #[error("ingestion payload is required")]
    MissingPayload,
    /// Ingestion document identifier is empty.
    #[error("document identifier is required")]
    MissingDocumentId,
    /// Declared content exceeds the configured stream limit.
    #[error("declared content size exceeds the configured stream limit")]
    DeclaredContentTooLarge,
    /// A content chunk exceeds the configured message limit.
    #[error("content chunk exceeds the configured message limit")]
    ChunkTooLarge,
    /// Query text is empty.
    #[error("query text is required")]
    EmptyQuery,
    /// Query result limit is zero.
    #[error("maximum query results must be greater than zero")]
    InvalidMaximumResults,
}

/// Validates a v1 ingestion item against the default resource policy.
///
/// # Errors
///
/// Returns a typed validation failure for absent authority, identity, payload,
/// or content that cannot fit within the protocol limits.
pub fn validate_ingestion(request: &v1::IngestionRequest) -> Result<(), RequestValidationError> {
    validate_ingestion_with_limits(request, ProtocolLimits::default())
}

/// Validates a v1 ingestion item against an explicit resource policy.
///
/// # Errors
///
/// Returns a typed validation failure for absent authority, identity, payload,
/// or content that cannot fit within `limits`.
pub fn validate_ingestion_with_limits(
    request: &v1::IngestionRequest,
    limits: ProtocolLimits,
) -> Result<(), RequestValidationError> {
    validate_session(request.session.as_ref())?;
    validate_scope(request.scope.as_ref())?;
    if request.request_id.is_empty() {
        return Err(RequestValidationError::MissingRequestId);
    }
    match request.payload.as_ref() {
        Some(v1::ingestion_request::Payload::Start(start)) => {
            if start.document_id.is_empty() {
                return Err(RequestValidationError::MissingDocumentId);
            }
            if start.expected_content_bytes > limits.max_stream_bytes() {
                return Err(RequestValidationError::DeclaredContentTooLarge);
            }
        }
        Some(v1::ingestion_request::Payload::Chunk(chunk)) => {
            if chunk.content.len() > limits.max_message_bytes() {
                return Err(RequestValidationError::ChunkTooLarge);
            }
        }
        None => return Err(RequestValidationError::MissingPayload),
    }
    Ok(())
}

/// Validates a v1 semantic query.
///
/// # Errors
///
/// Returns a typed validation failure when authentication, tenant/library
/// scope, request identity, query text, or result bound is absent.
pub fn validate_query(request: &v1::QueryRequest) -> Result<(), RequestValidationError> {
    validate_session(request.session.as_ref())?;
    validate_scope(request.scope.as_ref())?;
    if request.request_id.is_empty() {
        return Err(RequestValidationError::MissingRequestId);
    }
    if request.query.is_empty() {
        return Err(RequestValidationError::EmptyQuery);
    }
    if request.maximum_results == 0 {
        return Err(RequestValidationError::InvalidMaximumResults);
    }
    Ok(())
}

fn validate_session(session: Option<&v1::SessionContext>) -> Result<(), RequestValidationError> {
    if session
        .is_none_or(|session| session.session_id.is_empty() || session.session_token.is_empty())
    {
        return Err(RequestValidationError::MissingSession);
    }
    Ok(())
}

fn validate_scope(scope: Option<&v1::ResourceScope>) -> Result<(), RequestValidationError> {
    if scope.is_none_or(|scope| scope.tenant_id.is_empty() || scope.library_id.is_empty()) {
        return Err(RequestValidationError::MissingScope);
    }
    Ok(())
}
