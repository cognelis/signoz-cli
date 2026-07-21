use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Clone, Debug, Parser)]
#[command(name = "signoz", version, about = "Query SigNoz from the command line")]
pub struct Cli {
    #[arg(long, global = true)]
    pub profile: Option<String>,
    #[arg(long, global = true, env = "SIGNOZ_URL")]
    pub url: Option<String>,
    #[arg(long, global = true, value_enum, default_value = "auto")]
    pub output: OutputFormat,
    #[arg(long, global = true, default_value_t = 60)]
    pub timeout: u64,
    #[arg(long, global = true)]
    pub raw: bool,
    #[arg(long, global = true)]
    pub verbose: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Auto,
    Json,
    Table,
}

#[derive(Clone, Debug, Subcommand)]
pub enum Command {
    Version,
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    Metrics {
        #[command(subcommand)]
        command: MetricsCommand,
    },
    Fields {
        #[command(subcommand)]
        command: FieldsCommand,
    },
    Logs {
        #[command(subcommand)]
        command: LogsCommand,
    },
    Traces {
        #[command(subcommand)]
        command: TracesCommand,
    },
    Services {
        #[command(subcommand)]
        command: ServicesCommand,
    },
    Alerts {
        #[command(subcommand)]
        command: AlertsCommand,
    },
    Rules {
        #[command(subcommand)]
        command: RulesCommand,
    },
    Dashboards {
        #[command(subcommand)]
        command: ResourceCommand,
    },
    Views {
        #[command(subcommand)]
        command: ResourceCommand,
    },
    Channels {
        #[command(subcommand)]
        command: ResourceCommand,
    },
    Query {
        #[command(subcommand)]
        command: QueryCommand,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum AuthCommand {
    Login {
        #[arg(long, required = true)]
        api_key_stdin: bool,
    },
    Check,
    Logout,
}

#[derive(Clone, Debug, Subcommand)]
pub enum ProfileCommand {
    Add { name: String, #[arg(long)] url: String },
    List,
    Show { name: Option<String> },
    Use { name: String },
    Remove { name: String },
}

#[derive(Clone, Debug, Subcommand)]
pub enum MetricsCommand {
    List,
    Query {
        name: String,
        #[arg(long, default_value = "avg")]
        aggregation: String,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
    Top {
        name: String,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
    Cardinality {
        name: String,
        #[command(flatten)]
        time: TimeArgs,
    },
    Usage { name: String },
}

#[derive(Clone, Debug, Subcommand)]
pub enum FieldsCommand {
    Keys { #[arg(long, value_enum)] signal: Signal },
    Values {
        #[arg(long, value_enum)]
        signal: Signal,
        key: String,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Signal {
    Metrics,
    Logs,
    Traces,
}

impl Signal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Metrics => "metrics",
            Self::Logs => "logs",
            Self::Traces => "traces",
        }
    }
}

#[derive(Clone, Debug, Subcommand)]
pub enum LogsCommand {
    Search {
        #[arg(long)]
        search: Option<String>,
        #[arg(long)]
        filter: Option<String>,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
    Aggregate {
        #[arg(long)]
        filter: Option<String>,
        #[arg(long, default_value = "count")]
        aggregation: String,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum TracesCommand {
    Search {
        #[arg(long)]
        filter: Option<String>,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
    Aggregate {
        #[arg(long)]
        filter: Option<String>,
        #[arg(long, default_value = "count")]
        aggregation: String,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
    Get {
        trace_id: String,
        #[command(flatten)]
        time: TimeArgs,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum ServicesCommand {
    List {
        #[command(flatten)]
        time: TimeArgs,
    },
    Operations {
        service: String,
        #[command(flatten)]
        time: TimeArgs,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum AlertsCommand {
    List,
}

#[derive(Clone, Debug, Subcommand)]
pub enum RulesCommand {
    List,
    Get { id: String },
    History {
        id: String,
        #[command(flatten)]
        time: TimeArgs,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum ResourceCommand {
    List,
    Get { id: String },
}

#[derive(Clone, Debug, Subcommand)]
pub enum QueryCommand {
    Run {
        #[arg(long, default_value = "-")]
        file: PathBuf,
    },
}

#[derive(Clone, Debug, Args)]
pub struct TimeArgs {
    #[arg(long, conflicts_with_all = ["start", "end"])]
    pub since: Option<String>,
    #[arg(long, requires = "end")]
    pub start: Option<String>,
    #[arg(long, requires = "start")]
    pub end: Option<String>,
}

impl TimeArgs {
    pub fn as_client_args(&self) -> signoz_client::TimeWindowArgs {
        if self.since.is_none() && self.start.is_none() && self.end.is_none() {
            signoz_client::TimeWindowArgs::since("1h")
        } else {
            signoz_client::TimeWindowArgs::from_parts(
                self.since.as_deref(),
                self.start.as_deref(),
                self.end.as_deref(),
            )
        }
    }
}

#[derive(Clone, Debug, Args)]
pub struct PaginationArgs {
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=10_000))]
    pub limit: u32,
}
