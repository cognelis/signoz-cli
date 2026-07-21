use signoz_client::{Credentials, ErrorCode, HttpTransport, SigNozOrigin};
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
