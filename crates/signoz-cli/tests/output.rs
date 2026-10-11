use std::process::{Command, Stdio};

#[tokio::test]
async fn log_display_is_readable_while_json_and_raw_retain_every_value() {
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let directory = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    let message = format!("支付失败\n{}", "日志详情".repeat(30));
    let response = json!({"data": {"data": {"results": [{"rows": [{
        "timestamp": "2026-09-06T01:00:00Z", "data": {"body": message, "severity_text": "ERROR"}
    }]}]}}});
    Mock::given(method("POST"))
        .and(path("/api/v5/query_range"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
        .expect(4)
        .mount(&server)
        .await;
    for options in [
        vec![],
        vec!["--output", "table"],
        vec!["--output", "json"],
        vec!["--raw"],
    ] {
        let output = assert_cmd::Command::cargo_bin("signoz-cli")
            .unwrap()
            .args(&options)
            .args(["--url", &server.uri(), "logs", "search", "--limit", "1"])
            .env("SIGNOZ_CONFIG", directory.path().join("config.toml"))
            .env("SIGNOZ_API_KEY", "test-key")
            .env("COLUMNS", "55")
            .assert()
            .success()
            .get_output()
            .clone();
        if options.contains(&"table") {
            let stdout = String::from_utf8(output.stdout).unwrap();
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(stdout.contains("ERROR"));
            assert!(stdout.contains("支付失败\\n"));
            assert!(!stdout.contains("\"data\""));
            assert!(
                stdout
                    .lines()
                    .all(|line| console::measure_text_width(line) <= 55)
            );
            assert!(stderr.contains("displayed values were shortened"));
            assert!(stderr.contains("--output json for full values"));
            assert!(stderr.contains("Results: partial"));
        } else {
            assert!(output.stderr.is_empty());
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            let payload = if options.contains(&"--raw") {
                &value
            } else {
                &value["data"]
            };
            assert_eq!(payload, &response);
        }
    }
}

#[test]
fn explicit_table_errors_remain_human_readable_when_stderr_is_captured() {
    for options in [
        vec![],
        vec!["--output", "table"],
        vec!["--output", "json"],
        vec!["--output", "table", "--raw"],
    ] {
        let output = assert_cmd::Command::cargo_bin("signoz-cli")
            .unwrap()
            .args(&options)
            .args(["logs", "search", "--invented-flag"])
            .assert()
            .code(2)
            .stdout("")
            .get_output()
            .clone();
        if options.contains(&"table") && !options.contains(&"--raw") {
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(
                stderr.starts_with("error: unexpected argument '--invented-flag'"),
                "{stderr}"
            );
            assert!(stderr.contains("\nUsage: signoz-cli"), "{stderr}");
            assert!(!stderr.contains("\\n"), "{stderr}");
        } else {
            let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(error["error"]["code"], "validation");
        }
    }
}

#[test]
fn table_help_for_a_missing_subcommand_keeps_its_line_breaks() {
    // A group without a subcommand shows its help, as `signoz-cli` alone does on a terminal.
    let output = assert_cmd::Command::cargo_bin("signoz-cli")
        .unwrap()
        .args(["--output", "table", "logs"])
        .assert()
        .code(2)
        .stdout("")
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.starts_with("error:"), "{stderr}");
    assert!(stderr.contains("\nUsage: signoz-cli logs"), "{stderr}");
    assert!(!stderr.contains("\\n"), "{stderr}");
}

#[test]
fn profile_tables_mark_the_default_and_config_uses_details() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(&config, "current_profile = \"prod\"\n[profiles.prod]\nurl = \"https://prod.example.com\"\n[profiles.test]\nurl = \"https://test.example.com\"\n").unwrap();
    for command in [["profile", "list"], ["config", "show"]] {
        let output = assert_cmd::Command::cargo_bin("signoz-cli")
            .unwrap()
            .args(["--output", "table"])
            .args(command)
            .env("SIGNOZ_CONFIG", &config)
            .env("COLUMNS", "100")
            .assert()
            .success()
            .get_output()
            .clone();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout
                .lines()
                .any(|line| line.starts_with('*') && line.contains("prod"))
        );
        assert!(stdout.contains("https://test.example.com"));
        if command[0] == "config" {
            assert!(stdout.contains("current_profile: prod"));
        }
    }
}

#[tokio::test]
async fn capped_query_reports_the_profile_and_warning_in_table_mode() {
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let directory = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        format!("[profiles.prod]\nurl = \"{}\"\n", server.uri()),
    )
    .unwrap();
    Mock::given(method("GET"))
        .and(path("/api/v2/metrics"))
        .and(query_param("limit", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"metrics": [{"metricName": "requests"}]}
        })))
        .expect(3)
        .mount(&server)
        .await;
    for options in [
        vec!["--output", "table"],
        vec!["--output", "json"],
        vec!["--raw"],
    ] {
        let output = assert_cmd::Command::cargo_bin("signoz-cli")
            .unwrap()
            .args(&options)
            .args(["--profile", "prod", "metrics", "list", "--limit", "1"])
            .env("SIGNOZ_CONFIG", &config)
            .env("SIGNOZ_API_KEY", "test-key")
            .env_remove("SIGNOZ_URL")
            .assert()
            .success()
            .get_output()
            .clone();
        if options.contains(&"table") {
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(stderr.contains("Profile: prod"));
            assert!(stderr.contains("Results: partial"));
            assert!(stderr.contains("no offset pagination"));
            assert!(!stderr.contains("continue with --offset"));
        } else {
            assert!(output.stderr.is_empty());
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            if options.contains(&"--raw") {
                assert_eq!(
                    value,
                    json!({"data": {"metrics": [{"metricName": "requests"}]}})
                );
            } else {
                assert_eq!(value["meta"]["profile"], "prod");
                assert_eq!(value["meta"]["hasMore"], true);
            }
        }
    }
}

#[test]
fn closing_the_output_pipe_exits_cleanly_in_every_format() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    let contents: String = (0..5000)
        .map(|i| format!("[profiles.p{i}]\nurl = \"https://p{i}.example.com\"\n"))
        .collect();
    std::fs::write(&config, contents).unwrap();

    for options in [
        vec![],
        vec!["--output", "json"],
        vec!["--output", "table"],
        vec!["--raw"],
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_signoz-cli"))
            .args(options)
            .args(["config", "show"])
            .env("SIGNOZ_CONFIG", &config)
            .env_remove("SIGNOZ_URL")
            .env_remove("SIGNOZ_API_KEY")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
    }
}
