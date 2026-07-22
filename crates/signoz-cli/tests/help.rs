use clap::CommandFactory;
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
        "doctor",
        "skills",
        "auth",
        "profile",
        "metrics",
        "fields",
        "logs",
        "traces",
        "services",
        "alerts",
        "rules",
        "dashboards",
        "views",
        "channels",
        "query",
    ] {
        assert!(names.contains(&expected), "missing command {expected}");
    }
}
