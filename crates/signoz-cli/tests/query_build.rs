use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_json, method, path},
};

fn builder(config: &std::path::Path) -> Command {
    let mut command = Command::cargo_bin("signoz-cli").unwrap();
    command
        .env("SIGNOZ_CONFIG", config)
        .args(["--raw", "query", "build"]);
    command
}

#[test]
fn builds_frozen_search_queries_without_configuration_or_credentials() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("broken.toml");
    std::fs::write(&config, "invalid = [").unwrap();
    for signal in ["logs", "traces"] {
        let output = builder(&config)
            .args([
                "--signal",
                signal,
                "--start",
                "@1000",
                "--end",
                "@2000",
                "--limit",
                "7",
                "--offset",
                "2",
                "--filter",
                "service.name = 'checkout'",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let body: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(body["schemaVersion"], "v1");
        assert_eq!(body["start"], 1000);
        assert_eq!(body["end"], 2000);
        let spec = &body["compositeQuery"]["queries"][0]["spec"];
        assert_eq!(spec["signal"], signal);
        assert_eq!(spec["limit"], 7);
        assert_eq!(spec["offset"], 2);
        assert_eq!(spec["filter"]["expression"], "service.name = 'checkout'");
    }
    builder(&config)
        .args(["--signal", "traces", "--search", "oops"])
        .assert()
        .code(2)
        .stdout("");
    builder(&config)
        .args(["--signal", "logs", "--since", "0h"])
        .assert()
        .code(2)
        .stdout("");
    assert_eq!(std::fs::read_to_string(config).unwrap(), "invalid = [");
}

#[tokio::test]
async fn built_payload_can_be_reviewed_and_then_submitted_without_rebuilding_it() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("empty.toml");
    let output = builder(&config)
        .args([
            "--signal",
            "logs",
            "--search",
            "can't connect",
            "--start",
            "@1000",
            "--end",
            "@2000",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        body["compositeQuery"]["queries"][0]["spec"]["filter"]["expression"]
            .as_str()
            .unwrap()
            .contains("connect")
    );
    let file = root.path().join("request.json");
    std::fs::write(&file, output.stdout).unwrap();
    let validated = Command::cargo_bin("signoz-cli")
        .unwrap()
        .env("SIGNOZ_CONFIG", &config)
        .args(["--raw", "query", "validate", "--file"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(validated.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&validated.stdout).unwrap(),
        body
    );
    std::fs::write(&file, validated.stdout).unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v5/query_range"))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
        .expect(1)
        .mount(&server)
        .await;
    Command::cargo_bin("signoz-cli")
        .unwrap()
        .env("SIGNOZ_CONFIG", &config)
        .env("SIGNOZ_API_KEY", "test-secret")
        .args([
            "--url",
            &server.uri(),
            "--output",
            "json",
            "query",
            "run",
            "--file",
        ])
        .arg(file)
        .assert()
        .success()
        .stdout(predicate::str::contains("query.run"));
}

#[test]
fn validates_saved_queries_offline_and_emits_normalized_payloads() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("broken.toml");
    std::fs::write(&config, "invalid = [").unwrap();
    let body = json!({"start": 1000, "end": 2000, "requestType": "raw",
        "compositeQuery": {"queries": [{"type": "builder_query", "spec": {"signal": "logs"}}]}});
    let file = root.path().join("query.json");
    std::fs::write(&file, body.to_string()).unwrap();
    let output = Command::cargo_bin("signoz-cli")
        .unwrap()
        .env("SIGNOZ_CONFIG", &config)
        .args(["--raw", "query", "validate", "--file"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let normalized: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(normalized["schemaVersion"], "v1");
    let mut expected = body.clone();
    expected["schemaVersion"] = json!("v1");
    assert_eq!(normalized, expected);
    assert_eq!(
        serde_json::from_str::<Value>(&std::fs::read_to_string(&file).unwrap()).unwrap(),
        body
    );
    Command::cargo_bin("signoz-cli")
        .unwrap()
        .env("SIGNOZ_CONFIG", &config)
        .args(["--output", "json", "query", "validate", "--file", "-"])
        .write_stdin(normalized.to_string())
        .assert()
        .success()
        .stdout(predicate::str::contains("query.validate"));
    for invalid in ["{", "[]", r#"{"start":2,"end":1}"#] {
        Command::cargo_bin("signoz-cli")
            .unwrap()
            .env("SIGNOZ_CONFIG", &config)
            .args(["query", "validate"])
            .write_stdin(invalid)
            .assert()
            .code(2)
            .stdout("");
    }
}
