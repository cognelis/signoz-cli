use signoz_client::{Credentials, ErrorCode, HttpTransport, SigNozOrigin};
use std::time::Duration;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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
async fn rejects_responses_above_the_configured_cap() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/large"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 9]))
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
