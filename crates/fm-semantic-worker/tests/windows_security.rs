//! Platform-independent coverage for the Windows named-pipe security policy.

use fm_semantic_worker::{PipeSecurityError, is_local_named_pipe_endpoint, owner_only_pipe_sddl};

#[test]
fn owner_only_pipe_descriptor_contains_only_the_current_user_sid() {
    let sid = "S-1-5-21-1000-2000-3000-4000";

    assert_eq!(
        owner_only_pipe_sddl(sid),
        Ok(format!("O:{sid}G:{sid}D:P(A;;GA;;;{sid})"))
    );
    assert_eq!(
        owner_only_pipe_sddl("S-1-5-21-1)D:(A;;GA;;;WD"),
        Err(PipeSecurityError::InvalidUserSid)
    );
}

#[test]
fn only_local_named_pipe_endpoints_are_eligible_for_authentication() {
    assert!(is_local_named_pipe_endpoint(
        r"\\.\pipe\procyon-semantic-1234"
    ));
    assert!(!is_local_named_pipe_endpoint(
        r"\\server\pipe\procyon-semantic-1234"
    ));
    assert!(!is_local_named_pipe_endpoint("127.0.0.1:8787"));
}

#[cfg(windows)]
#[tokio::test]
async fn client_rejects_an_unprotected_pipe_before_sending_authentication_material() {
    use std::time::Duration;

    use fm_semantic_protocol::{MAX_MESSAGE_BYTES, read_frame, v1};
    use fm_semantic_worker::{ClientError, Endpoint, LaunchSecret, WorkerClient};
    use interprocess::local_socket::traits::tokio::Listener as _;
    use interprocess::local_socket::{ListenerOptions, ToFsName};
    use interprocess::os::windows::local_socket::{ListenerOptionsExt, NamedPipe};
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use widestring::U16CString;

    let pipe_path = format!(
        r"\\.\pipe\procyon-semantic-insecure-test-{}",
        std::process::id()
    );
    let name = pipe_path.as_str().to_fs_name::<NamedPipe>().unwrap();
    let descriptor =
        SecurityDescriptor::deserialize(&U16CString::from_str("D:P(A;;GA;;;WD)").unwrap()).unwrap();
    let listener = ListenerOptions::new()
        .name(name)
        .security_descriptor(descriptor)
        .create_tokio()
        .unwrap();
    let endpoint = Endpoint::Windows(pipe_path);

    let (connection, accepted) = tokio::join!(
        WorkerClient::connect(&endpoint, LaunchSecret::from_bytes([1; 32])),
        listener.accept()
    );
    assert!(matches!(connection, Err(ClientError::InsecureEndpoint)));
    let mut accepted = accepted.unwrap();
    let received = tokio::time::timeout(
        Duration::from_millis(100),
        read_frame::<_, v1::ClientFrame>(&mut accepted, MAX_MESSAGE_BYTES),
    )
    .await;
    assert!(
        !matches!(received, Ok(Ok(_))),
        "authentication material reached an unprotected pipe"
    );
}
