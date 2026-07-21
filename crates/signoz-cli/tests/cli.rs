use serde_json::json;
use signoz_cli::output::{Meta, SuccessEnvelope};
use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn success_envelope_is_versioned() {
    let value = SuccessEnvelope::new("metrics.list", json!([]), Meta::default());
    let serialized = serde_json::to_value(value).unwrap();
    assert_eq!(serialized["schemaVersion"], "1");
    assert_eq!(serialized["command"], "metrics.list");
}

#[test]
fn client_error_codes_have_stable_process_exit_codes() {
    use signoz_client::ErrorCode;
    assert_eq!(signoz_cli::output::exit_code(ErrorCode::Validation), 2);
    assert_eq!(signoz_cli::output::exit_code(ErrorCode::Unauthorized), 3);
    assert_eq!(signoz_cli::output::exit_code(ErrorCode::Network), 5);
    assert_eq!(signoz_cli::output::exit_code(ErrorCode::FeatureUnsupported), 6);
    assert_eq!(signoz_cli::output::exit_code(ErrorCode::IncompatibleResponse), 7);
}

#[tokio::test]
async fn metrics_list_emits_the_versioned_json_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/metrics"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": ["requests"]})))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz").unwrap();
    command
        .args(["--url", &server.uri(), "--output", "json", "metrics", "list"])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"schemaVersion\": \"1\""))
        .stdout(predicate::str::contains("\"command\": \"metrics.list\""));
}

#[tokio::test]
async fn unauthorized_is_json_on_stderr_and_never_leaks_the_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/service_accounts/me"))
        .respond_with(ResponseTemplate::new(401).set_body_string("denied"))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz").unwrap();
    command
        .args(["--url", &server.uri(), "--output", "json", "auth", "check"])
        .env("SIGNOZ_API_KEY", "never-print-this")
        .assert()
        .code(3)
        .stdout("")
        .stderr(predicate::str::contains("\"code\": \"unauthorized\""))
        .stderr(predicate::str::contains("never-print-this").not());
}

#[test]
fn invalid_time_is_a_validation_error_before_network_access() {
    let mut command = Command::cargo_bin("signoz").unwrap();
    command
        .args(["--output", "json", "logs", "search", "--since", "0h"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("\"code\": \"validation\""));
}
