use clap::Parser;
use signoz_cli::args::Cli;
use signoz_cli::dispatch;
use signoz_cli::output::{emit_error, emit_success, exit_code};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    match dispatch::run(&cli).await {
        Ok(result) => {
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
                );
                std::process::exit(7);
            }
        }
        Err(error) => {
            emit_error(
                cli.output,
                cli.raw,
                error.code,
                &error.message,
                error.hint.as_deref(),
            );
            std::process::exit(exit_code(error.code));
        }
    }
}
