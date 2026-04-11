use std::time::Duration;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn signal_socket_receives_idle_signal() {
    let tmp = tempfile::tempdir().unwrap();
    let agents_dir = tmp.path().to_path_buf();

    let server = nexal_agent::StateSignalServer::start(&agents_dir)
        .await
        .unwrap();
    let mut rx = server.subscribe();

    let socket_path = agents_dir.join(".state");
    assert!(socket_path.exists(), "socket file should exist");

    // Simulate a script sending IDLE signal
    let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
    stream
        .write_all(b"{\"session\":\"http:test-chat\",\"state\":\"IDLE\"}\n")
        .await
        .unwrap();
    drop(stream);

    let signal = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("should receive within 2s")
        .expect("should not be lagged");

    assert_eq!(signal.session, "http:test-chat");
    assert_eq!(signal.state, "IDLE");
}

#[tokio::test]
async fn signal_socket_filters_by_session() {
    let tmp = tempfile::tempdir().unwrap();
    let server = nexal_agent::StateSignalServer::start(tmp.path())
        .await
        .unwrap();
    let mut rx = server.subscribe();

    let socket_path = tmp.path().join(".state");

    // Send two signals for different sessions
    for (session, state) in [("telegram:-111", "IDLE"), ("http:test", "IDLE")] {
        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        let msg = format!("{{\"session\":\"{session}\",\"state\":\"{state}\"}}\n");
        stream.write_all(msg.as_bytes()).await.unwrap();
        drop(stream);
    }

    // Should receive both
    let sig1 = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    let sig2 = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();

    let sessions: Vec<&str> = vec![&sig1.session, &sig2.session].into_iter().map(|s| s.as_str()).collect();
    assert!(sessions.contains(&"telegram:-111"));
    assert!(sessions.contains(&"http:test"));
}

#[tokio::test]
async fn signal_socket_ignores_invalid_json() {
    let tmp = tempfile::tempdir().unwrap();
    let server = nexal_agent::StateSignalServer::start(tmp.path())
        .await
        .unwrap();
    let mut rx = server.subscribe();

    let socket_path = tmp.path().join(".state");

    // Send invalid JSON, then valid
    let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
    stream.write_all(b"not json\n").await.unwrap();
    stream
        .write_all(b"{\"session\":\"http:ok\",\"state\":\"IDLE\"}\n")
        .await
        .unwrap();
    drop(stream);

    // Should only receive the valid one
    let signal = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(signal.session, "http:ok");
}
