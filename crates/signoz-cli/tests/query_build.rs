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
        .expect(2)
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
    Command::cargo_bin("signoz-cli")
        .unwrap()
        .env("SIGNOZ_CONFIG", &config)
        .env("SIGNOZ_API_KEY", "test-secret")
        .args([
            "--url",
            &server.uri(),
            "logs",
            "search",
            "--search",
            "can't connect",
            "--since",
            "1s",
            "--end",
            "@2000",
        ])
        .assert()
        .success();
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

#[tokio::test]
async fn builds_aggregates_with_the_same_payload_as_direct_execution() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("empty.toml");
    for signal in ["logs", "traces"] {
        let options = [
            "--aggregation",
            "p99",
            "--aggregate-on",
            "duration_nano",
            "--group-by",
            "service.name",
            "--step",
            "60",
            "--limit",
            "7",
            "--start",
            "@1000",
            "--end",
            "@121000",
        ];
        let output = builder(&config)
            .args(["--signal", signal])
            .args(options)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let body: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(body["requestType"], "time_series");
        assert_eq!(body["start"], 1000);
        assert_eq!(body["end"], 121000);
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
            .args(["--url", &server.uri(), signal, "aggregate"])
            .args(options)
            .assert()
            .success();
        server.verify().await;
    }
}

#[test]
fn aggregation_build_rejects_ambiguous_or_incomplete_options() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("broken.toml");
    std::fs::write(&config, "bad = [").unwrap();
    let output = builder(&config)
        .args(["--signal", "logs", "--aggregation", "count"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["requestType"],
        "scalar"
    );
    for args in [
        vec!["--aggregation", "p99"],
        vec!["--group-by", "service.name"],
        vec!["--step", "60"],
        vec!["--aggregate-on", "duration_nano"],
        vec!["--aggregation", "count", "--offset", "0"],
        vec!["--aggregation", "count", "--search", "error"],
        vec!["--aggregation", "count", "--group-by", " "],
        vec!["--aggregation", "count", "--step", "0"],
    ] {
        builder(&config)
            .args(["--signal", "logs"])
            .args(args)
            .assert()
            .code(2)
            .stdout("");
    }
}

#[test]
fn saves_queries_atomically_and_can_validate_in_place() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("broken.toml");
    std::fs::write(&config, "bad = [").unwrap();
    let file = root.path().join("query.json");
    let output = builder(&config)
        .args(["--signal", "logs", "--aggregation", "count", "--out"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        serde_json::from_str::<Value>(&std::fs::read_to_string(&file).unwrap()).unwrap()
    );
    let mut body: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    body.as_object_mut().unwrap().remove("schemaVersion");
    std::fs::write(&file, body.to_string()).unwrap();
    Command::cargo_bin("signoz-cli")
        .unwrap()
        .env("SIGNOZ_CONFIG", &config)
        .args(["query", "validate", "--file"])
        .arg(&file)
        .arg("--out")
        .arg(&file)
        .assert()
        .success();
    let normalized: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(normalized["schemaVersion"], "v1");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    std::fs::write(&file, "invalid").unwrap();
    Command::cargo_bin("signoz-cli")
        .unwrap()
        .env("SIGNOZ_CONFIG", &config)
        .args(["query", "validate", "--file"])
        .arg(&file)
        .arg("--out")
        .arg(&file)
        .assert()
        .code(2)
        .stdout("");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "invalid");
    let dir = root.path().join("not-a-file");
    std::fs::create_dir(&dir).unwrap();
    let before = std::fs::read_dir(root.path()).unwrap().count();
    builder(&config)
        .args(["--signal", "logs", "--out"])
        .arg(&dir)
        .assert()
        .code(2)
        .stdout("");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), before);
    assert!(dir.is_dir());
    builder(&config)
        .current_dir(root.path())
        .args(["--signal", "logs", "--out", "relative.json"])
        .assert()
        .success();
    assert!(root.path().join("relative.json").is_file());
    builder(&config)
        .current_dir(root.path())
        .args(["--signal", "logs", "--out", "-"])
        .assert()
        .success();
    assert!(!root.path().join("-").exists());
    #[cfg(unix)]
    {
        let link = root.path().join("link.json");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        builder(&config)
            .args(["--signal", "logs", "--out"])
            .arg(&link)
            .assert()
            .success();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "invalid");
        assert!(
            !std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}

#[test]
fn lookback_with_end_builds_a_reproducible_historical_query() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("broken.toml");
    std::fs::write(&config, "bad = [").unwrap();
    let result = builder(&config)
        .args(["--signal", "logs", "--since", "1m", "--end", "@121000"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let body: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(body["start"], 61000);
    assert_eq!(body["end"], 121000);
    builder(&config)
        .args(["--signal", "logs", "--end", "@121000"])
        .assert()
        .code(2)
        .stdout("");
    builder(&config)
        .args([
            "--signal", "logs", "--since", "1m", "--start", "@1", "--end", "@121000",
        ])
        .assert()
        .code(2)
        .stdout("");
}

#[tokio::test]
async fn retimes_a_saved_query_without_changing_its_specification() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("broken.toml");
    std::fs::write(&config, "bad = [").unwrap();
    let original = builder(&config)
        .args([
            "--signal",
            "traces",
            "--aggregation",
            "p99",
            "--aggregate-on",
            "duration_nano",
            "--group-by",
            "service.name",
            "--step",
            "60",
            "--start",
            "@1000",
            "--end",
            "@61000",
        ])
        .output()
        .unwrap();
    assert!(original.status.success());
    let mut body: Value = serde_json::from_slice(&original.stdout).unwrap();
    body["variables"] = json!({"custom":"preserved"});
    let input = root.path().join("original.json");
    let output = root.path().join("retimed.json");
    std::fs::write(&input, body.to_string()).unwrap();
    let result = Command::cargo_bin("signoz-cli")
        .unwrap()
        .env("SIGNOZ_CONFIG", &config)
        .args(["--raw", "query", "retime", "--file"])
        .arg(&input)
        .args(["--since", "1m", "--end", "@121000", "--out"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut expected = body.clone();
    expected["start"] = json!(61000);
    expected["end"] = json!(121000);
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap(),
        expected
    );
    assert_eq!(
        serde_json::from_str::<Value>(&std::fs::read_to_string(&input).unwrap()).unwrap(),
        body
    );
    assert_eq!(
        serde_json::from_str::<Value>(&std::fs::read_to_string(&output).unwrap()).unwrap(),
        expected
    );
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v5/query_range"))
        .and(body_json(expected))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":{}})))
        .expect(1)
        .mount(&server)
        .await;
    Command::cargo_bin("signoz-cli")
        .unwrap()
        .env("SIGNOZ_CONFIG", root.path().join("empty.toml"))
        .env("SIGNOZ_API_KEY", "test-secret")
        .args(["--url", &server.uri(), "query", "run", "--file"])
        .arg(&output)
        .assert()
        .success();
    for args in [
        vec![],
        vec!["--end", "@2000"],
        vec!["--since", "0h"],
        vec!["--start", "@3", "--end", "@1"],
    ] {
        Command::cargo_bin("signoz-cli")
            .unwrap()
            .env("SIGNOZ_CONFIG", &config)
            .args(["query", "retime", "--file"])
            .arg(&input)
            .arg("--out")
            .arg(&output)
            .args(args)
            .assert()
            .code(2)
            .stdout("");
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&output).unwrap()).unwrap()["end"],
            121000
        );
    }
    let from_stdin = Command::cargo_bin("signoz-cli")
        .unwrap()
        .env("SIGNOZ_CONFIG", &config)
        .args([
            "--raw", "query", "retime", "--start", "@2000", "--end", "@62000",
        ])
        .write_stdin(body.to_string())
        .output()
        .unwrap();
    assert!(from_stdin.status.success());
    let mut expected_stdin = body.clone();
    expected_stdin["start"] = json!(2000);
    expected_stdin["end"] = json!(62000);
    assert_eq!(
        serde_json::from_slice::<Value>(&from_stdin.stdout).unwrap(),
        expected_stdin
    );
    std::fs::write(&input, "invalid JSON").unwrap();
    Command::cargo_bin("signoz-cli")
        .unwrap()
        .env("SIGNOZ_CONFIG", &config)
        .args(["query", "retime", "--file"])
        .arg(&input)
        .args(["--since", "1h", "--out"])
        .arg(&output)
        .assert()
        .code(2)
        .stdout("");
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(&output).unwrap()).unwrap()["end"],
        121000
    );
}
