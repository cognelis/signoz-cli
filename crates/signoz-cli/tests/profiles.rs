use assert_cmd::Command;
use serde_json::Value;

#[test]
fn profile_show_uses_positional_then_global_then_current_selection() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        r#"
current_profile = "test"
[profiles.test]
url = "https://test.example.com"
[profiles.prod]
url = "https://prod.example.com"
"#,
    )
    .unwrap();

    for (arguments, expected) in [
        (vec!["profile", "show"], "test"),
        (vec!["--profile", "prod", "profile", "show"], "prod"),
        (vec!["profile", "show", "--profile", "prod"], "prod"),
        (vec!["--profile", "prod", "profile", "show", "test"], "test"),
    ] {
        let output = Command::cargo_bin("signoz-cli")
            .unwrap()
            .args(["--output", "json"])
            .args(arguments)
            .env("SIGNOZ_CONFIG", &config)
            .env_remove("SIGNOZ_URL")
            .env_remove("SIGNOZ_API_KEY")
            .assert()
            .success()
            .get_output()
            .clone();
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(envelope["data"]["name"], expected);
        assert_eq!(
            envelope["data"]["url"],
            format!("https://{expected}.example.com")
        );
        assert!(output.stderr.is_empty());
    }

    Command::cargo_bin("signoz-cli")
        .unwrap()
        .args(["--profile", "missing", "profile", "show"])
        .env("SIGNOZ_CONFIG", config)
        .assert()
        .code(3)
        .stdout("")
        .stderr(predicates::str::contains(
            "profile 'missing' does not exist",
        ));
}

#[test]
fn profile_descriptions_are_stored_listed_and_cleared() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    let run = |arguments: &[&str]| {
        Command::cargo_bin("signoz-cli")
            .unwrap()
            .args(["--output", "json"])
            .args(arguments)
            .env("SIGNOZ_CONFIG", &config)
            .env_remove("SIGNOZ_URL")
            .env_remove("SIGNOZ_API_KEY")
            .output()
            .unwrap()
    };
    let json = |output: &std::process::Output| -> Value {
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let output = run(&[
        "profile",
        "add",
        "prod",
        "--url",
        "https://prod.example.com",
        "--description",
        "Production observability",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        json(&output)["data"]["description"],
        "Production observability"
    );
    assert!(
        run(&[
            "profile",
            "add",
            "test",
            "--url",
            "https://test.example.com"
        ])
        .status
        .success()
    );
    let listed = json(&run(&["profile", "list"]))["data"].clone();
    assert_eq!(listed["prod"]["description"], "Production observability");
    assert!(listed["test"].get("description").is_none());

    // A description alone keeps the URL, so the saved API key is not cleared.
    let output = run(&[
        "profile",
        "update",
        "test",
        "--description",
        "Shared test environment",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let updated = json(&output);
    assert_eq!(updated["data"]["url"], "https://test.example.com");
    assert_eq!(updated["data"]["description"], "Shared test environment");
    assert!(
        updated["meta"]["warnings"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
    assert_eq!(
        json(&run(&["profile", "show", "test"]))["data"]["description"],
        "Shared test environment"
    );
    let output = run(&["profile", "update", "test", "--description", ""]);
    assert!(json(&output)["data"].get("description").is_none());
    for arguments in [
        vec!["profile", "update", "test"],
        vec!["profile", "update", "test", "--description", "two\nlines"],
    ] {
        assert_eq!(run(&arguments).status.code(), Some(2));
    }
}

#[test]
fn credential_store_choice_keeps_api_keys_where_configured() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config.toml");
    let credentials = directory.path().join("credentials.toml");
    let command = || {
        let mut command = Command::cargo_bin("signoz-cli").unwrap();
        command
            .env("SIGNOZ_CONFIG", &config)
            .env_remove("SIGNOZ_URL")
            .env_remove("SIGNOZ_API_KEY")
            .env_remove("SIGNOZ_CLI_CREDENTIAL_STORE")
            .args(["--output", "json"]);
        command
    };
    let run = |arguments: &[&str], input: Option<&str>| {
        let mut command = command();
        command
            .env("SIGNOZ_CLI_CREDENTIAL_STORE", "file")
            .args(arguments);
        if let Some(input) = input {
            command.write_stdin(input);
        }
        command.output().unwrap()
    };

    // Keeping API keys in a plain-text file needs a person at a terminal.
    let output = command()
        .args(["config", "set", "credential-store", "file"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "forbidden");
    assert!(!config.exists());

    let output = run(
        &["profile", "add", "prod", "--url", "http://127.0.0.1:9"],
        None,
    );
    assert!(output.status.success());
    let output = run(
        &[
            "--profile",
            "prod",
            "auth",
            "login",
            "--api-key-stdin",
            "--no-verify",
        ],
        Some("file-secret\n"),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        std::fs::read_to_string(&credentials)
            .unwrap()
            .contains("file-secret")
    );
    assert!(
        !std::fs::read_to_string(&config)
            .unwrap()
            .contains("file-secret")
    );

    // doctor names the store the API key came from, even when the server is unreachable.
    let output = run(&["--timeout", "1", "--profile", "prod", "doctor"], None);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let check = report["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "credentials")
        .unwrap()
        .clone();
    assert_eq!(check["status"], "pass");
    assert!(
        check["message"]
            .as_str()
            .unwrap()
            .contains("credentials file"),
        "{check}"
    );

    // The environment setting outranks the config file, and config set says so.
    let output = run(&["config", "set", "credential-store", "keychain"], None);
    assert!(output.status.success());
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["data"]["value"], "keychain");
    assert!(
        envelope["meta"]["warnings"]
            .to_string()
            .contains("SIGNOZ_CLI_CREDENTIAL_STORE")
    );

    let output = run(&["--profile", "prod", "auth", "logout"], None);
    assert!(output.status.success());
    assert!(
        !std::fs::read_to_string(&credentials)
            .unwrap()
            .contains("file-secret")
    );
}
