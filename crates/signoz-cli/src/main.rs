use clap::Parser;

#[derive(Parser)]
#[command(name = "signoz", version, about = "Query SigNoz from the command line")]
struct Cli {}

fn main() {
    let _ = Cli::parse();
}
