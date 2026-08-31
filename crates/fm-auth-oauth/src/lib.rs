//! OAuth 2.0 Authorization Code + PKCE for a public desktop client (task
//! 0110, spec §5.3 "native OneDrive provider").
//!
//! This crate owns exactly the token-shaped protocol work: generating
//! `state`/PKCE material, building the Microsoft identity platform
//! authorization URL, running a system-browser-compatible loopback callback
//! listener (RFC 8252 §7.3), and exchanging an authorization code or
//! refresh token for tokens with no `client_secret` (this is a public
//! client - PKCE replaces the secret, spec §19 "no client secret in a
//! desktop app").
//!
//! What it deliberately does **not** do:
//!
//! * Open a system browser. `fm-application` orchestrates that (this crate
//!   must not depend on any OS/browser-launching crate - spec §3 core
//!   engine crates stay platform/host-agnostic).
//! * Persist tokens. [`token::TokenResponse`] hands back the data needed for
//!   an atomic refresh-token replacement (Microsoft identity platform
//!   rotates refresh tokens on every use); `fm-application` owns writing
//!   that into a `CredentialStore` (task 0103).
//!
//! # Typical flow
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use fm_auth_oauth::authorization::build_authorization_request;
//! use fm_auth_oauth::callback::CallbackListener;
//! use fm_auth_oauth::config::PublicClientConfig;
//! use fm_auth_oauth::pkce::generate_pkce_pair;
//! use fm_auth_oauth::token::exchange_authorization_code;
//! use tokio_util::sync::CancellationToken;
//!
//! # async fn run() -> Result<(), fm_auth_oauth::error::OAuthError> {
//! let config = PublicClientConfig::microsoft_common("9b01b729-5908-492b-bcd1-32b4a36096de");
//! let listener = CallbackListener::bind().await?;
//! let redirect_uri = listener.redirect_uri().clone();
//! let pkce = generate_pkce_pair();
//! let request = build_authorization_request(&config, &redirect_uri, &pkce.challenge);
//!
//! // `fm-application` opens `request.url` in the system browser here.
//!
//! let code = listener
//!     .listen(&request.state, Duration::from_secs(300), CancellationToken::new())
//!     .await?;
//! let http = reqwest::Client::new();
//! let tokens =
//!     exchange_authorization_code(&http, &config, &redirect_uri, &code, &pkce.verifier).await?;
//! # let _ = tokens;
//! # Ok(())
//! # }
//! ```

pub mod authorization;
pub mod callback;
pub mod claims;
pub mod config;
pub mod error;
pub mod fixture;
pub mod pkce;
pub mod token;
