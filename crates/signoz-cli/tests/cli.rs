use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::json;
use signoz_cli::output::{Meta, SuccessEnvelope};
use wiremock::matchers::{body_partial_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn success_envelope_is_versioned() {
    let value = SuccessEnvelope::new("metrics.list", json!([]), Meta::default());
    let serialized = serde_json::to_value(value).unwrap();
    assert_eq!(serialized["schemaVersion"], "1");
    assert_eq!(serialized["command"], "metrics.list");
}

#[test]
fn version_is_independent_of_connection_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("broken.toml");
    std::fs::write(&config, "not valid = [").unwrap();
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args(["--output", "json", "version"])
        .env("SIGNOZ_CONFIG", config)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"command\": \"version\""));
}

#[test]
fn client_error_codes_have_stable_process_exit_codes() {
    use signoz_client::ErrorCode;
    assert_eq!(signoz_cli::output::exit_code(ErrorCode::Validation), 2);
    assert_eq!(signoz_cli::output::exit_code(ErrorCode::Unauthorized), 3);
    assert_eq!(signoz_cli::output::exit_code(ErrorCode::Network), 4);
    assert_eq!(signoz_cli::output::exit_code(ErrorCode::NotFound), 5);
    assert_eq!(signoz_cli::output::exit_code(ErrorCode::RequestRejected), 5);
    assert_eq!(signoz_cli::output::exit_code(ErrorCode::Upstream), 6);
    assert_eq!(
        signoz_cli::output::exit_code(ErrorCode::FeatureUnsupported),
        7
    );
    assert_eq!(
        signoz_cli::output::exit_code(ErrorCode::IncompatibleResponse),
        7
    );
}

#[tokio::test]
async fn metrics_list_emits_the_versioned_json_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/metrics"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"metrics": ["requests"]}
        })))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args([
            "--url",
            &server.uri(),
            "--output",
            "json",
            "metrics",
            "list",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"schemaVersion\": \"1\""))
        .stdout(predicate::str::contains("\"command\": \"metrics.list\""));
}

#[tokio::test]
async fn metrics_list_reports_upstream_limit_as_potentially_incomplete() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/metrics"))
        .and(query_param("limit", "3"))
        .and(query_param("searchText", "http"))
        .and(query_param("source", "meter"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"metrics": ["first", "second", "third"]}
        })))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args([
            "--url",
            &server.uri(),
            "--output",
            "json",
            "metrics",
            "list",
            "--limit",
            "3",
            "--search",
            "http",
            "--source",
            "meter",
            "--since",
            "1h",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"second\""))
        .stdout(predicate::str::contains("\"third\""))
        .stdout(predicate::str::contains("\"first\""))
        .stdout(predicate::str::contains("\"hasMore\": true"))
        .stdout(predicate::str::contains("has no offset pagination"))
        .stdout(predicate::str::contains("\"nextOffset\"").not())
        .stdout(predicate::str::contains("\"total\":").not());
}

#[tokio::test]
async fn unbounded_resource_lists_apply_exact_client_side_pagination() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/dashboards"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                {"id": "first"},
                {"id": "second"},
                {"id": "third"}
            ]
        })))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args([
            "--url",
            &server.uri(),
            "--output",
            "json",
            "dashboards",
            "list",
            "--limit",
            "2",
            "--offset",
            "1",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"second\""))
        .stdout(predicate::str::contains("\"id\": \"third\""))
        .stdout(predicate::str::contains("\"total\": 3"))
        .stdout(predicate::str::contains("\"hasMore\": false"));
}

#[tokio::test]
async fn empty_and_incompatible_list_shapes_are_distinguished() {
    let empty_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/dashboards"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": null
        })))
        .mount(&empty_server)
        .await;
    let mut empty = Command::cargo_bin("signoz-cli").unwrap();
    empty
        .args([
            "--url",
            &empty_server.uri(),
            "--output",
            "json",
            "dashboards",
            "list",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"total\": 0"));

    let view_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/explorer/views"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success"
        })))
        .mount(&view_server)
        .await;
    let mut views = Command::cargo_bin("signoz-cli").unwrap();
    views
        .args([
            "--url",
            &view_server.uri(),
            "--output",
            "json",
            "views",
            "list",
            "--source-page",
            "logs",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"total\": 0"));

    let metrics_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/metrics"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": {"metrics": null}
        })))
        .mount(&metrics_server)
        .await;
    let mut metrics = Command::cargo_bin("signoz-cli").unwrap();
    metrics
        .args([
            "--url",
            &metrics_server.uri(),
            "--output",
            "json",
            "metrics",
            "list",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"total\": 0"));

    let drift_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/alerts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": {"unexpected": true}
        })))
        .mount(&drift_server)
        .await;
    let mut drift = Command::cargo_bin("signoz-cli").unwrap();
    drift
        .args([
            "--url",
            &drift_server.uri(),
            "--output",
            "json",
            "alerts",
            "list",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .code(7)
        .stdout("")
        .stderr(predicate::str::contains(
            "\"code\": \"incompatible_response\"",
        ));
}

#[tokio::test]
async fn metrics_query_resolves_aggregation_from_catalog_metadata() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/metrics"))
        .and(query_param("searchText", "http.requests"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"metrics": [{
                "metricName": "http.requests",
                "type": "sum",
                "temporality": "cumulative",
                "isMonotonic": true
            }]}
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v5/query_range"))
        .and(body_partial_json(json!({
            "compositeQuery": {"queries": [{"spec": {"aggregations": [{
                "metricName": "http.requests",
                "temporality": "cumulative",
                "timeAggregation": "rate",
                "spaceAggregation": "sum"
            }]}}]}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"data": {"results": [{"rows": []}]}}
        })))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args([
            "--url",
            &server.uri(),
            "--output",
            "json",
            "metrics",
            "query",
            "http.requests",
            "--since",
            "1h",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"command\": \"metrics.query\""));
}

#[tokio::test]
async fn metrics_top_exact_cap_reports_no_offset_pagination() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/metrics/treemap"))
        .and(body_partial_json(json!({"limit": 2})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": {"samples": [
                {"metricName": "first"},
                {"metricName": "second"}
            ]}
        })))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args([
            "--url",
            &server.uri(),
            "--output",
            "json",
            "metrics",
            "top",
            "--since",
            "1h",
            "--limit",
            "2",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"hasMore\": true"))
        .stdout(predicate::str::contains("has no offset pagination"))
        .stdout(predicate::str::contains("\"nextOffset\"").not());
}

#[tokio::test]
async fn rule_history_uses_only_the_upstream_cursor_for_completeness() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/rules/rule-id/history/timeline"))
        .and(query_param("limit", "20"))
        .and(query_param("order", "desc"))
        .and(query_param("cursor", "current-cursor"))
        .and(query_param("state", "firing"))
        .and(query_param("filterExpression", "severity = 'critical'"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "items": [{"state": "firing"}],
                "nextCursor": "next-cursor"
            }
        })))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args([
            "--url",
            &server.uri(),
            "--output",
            "json",
            "rules",
            "history",
            "rule-id",
            "--since",
            "1h",
            "--cursor",
            "current-cursor",
            "--order",
            "desc",
            "--state",
            "firing",
            "--filter",
            "severity = 'critical'",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"hasMore\": true"))
        .stdout(predicate::str::contains("\"cursor\": \"current-cursor\""))
        .stdout(predicate::str::contains("\"nextCursor\": \"next-cursor\""))
        .stdout(predicate::str::contains("\"nextOffset\"").not());

    let complete_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/rules/rule-id/history/timeline"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"items": vec![json!({"state": "inactive"}); 20]}
        })))
        .mount(&complete_server)
        .await;
    let mut complete = Command::cargo_bin("signoz-cli").unwrap();
    complete
        .args([
            "--url",
            &complete_server.uri(),
            "--output",
            "json",
            "rules",
            "history",
            "rule-id",
            "--since",
            "1h",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"hasMore\": false"))
        .stdout(predicate::str::contains("\"nextCursor\"").not());
}

#[tokio::test]
async fn unauthorized_is_json_on_stderr_and_never_leaks_the_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/service_accounts/me"))
        .respond_with(ResponseTemplate::new(401).set_body_string("denied"))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args(["--url", &server.uri(), "--output", "json", "auth", "check"])
        .env("SIGNOZ_API_KEY", "never-print-this")
        .assert()
        .code(3)
        .stdout("")
        .stderr(predicate::str::contains("\"code\": \"unauthorized\""))
        .stderr(predicate::str::contains("\"status\": 401"))
        .stderr(predicate::str::contains("never-print-this").not());
}

#[tokio::test]
async fn auth_check_success_uses_the_service_account_route() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/service_accounts/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "id": "account-id",
                "name": "automation",
                "orgId": "org-id",
                "apiKey": "must-not-be-emitted"
            }
        })))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args(["--url", &server.uri(), "--output", "json", "auth", "check"])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"command\": \"auth.check\""))
        .stdout(predicate::str::contains("\"authenticated\": true"))
        .stdout(predicate::str::contains("must-not-be-emitted").not());
}

#[tokio::test]
async fn channel_list_projects_safe_summaries_without_provider_credentials() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/channels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": [{
                "id": "channel-id",
                "type": "slack",
                "createdAt": "2026-07-22T00:00:00Z",
                "updatedAt": "2026-07-22T00:00:00Z",
                "data": "{\"name\":\"on-call\",\"webhookUrl\":\"https://secret.invalid/hook\",\"apiKey\":\"provider-secret\"}"
            }]
        })))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args([
            "--url",
            &server.uri(),
            "--output",
            "json",
            "channels",
            "list",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\": \"on-call\""))
        .stdout(predicate::str::contains("provider-secret").not())
        .stdout(predicate::str::contains("secret.invalid").not())
        .stdout(predicate::str::contains("webhookUrl").not());
}

#[tokio::test]
async fn metrics_usage_combines_both_reference_endpoints_at_the_cli_boundary() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/metrics/dashboards"))
        .and(query_param("metricName", "http.requests"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/metrics/alerts"))
        .and(query_param("metricName", "http.requests"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args([
            "--url",
            &server.uri(),
            "--output",
            "json",
            "metrics",
            "usage",
            "http.requests",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"command\": \"metrics.usage\""))
        .stdout(predicate::str::contains("\"dashboards\""))
        .stdout(predicate::str::contains("\"alerts\""));
}

#[tokio::test]
async fn cli_timeout_oversize_and_reflected_secret_errors_are_structured() {
    let timeout_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/service_accounts/me"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(2))
                .set_body_json(json!({"data": {}})),
        )
        .mount(&timeout_server)
        .await;
    let mut timeout = Command::cargo_bin("signoz-cli").unwrap();
    timeout
        .args([
            "--url",
            &timeout_server.uri(),
            "--output",
            "json",
            "--timeout",
            "1",
            "auth",
            "check",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .code(4)
        .stdout("")
        .stderr(predicate::str::contains("\"code\": \"timeout\""));

    let oversize_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/service_accounts/me"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 64 * 1024 * 1024 + 1]))
        .mount(&oversize_server)
        .await;
    let mut oversize = Command::cargo_bin("signoz-cli").unwrap();
    oversize
        .args([
            "--url",
            &oversize_server.uri(),
            "--output",
            "json",
            "auth",
            "check",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .code(7)
        .stdout("")
        .stderr(predicate::str::contains("\"code\": \"response_too_large\""));

    let reflected_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/service_accounts/me"))
        .respond_with(ResponseTemplate::new(400).set_body_string("bad key: echo-secret"))
        .mount(&reflected_server)
        .await;
    let mut reflected = Command::cargo_bin("signoz-cli").unwrap();
    reflected
        .args([
            "--url",
            &reflected_server.uri(),
            "--output",
            "json",
            "auth",
            "check",
        ])
        .env("SIGNOZ_API_KEY", "echo-secret")
        .assert()
        .code(5)
        .stdout("")
        .stderr(predicate::str::contains("[REDACTED]"))
        .stderr(predicate::str::contains("echo-secret").not());
}

#[tokio::test]
async fn log_search_sends_v5_spec_and_reports_pagination_meta() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v5/query_range"))
        .and(body_partial_json(json!({
            "schemaVersion": "v1",
            "requestType": "raw",
            "compositeQuery": {"queries": [{"spec": {
                "signal": "logs",
                "limit": 1,
                "offset": 2
            }}]}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"data": {"results": [{"rows": [{"data": {"body": "failed"}}]}]}}
        })))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args([
            "--url",
            &server.uri(),
            "--output",
            "json",
            "logs",
            "search",
            "--since",
            "1h",
            "--limit",
            "1",
            "--offset",
            "2",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"limit\": 1"))
        .stdout(predicate::str::contains("\"offset\": 2"))
        .stdout(predicate::str::contains("\"nextOffset\": 3"))
        .stdout(predicate::str::contains("\"start\""))
        .stdout(predicate::str::contains("\"end\""));
}

#[tokio::test]
async fn aggregate_commands_require_a_field_and_never_advertise_offset_pagination() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v5/query_range"))
        .and(body_partial_json(json!({
            "requestType": "scalar",
            "compositeQuery": {"queries": [{"spec": {
                "signal": "logs",
                "limit": 1,
                "aggregations": [{"expression": "p99(duration_nano)"}]
            }}]}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"data": {"results": [{"rows": [{"data": {"p99": 42}}]}]}}
        })))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args([
            "--url",
            &server.uri(),
            "--output",
            "json",
            "logs",
            "aggregate",
            "--since",
            "1h",
            "--aggregation",
            "p99",
            "--aggregate-on",
            "duration_nano",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"command\": \"logs.aggregate\""))
        .stdout(predicate::str::contains("\"hasMore\"").not())
        .stdout(predicate::str::contains("\"nextOffset\"").not());
    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(
        body.pointer("/compositeQuery/queries/0/spec/offset")
            .is_none()
    );

    let mut missing_field = Command::cargo_bin("signoz-cli").unwrap();
    missing_field
        .args([
            "--output",
            "json",
            "traces",
            "aggregate",
            "--since",
            "1h",
            "--aggregation",
            "avg",
        ])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains(
            "--aggregate-on is required for --aggregation avg",
        ));

    let mut offset = Command::cargo_bin("signoz-cli").unwrap();
    offset
        .args([
            "--output",
            "json",
            "logs",
            "aggregate",
            "--since",
            "1h",
            "--offset",
            "1",
        ])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("unexpected argument '--offset'"));
}

#[test]
fn invalid_time_is_a_validation_error_before_network_access() {
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args(["--output", "json", "logs", "search", "--since", "0h"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("\"code\": \"validation\""));
}

#[test]
fn missing_connection_is_a_structured_configuration_error() {
    let directory = tempfile::tempdir().unwrap();
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args(["--output", "json", "auth", "check"])
        .env("SIGNOZ_CONFIG", directory.path().join("config.toml"))
        .env_remove("SIGNOZ_URL")
        .env_remove("SIGNOZ_API_KEY")
        .assert()
        .code(3)
        .stdout("")
        .stderr(predicate::str::contains("\"code\": \"config\""));
}

#[test]
fn raw_query_rejects_non_v5_json_before_network_access() {
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args(["--output", "json", "query", "run", "--file", "-"])
        .write_stdin("{}")
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("Query Builder v5"));
}

#[tokio::test]
async fn raw_query_inserts_an_omitted_v5_schema_version() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v5/query_range"))
        .and(body_partial_json(json!({"schemaVersion": "v1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .mount(&server)
        .await;
    let request = json!({
        "start": 1,
        "end": 2,
        "requestType": "raw",
        "compositeQuery": {"queries": [{
            "type": "builder_query",
            "spec": {"name": "A", "signal": "logs"}
        }]}
    });
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args([
            "--url",
            &server.uri(),
            "--output",
            "json",
            "query",
            "run",
            "--file",
            "-",
        ])
        .env("SIGNOZ_API_KEY", "test-secret")
        .write_stdin(request.to_string())
        .assert()
        .success();
}

#[tokio::test]
async fn unsupported_rule_list_reports_the_minimum_version() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/rules"))
        .respond_with(ResponseTemplate::new(404).set_body_string("missing"))
        .mount(&server)
        .await;
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args(["--url", &server.uri(), "--output", "json", "rules", "list"])
        .env("SIGNOZ_API_KEY", "test-secret")
        .assert()
        .code(7)
        .stdout("")
        .stderr(predicate::str::contains(
            "\"code\": \"feature_unsupported\"",
        ))
        .stderr(predicate::str::contains("requires SigNoz 0.120 or newer"));
}

#[tokio::test]
async fn remaining_remote_subcommands_dispatch_to_their_read_routes() {
    struct Case {
        args: &'static [&'static str],
        method: &'static str,
        path: &'static str,
        command: &'static str,
        response: serde_json::Value,
    }

    let query_response = || json!({"data": {"data": {"results": [{"rows": []}]}}});
    let cases = [
        Case {
            args: &["metrics", "top", "--since", "1h"],
            method: "POST",
            path: "/api/v2/metrics/treemap",
            command: "metrics.top",
            response: json!({"data": []}),
        },
        Case {
            args: &["metrics", "cardinality", "requests", "--since", "1h"],
            method: "GET",
            path: "/api/v2/metrics/attributes",
            command: "metrics.cardinality",
            response: json!({"data": {}}),
        },
        Case {
            args: &["fields", "keys", "--signal", "logs"],
            method: "GET",
            path: "/api/v1/fields/keys",
            command: "fields.keys",
            response: json!({"data": []}),
        },
        Case {
            args: &["fields", "values", "--signal", "logs", "service.name"],
            method: "GET",
            path: "/api/v1/fields/values",
            command: "fields.values",
            response: json!({"data": []}),
        },
        Case {
            args: &["logs", "aggregate", "--since", "1h"],
            method: "POST",
            path: "/api/v5/query_range",
            command: "logs.aggregate",
            response: query_response(),
        },
        Case {
            args: &["traces", "search", "--since", "1h"],
            method: "POST",
            path: "/api/v5/query_range",
            command: "traces.search",
            response: query_response(),
        },
        Case {
            args: &["traces", "aggregate", "--since", "1h"],
            method: "POST",
            path: "/api/v5/query_range",
            command: "traces.aggregate",
            response: query_response(),
        },
        Case {
            args: &["traces", "get", "trace-id", "--since", "1h"],
            method: "POST",
            path: "/api/v5/query_range",
            command: "traces.get",
            response: query_response(),
        },
        Case {
            args: &["services", "list", "--since", "1h"],
            method: "POST",
            path: "/api/v1/services",
            command: "services.list",
            response: json!([]),
        },
        Case {
            args: &["services", "operations", "checkout", "--since", "1h"],
            method: "POST",
            path: "/api/v1/service/top_operations",
            command: "services.operations",
            response: json!({"data": []}),
        },
        Case {
            args: &["alerts", "list"],
            method: "GET",
            path: "/api/v1/alerts",
            command: "alerts.list",
            response: json!({"data": []}),
        },
        Case {
            args: &["rules", "get", "rule-id"],
            method: "GET",
            path: "/api/v2/rules/rule-id",
            command: "rules.get",
            response: json!({"data": {}}),
        },
        Case {
            args: &["rules", "history", "rule-id", "--since", "1h"],
            method: "GET",
            path: "/api/v2/rules/rule-id/history/timeline",
            command: "rules.history",
            response: json!({"data": []}),
        },
        Case {
            args: &["dashboards", "get", "dashboard-id"],
            method: "GET",
            path: "/api/v1/dashboards/dashboard-id",
            command: "dashboards.get",
            response: json!({"data": {}}),
        },
        Case {
            args: &["views", "list", "--source-page", "logs"],
            method: "GET",
            path: "/api/v1/explorer/views",
            command: "views.list",
            response: json!({"data": []}),
        },
        Case {
            args: &["views", "get", "view-id"],
            method: "GET",
            path: "/api/v1/explorer/views/view-id",
            command: "views.get",
            response: json!({"data": {}}),
        },
        Case {
            args: &["channels", "list"],
            method: "GET",
            path: "/api/v1/channels",
            command: "channels.list",
            response: json!({"data": []}),
        },
        Case {
            args: &["channels", "get", "channel-id"],
            method: "GET",
            path: "/api/v1/channels/channel-id",
            command: "channels.get",
            response: json!({"data": {}}),
        },
    ];

    for case in cases {
        let server = MockServer::start().await;
        Mock::given(method(case.method))
            .and(path(case.path))
            .respond_with(ResponseTemplate::new(200).set_body_json(case.response))
            .expect(1)
            .mount(&server)
            .await;
        let mut arguments = vec![
            "--url".to_owned(),
            server.uri(),
            "--output".to_owned(),
            "json".to_owned(),
        ];
        arguments.extend(case.args.iter().map(|value| (*value).to_owned()));
        let mut command = Command::cargo_bin("signoz-cli").unwrap();
        command
            .args(arguments)
            .env("SIGNOZ_API_KEY", "test-secret")
            .assert()
            .success()
            .stdout(predicate::str::contains(format!(
                "\"command\": \"{}\"",
                case.command
            )));
    }
}

#[test]
fn clap_errors_are_structured_when_json_is_requested() {
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args(["--output", "json", "logs", "search", "--invented-flag"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("\"code\": \"validation\""))
        .stderr(predicate::str::contains("unexpected argument"));
}

#[test]
fn clap_errors_are_structured_for_auto_output_in_a_pipeline() {
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .args(["logs", "search", "--invented-flag"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("\"code\": \"validation\""))
        .stderr(predicate::str::contains("unexpected argument"));
}
