use clap::{CommandFactory, Parser};
use signoz_cli::args::Cli;

#[test]
fn command_tree_is_valid_and_complete() {
    Cli::command().debug_assert();
    let command = Cli::command();
    assert_eq!(command.get_name(), "signoz-cli");
    let names: Vec<_> = command
        .get_subcommands()
        .map(|value| value.get_name())
        .collect();
    for expected in [
        "version",
        "completion",
        "init",
        "doctor",
        "config",
        "skills",
        "auth",
        "profile",
        "metrics",
        "fields",
        "logs",
        "traces",
        "services",
        "exceptions",
        "infrastructure",
        "meters",
        "alerts",
        "rules",
        "dashboards",
        "views",
        "channels",
        "query",
    ] {
        assert!(names.contains(&expected), "missing command {expected}");
    }

    assert!(Cli::try_parse_from(["signoz-cli", "init"]).is_ok());
    assert!(Cli::try_parse_from(["signoz-cli", "auth", "login"]).is_ok());
    assert!(
        Cli::try_parse_from(["signoz-cli", "exceptions", "list", "--service", "checkout",]).is_ok()
    );
    assert!(
        Cli::try_parse_from([
            "signoz-cli",
            "infrastructure",
            "list",
            "hosts",
            "--status",
            "active",
        ])
        .is_ok()
    );
    assert!(Cli::try_parse_from(["signoz-cli", "meters", "summary"]).is_ok());
    assert!(
        Cli::try_parse_from([
            "signoz-cli",
            "metrics",
            "query",
            "requests",
            "--filter",
            "service.name = 'api'",
            "--group-by",
            "service.name",
            "--step",
            "300",
        ])
        .is_ok()
    );
    assert!(
        Cli::try_parse_from([
            "signoz-cli",
            "fields",
            "values",
            "--signal",
            "logs",
            "service.name",
            "--context",
            "resource",
            "--type",
            "string",
            "--search",
            "checkout",
        ])
        .is_ok()
    );
    assert!(
        Cli::try_parse_from([
            "signoz-cli",
            "logs",
            "aggregate",
            "--group-by",
            "service.name",
            "--step",
            "60",
        ])
        .is_ok()
    );
    assert!(
        Cli::try_parse_from(["signoz-cli", "query", "preview", "--file", "query.json"]).is_ok()
    );
    assert!(
        Cli::try_parse_from([
            "signoz-cli",
            "profile",
            "update",
            "production",
            "--url",
            "https://signoz.example.com",
        ])
        .is_ok()
    );
}

#[test]
fn metric_inspect_help_explains_its_default_and_limit() {
    assert_cmd::Command::cargo_bin("signoz-cli")
        .unwrap()
        .args(["metrics", "inspect", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("default: --since 30m"))
        .stdout(predicates::str::contains("at most 30 minutes"));
}
