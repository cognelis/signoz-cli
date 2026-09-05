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
