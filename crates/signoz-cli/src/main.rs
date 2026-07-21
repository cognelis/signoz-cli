use clap::{Parser, error::ErrorKind};
use signoz_cli::args::{Cli, OutputFormat};
use signoz_cli::dispatch;
use signoz_cli::output::{emit_error, emit_success, exit_code};

#[tokio::main]
async fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return;
        }
        Err(error) => {
            let arguments: Vec<_> = std::env::args().collect();
            emit_error(
                requested_output_format(&arguments),
                arguments.iter().any(|argument| argument == "--raw"),
                signoz_client::ErrorCode::Validation,
                &error.to_string(),
                None,
                Some("Run `signoz-cli --help` or the subcommand's `--help`"),
            );
            std::process::exit(2);
        }
    };
    match dispatch::run(&cli).await {
        Ok(result) => {
            let failure_code = result.failure_code;
            if cli.verbose {
                eprintln!(
                    "signoz-cli: command={} status={} elapsedMs={}",
                    result.command,
                    result.meta.status.unwrap_or_default(),
                    result.meta.elapsed_ms.unwrap_or_default()
                );
            }
            if let Err(message) = emit_success(
                cli.output,
                cli.raw,
                &result.command,
                result.data,
                result.meta,
            ) {
                emit_error(
                    cli.output,
                    cli.raw,
                    signoz_client::ErrorCode::IncompatibleResponse,
                    &message,
                    None,
                    None,
                );
                std::process::exit(7);
            }
            if let Some(code) = failure_code {
                std::process::exit(exit_code(code));
            }
        }
        Err(error) => {
            emit_error(
                cli.output,
                cli.raw,
                error.code,
                &error.message,
                error.status,
                error.hint.as_deref(),
            );
            std::process::exit(exit_code(error.code));
        }
    }
}

fn requested_output_format(arguments: &[String]) -> OutputFormat {
    for (index, argument) in arguments.iter().enumerate() {
        let value = argument.strip_prefix("--output=").or_else(|| {
            (argument == "--output")
                .then(|| arguments.get(index + 1))
                .flatten()
                .map(String::as_str)
        });
        match value {
            Some("json") => return OutputFormat::Json,
            Some("table") => return OutputFormat::Table,
            Some("auto") => return OutputFormat::Auto,
            _ => {}
        }
    }
    OutputFormat::Auto
}
