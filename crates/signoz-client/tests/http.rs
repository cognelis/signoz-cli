use signoz_client::{Credentials, ErrorCode, HttpTransport, SigNozOrigin};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// wiremock completes response bodies; use TCP to reproduce a mid-body disconnect.
fn truncated_server(recover_after: usize) -> (String, Sender<()>, JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let (stop, stopped) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut count = 0;
        while matches!(stopped.try_recv(), Err(mpsc::TryRecvError::Empty)) {
            let mut stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(error) => panic!("mock accept failed: {error}"),
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() <= 8192);
            }
            count += 1;
            let response = if count <= recover_after {
                "HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\n{"
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}"
            };
            stream.write_all(response.as_bytes()).unwrap();
        }
        count
    });
    (url, stop, worker)
}

#[tokio::test]
async fn retries_replay_safe_requests_after_a_response_body_disconnect() {
    let (url, stop, worker) = truncated_server(2);
    let transport = HttpTransport::new(
        SigNozOrigin::parse(&url).unwrap(),
        Credentials::new("secret"),
    )
    .unwrap();
    let result = transport.get("/query", &[]).await;
    drop(stop);
    let count = worker.join().unwrap();
    assert_eq!(result.unwrap().value["ok"], true);
    assert_eq!(count, 3);
}

#[tokio::test]
async fn response_body_retries_respect_replay_safety_and_attempt_limit() {
    for (replay_safe, expected_attempts) in [(true, 3), (false, 1)] {
        let (url, stop, worker) = truncated_server(usize::MAX);
        let transport = HttpTransport::new(
            SigNozOrigin::parse(&url).unwrap(),
            Credentials::new("secret"),
        )
        .unwrap();
        let result = transport
            .execute_with_minimum_version(
                reqwest::Method::POST,
                "/query",
                &[],
                None,
                replay_safe,
                None,
            )
            .await;
        drop(stop);
        let count = worker.join().unwrap();
        assert_eq!(result.unwrap_err().code(), ErrorCode::Network);
        assert_eq!(count, expected_attempts);
    }
}

#[tokio::test]
async fn authenticates_and_retries_replay_safe_requests() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .and(header("SIGNOZ-API-KEY", "secret"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&server)
        .await;

    let transport = HttpTransport::new(
        SigNozOrigin::parse(&server.uri()).unwrap(),
        Credentials::new("secret"),
    )
    .unwrap();
    let response = transport.get("/ok", &[]).await.unwrap();
    assert_eq!(response.value["ok"], true);
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn preserves_reverse_proxy_base_paths_when_building_requests() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/observability/signoz/api/v1/service_accounts/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;
    let transport = HttpTransport::new(
        SigNozOrigin::parse(&format!("{}/observability/signoz/", server.uri())).unwrap(),
        Credentials::new("secret"),
    )
    .unwrap();

    let response = transport
        .get("/api/v1/service_accounts/me", &[])
        .await
        .unwrap();

    assert_eq!(response.value["ok"], true);
}

#[tokio::test]
async fn rejects_responses_above_the_configured_cap() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/large"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 9]))
        .expect(1)
        .mount(&server)
        .await;
    let transport = HttpTransport::new(
        SigNozOrigin::parse(&server.uri()).unwrap(),
        Credentials::new("secret"),
    )
    .unwrap()
    .with_max_response_bytes(8);
    let error = transport.get("/large", &[]).await.unwrap_err();
    assert_eq!(error.code(), ErrorCode::ResponseTooLarge);
    assert_eq!(error.status(), Some(200));
}

#[tokio::test]
async fn complete_but_invalid_json_is_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/invalid"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{"))
        .expect(1)
        .mount(&server)
        .await;
    let transport = HttpTransport::new(
        SigNozOrigin::parse(&server.uri()).unwrap(),
        Credentials::new("secret"),
    )
    .unwrap();
    let error = transport.get("/invalid", &[]).await.unwrap_err();
    assert_eq!(error.code(), ErrorCode::IncompatibleResponse);
}

#[tokio::test]
async fn response_body_retries_stay_within_the_total_timeout() {
    let (url, stop, worker) = truncated_server(usize::MAX);
    let transport = HttpTransport::new_with_timeout(
        SigNozOrigin::parse(&url).unwrap(),
        Credentials::new("secret"),
        Duration::from_millis(250),
    )
    .unwrap();
    let result = transport.get("/query", &[]).await;
    drop(stop);
    let count = worker.join().unwrap();
    assert_eq!(result.unwrap_err().code(), ErrorCode::Timeout);
    assert!((1..=2).contains(&count));
}

#[tokio::test]
async fn timeout_bounds_the_entire_retry_sequence() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/slow"))
        .respond_with(ResponseTemplate::new(503).set_delay(Duration::from_millis(100)))
        .mount(&server)
        .await;
    let transport = HttpTransport::new_with_timeout(
        SigNozOrigin::parse(&server.uri()).unwrap(),
        Credentials::new("secret"),
        Duration::from_millis(25),
    )
    .unwrap();
    let started = std::time::Instant::now();
    let error = transport.get("/slow", &[]).await.unwrap_err();
    assert_eq!(error.code(), ErrorCode::Timeout);
    assert!(started.elapsed() < Duration::from_millis(80));
}

#[tokio::test]
async fn upstream_errors_redact_an_echoed_api_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/echo"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad key: echo-secret"))
        .mount(&server)
        .await;
    let transport = HttpTransport::new(
        SigNozOrigin::parse(&server.uri()).unwrap(),
        Credentials::new("echo-secret"),
    )
    .unwrap();
    let error = transport.get("/echo", &[]).await.unwrap_err();
    assert_eq!(error.code(), ErrorCode::RequestRejected);
    assert!(!error.to_string().contains("echo-secret"));
    assert!(error.to_string().contains("[REDACTED]"));
}

#[tokio::test]
async fn redirects_never_forward_the_custom_api_key() {
    let destination = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/steal"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&destination)
        .await;

    let origin = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/redirect"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/steal", destination.uri())),
        )
        .mount(&origin)
        .await;

    let transport = HttpTransport::new(
        SigNozOrigin::parse(&origin.uri()).unwrap(),
        Credentials::new("never-forward-this"),
    )
    .unwrap();
    let error = transport.get("/redirect", &[]).await.unwrap_err();
    assert_eq!(error.code(), ErrorCode::Upstream);
    assert!(destination.received_requests().await.unwrap().is_empty());
}
