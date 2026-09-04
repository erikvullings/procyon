//! Public multiplexed transport framing tests.

use fm_semantic_protocol::v1;
use prost::Message;

#[test]
fn multiplexed_frames_preserve_correlation_and_stream_boundaries() {
    let frame = v1::ClientFrame {
        correlation_id: 42,
        payload: Some(v1::client_frame::Payload::Health(v1::HealthRequest {
            session: None,
        })),
    };
    let decoded = v1::ClientFrame::decode(frame.encode_to_vec().as_slice()).unwrap();

    assert_eq!(decoded.correlation_id, 42);
    assert!(matches!(
        decoded.payload,
        Some(v1::client_frame::Payload::Health(_))
    ));

    let end = v1::ServerFrame {
        correlation_id: 42,
        payload: Some(v1::server_frame::Payload::StreamEnd(v1::StreamEnd {})),
    };
    assert!(matches!(
        v1::ServerFrame::decode(end.encode_to_vec().as_slice())
            .unwrap()
            .payload,
        Some(v1::server_frame::Payload::StreamEnd(_))
    ));
}

#[tokio::test]
async fn rejects_oversized_frame_length_before_reading_a_payload() {
    let (mut sender, mut receiver) = tokio::io::duplex(16);
    tokio::io::AsyncWriteExt::write_all(&mut sender, &1025_u32.to_be_bytes())
        .await
        .unwrap();

    let error = fm_semantic_protocol::read_frame::<_, v1::ClientFrame>(&mut receiver, 1024)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        fm_semantic_protocol::FrameError::TooLarge {
            actual: 1025,
            maximum: 1024
        }
    ));
}
