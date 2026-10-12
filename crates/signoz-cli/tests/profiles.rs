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
