//! Public authentication-boundary tests.

use fm_semantic_protocol::{AuthenticationError, Authenticator, SessionToken};

#[test]
fn missing_and_wrong_session_tokens_are_rejected() {
    let expected = SessionToken::new(b"worker-secret".to_vec()).unwrap();
    let wrong = SessionToken::new(b"wrong-secret".to_vec()).unwrap();
    let authenticator = Authenticator::new(expected.clone());

    assert_eq!(
        authenticator.authenticate(None),
        Err(AuthenticationError::Missing)
    );
    assert_eq!(
        authenticator.authenticate(Some(&wrong)),
        Err(AuthenticationError::Rejected)
    );
    assert_eq!(authenticator.authenticate(Some(&expected)), Ok(()));
    assert_eq!(format!("{expected:?}"), "SessionToken([REDACTED])");
}
