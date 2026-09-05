use std::process::{Command as ProcessCommand, Stdio};
use std::time::Duration;

use assert_cmd::Command;
use clap::{CommandFactory, ValueEnum};
use clap_complete::Shell;
use signoz_cli::args::Cli;

#[test]
fn completion_menus_have_short_descriptions_for_every_candidate() {
    fn check(command: &clap::Command) {
        for subcommand in command.get_subcommands() {
            let description = subcommand
                .get_about()
                .map(ToString::to_string)
                .unwrap_or_default();
            assert!(
                !description.trim().is_empty(),
                "{} has no menu description",
                subcommand.get_name()
            );
            check(subcommand);
            assert!(
                description.chars().count() <= 48 && !description.contains('\n'),
                "{} description is too long for the menu: {description}",
                subcommand.get_name()
            );
        }
        for argument in command
            .get_arguments()
            .filter(|arg| arg.get_long().is_some())
        {
            let description = argument
                .get_help()
                .map(ToString::to_string)
                .unwrap_or_default();
            assert!(
                !description.trim().is_empty(),
                "--{} has no menu description",
                argument.get_long().unwrap()
            );
            assert!(
                description.chars().count() <= 48,
                "--{} description is too long: {description}",
                argument.get_long().unwrap()
            );
        }
    }
    check(&Cli::command());
    let script = signoz_cli::completion::generate(Shell::Zsh);
    assert!(script.contains("help:Show command help"));
    assert!(!script.contains("Print this message or the help"));
}

#[test]
fn every_shell_generates_code_without_loading_configuration_or_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("broken.toml");
    let original = "do-not-read-this-configuration = [";
    std::fs::write(&config, original).unwrap();
    for shell in Shell::value_variants() {
        let output = Command::cargo_bin("signoz-cli")
            .unwrap()
            .args(["completion", &shell.to_string()])
            .env("SIGNOZ_CONFIG", &config)
            .env("SIGNOZ_URL", "not-a-valid-url")
            .env("SIGNOZ_API_KEY", "must-not-appear-in-completion")
            .timeout(Duration::from_secs(5))
            .assert()
            .success()
            .stderr("")
            .get_output()
            .clone();
        let script = String::from_utf8(output.stdout).unwrap();
        assert!(script.contains("signoz-cli"), "empty script for {shell}");
        assert!(script.contains("completion"));
        assert!(!script.contains("schemaVersion"));
        assert!(!script.contains("must-not-appear"));
        assert!(!script.contains("do-not-read-this-configuration"));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), original);
    }
}

#[test]
fn zsh_script_includes_command_tree_enums_aliases_and_file_hints() {
    let script = signoz_cli::completion::generate(Shell::Zsh);
    assert!(script.starts_with("#compdef signoz-cli\n"));
    for command in Cli::command().get_subcommands() {
        assert!(
            script.contains(command.get_name()),
            "missing {}",
            command.get_name()
        );
    }
    for expected in [
        "(auto json table)",
        "(all codex claude)",
        "(meter)",
        "_files",
    ] {
        assert!(script.contains(expected), "missing {expected}");
    }
    assert!(
        script
            .lines()
            .any(|line| line.contains("--file=") && line.contains("_files"))
    );
    assert!(
        script
            .lines()
            .filter(|line| line.contains("--profile="))
            .all(|line| !line.contains("_files") && !line.contains("_default"))
    );
}

#[test]
fn successful_completion_is_always_plain_shell_code() {
    let baseline = Command::cargo_bin("signoz-cli")
        .unwrap()
        .args(["completion", "zsh"])
        .output()
        .unwrap();
    assert!(baseline.status.success());
    for options in [
        vec!["--output", "json"],
        vec!["--output", "table"],
        vec!["--raw"],
        vec!["--verbose", "--profile", "missing"],
    ] {
        let output = Command::cargo_bin("signoz-cli")
            .unwrap()
            .args(options)
            .args(["completion", "zsh"])
            .assert()
            .success()
            .get_output()
            .clone();
        assert_eq!(output.stdout, baseline.stdout);
    }
}

#[test]
fn unsupported_or_missing_shell_is_a_structured_usage_error() {
    for args in [vec!["completion"], vec!["completion", "unknown-shell"]] {
        let output = Command::cargo_bin("signoz-cli")
            .unwrap()
            .args(["--output", "json"])
            .args(args)
            .assert()
            .code(2)
            .stdout("")
            .get_output()
            .clone();
        let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "validation");
    }
}

#[test]
fn closing_the_completion_output_pipe_does_not_panic() {
    let mut child = ProcessCommand::new(env!("CARGO_BIN_EXE_signoz-cli"))
        .args(["completion", "zsh"])
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

#[cfg(unix)]
#[test]
fn generated_scripts_pass_shell_syntax_checks_when_shells_are_available() {
    let directory = tempfile::tempdir().unwrap();
    for (shell, executable) in [(Shell::Zsh, "zsh"), (Shell::Bash, "bash")] {
        let script = directory.path().join(executable);
        std::fs::write(&script, signoz_cli::completion::generate(shell)).unwrap();
        match ProcessCommand::new(executable)
            .arg("-n")
            .arg(&script)
            .output()
        {
            Ok(output) => assert!(
                output.status.success(),
                "{executable}: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("{executable} unavailable; syntax check skipped")
            }
            Err(error) => panic!("{error}"),
        }
    }
}

#[cfg(unix)]
#[test]
fn bash_completes_nested_commands_flags_and_enum_values() {
    if ProcessCommand::new("bash")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let script = directory.path().join("completion.bash");
    std::fs::write(&script, signoz_cli::completion::generate(Shell::Bash)).unwrap();
    for (words, current, previous, expected) in [
        (vec!["signoz-cli", "metr"], "metr", "signoz-cli", "metrics"),
        (vec!["signoz-cli", "logs", "se"], "se", "logs", "search"),
        (vec!["signoz-cli", "infra", "li"], "li", "infra", "list"),
        (
            vec!["signoz-cli", "--out"],
            "--out",
            "signoz-cli",
            "--output",
        ),
        (vec!["signoz-cli", "--output", "j"], "j", "--output", "json"),
    ] {
        let output = ProcessCommand::new("bash")
            .args([
                "--noprofile",
                "--norc",
                "-c",
                r#"
source "$1"
current=$2
previous=$3
shift 3
COMP_WORDS=("$@")
COMP_CWORD=$((${#COMP_WORDS[@]} - 1))
_signoz__cli signoz-cli "$current" "$previous"
printf '%s\n' "${COMPREPLY[@]}"
"#,
                "completion-test",
            ])
            .arg(&script)
            .args([current, previous])
            .args(words)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let candidates = String::from_utf8(output.stdout).unwrap();
        assert!(
            candidates.lines().any(|line| line == expected),
            "expected {expected}, received {candidates:?}"
        );
    }
}
