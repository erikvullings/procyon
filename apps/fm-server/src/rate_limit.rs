//! Rate limiting for mutating endpoints (task 0064).
//!
//! Read-only requests (`GET`/`HEAD`) are never throttled; every other method
//! shares one token-bucket limiter server-wide, protecting against a
//! runaway or malicious client hammering destructive/mutating routes. A
//! handful of `POST` routes are read-only in effect (see
//! [`READ_ONLY_POST_PATHS`]) and are exempted the same way.

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};

/// A single, server-wide token-bucket limiter for mutating requests.
pub(crate) type MutationLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

/// Builds a limiter allowing `requests_per_second` mutating requests per
/// second, with a burst equal to that same rate.
pub(crate) fn build_limiter(requests_per_second: u32) -> Arc<MutationLimiter> {
    let per_second = NonZeroU32::new(requests_per_second.max(1)).unwrap_or(NonZeroU32::MIN);
    Arc::new(RateLimiter::direct(Quota::per_second(per_second)))
}

/// `POST` routes that only read data - `POST` is used solely to carry a JSON request body, not
/// because the operation mutates anything - so they're exempt from the mutation limiter. A
/// directory full of thumbnailable files can legitimately fire many `files/range` reads at once.
const READ_ONLY_POST_PATHS: &[&str] = &[
    "/api/v1/files/range",
    "/api/v1/files/search",
    "/api/v1/directories/size",
];

/// Middleware that rejects mutating requests (`POST`/`PUT`/`PATCH`/`DELETE`)
/// with `429 Too Many Requests` once the shared limiter's quota is
/// exhausted. `GET`/`HEAD`/`OPTIONS` always pass through unthrottled.
pub(crate) async fn limit_mutations(
    State(limiter): State<Arc<MutationLimiter>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let is_mutating = matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) && !READ_ONLY_POST_PATHS.contains(&request.uri().path());

    if is_mutating && limiter.check().is_err() {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_allows_requests_within_quota() {
        let limiter = build_limiter(10);
        assert!(limiter.check().is_ok());
    }

    #[test]
    fn limiter_rejects_requests_beyond_burst() {
        let limiter = build_limiter(1);
        assert!(limiter.check().is_ok());
        assert!(limiter.check().is_err());
    }
}
