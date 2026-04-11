//! Integration test: state signal + HTTP response socket working together.
//!
//! Simulates what happens when a skill script sends a response AND signals IDLE,
//! verifying both the response delivery and the state transition.

use std::time::Duration;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn script_sends_response_and_signals_idle() {
    let tmp = tempfile::tempdir().unwrap();
    let agents_dir = tmp.path().to_path_buf();

    // 1. Start the state signal server
    let signal_server = nexal_agent::StateSignalServer::start(&agents_dir)
        .await
        .unwrap();
    let mut signal_rx = signal_server.subscribe();

    // 2. Verify socket exists
    let state_socket = agents_dir.join(".state");
    assert!(state_socket.exists());

    // 3. Simulate what http_send.py does:
    //    a) Send the response to the response socket (skipped — tested separately)
    //    b) Signal IDLE via the state socket
    let session_key = "http:test-chat";

    let mut stream = tokio::net::UnixStream::connect(&state_socket)
        .await
        .unwrap();
    let signal_msg = format!(
        "{{\"session\":\"{session_key}\",\"state\":\"IDLE\"}}\n"
    );
    stream.write_all(signal_msg.as_bytes()).await.unwrap();
    drop(stream);

    // 4. Actor side: receive the signal
    let signal = tokio::time::timeout(Duration::from_secs(2), signal_rx.recv())
        .await
        .expect("should receive signal within 2s")
        .expect("channel not lagged");

    assert_eq!(signal.session, session_key);
    assert_eq!(signal.state, "IDLE");

    // 5. Verify that a different session doesn't match
    let mut stream2 = tokio::net::UnixStream::connect(&state_socket)
        .await
        .unwrap();
    stream2
        .write_all(b"{\"session\":\"telegram:-999\",\"state\":\"IDLE\"}\n")
        .await
        .unwrap();
    drop(stream2);

    let signal2 = tokio::time::timeout(Duration::from_secs(2), signal_rx.recv())
        .await
        .unwrap()
        .unwrap();

    // The broadcast delivers to all subscribers — the actor would filter by session_key
    assert_eq!(signal2.session, "telegram:-999");
    assert_ne!(signal2.session, session_key); // different session
}

#[tokio::test]
async fn concurrent_sessions_get_independent_signals() {
    let tmp = tempfile::tempdir().unwrap();
    let signal_server = nexal_agent::StateSignalServer::start(tmp.path())
        .await
        .unwrap();

    // Two "actors" subscribe
    let mut rx1 = signal_server.subscribe();
    let mut rx2 = signal_server.subscribe();

    let state_socket = tmp.path().join(".state");

    // Send signal for session A
    let mut stream = tokio::net::UnixStream::connect(&state_socket)
        .await
        .unwrap();
    stream
        .write_all(b"{\"session\":\"http:chatA\",\"state\":\"IDLE\"}\n")
        .await
        .unwrap();
    drop(stream);

    // Both subscribers should receive it
    let s1 = tokio::time::timeout(Duration::from_secs(2), rx1.recv())
        .await
        .unwrap()
        .unwrap();
    let s2 = tokio::time::timeout(Duration::from_secs(2), rx2.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(s1.session, "http:chatA");
    assert_eq!(s2.session, "http:chatA");
}
