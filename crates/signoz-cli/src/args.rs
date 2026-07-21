use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Clone, Debug, Parser)]
#[command(
    name = "signoz-cli",
    version,
    about = "Query SigNoz from the command line"
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub profile: Option<String>,
    #[arg(long, global = true)]
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
        command: ViewsCommand,
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
    Add {
        name: String,
        #[arg(long)]
        url: String,
    },
    List,
    Show {
        name: Option<String>,
    },
    Use {
        name: String,
    },
    Remove {
        name: String,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum MetricsCommand {
    List {
        #[arg(long)]
        search: Option<String>,
        #[arg(long, value_enum)]
        source: Option<MetricSource>,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: LimitArgs,
    },
    Query {
        name: String,
        /// Override the metadata-derived spatial aggregation.
        #[arg(long)]
        aggregation: Option<String>,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
    Top {
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: LimitArgs,
    },
    Cardinality {
        name: String,
        #[command(flatten)]
        time: TimeArgs,
    },
    Usage {
        name: String,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum MetricSource {
    Meter,
}

impl MetricSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Meter => "meter",
        }
    }
}

#[derive(Clone, Debug, Subcommand)]
pub enum FieldsCommand {
    Keys {
        #[arg(long, value_enum)]
        signal: Signal,
    },
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
        #[arg(long, value_enum, default_value = "count")]
        aggregation: AggregateFunction,
        #[arg(long)]
        aggregate_on: Option<String>,
        #[command(flatten)]
        time: TimeArgs,
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
        #[arg(long, value_enum, default_value = "count")]
        aggregation: AggregateFunction,
        #[arg(long)]
        aggregate_on: Option<String>,
        #[command(flatten)]
        time: TimeArgs,
    },
    Get {
        trace_id: String,
        #[command(flatten)]
        time: TimeArgs,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum AggregateFunction {
    Count,
    #[value(name = "count_distinct")]
    CountDistinct,
    Avg,
    Sum,
    Min,
    Max,
    P50,
    P75,
    P90,
    P95,
    P99,
    Rate,
}

impl AggregateFunction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::CountDistinct => "count_distinct",
            Self::Avg => "avg",
            Self::Sum => "sum",
            Self::Min => "min",
            Self::Max => "max",
            Self::P50 => "p50",
            Self::P75 => "p75",
            Self::P90 => "p90",
            Self::P95 => "p95",
            Self::P99 => "p99",
            Self::Rate => "rate",
        }
    }

    pub fn requires_field(self) -> bool {
        !matches!(self, Self::Count | Self::Rate)
    }
}

#[derive(Clone, Debug, Subcommand)]
pub enum ServicesCommand {
    List {
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
    Operations {
        service: String,
        #[command(flatten)]
        time: TimeArgs,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum AlertsCommand {
    List {
        #[command(flatten)]
        page: PaginationArgs,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum RulesCommand {
    List {
        #[command(flatten)]
        page: PaginationArgs,
    },
    Get {
        id: String,
    },
    History {
        id: String,
        #[command(flatten)]
        time: TimeArgs,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=10_000))]
        limit: u32,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long, value_enum, default_value = "asc")]
        order: SortOrder,
        #[arg(long, value_enum)]
        state: Option<AlertState>,
        #[arg(long)]
        filter: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum AlertState {
    Inactive,
    Pending,
    Recovering,
    Firing,
    Nodata,
    Disabled,
}

impl AlertState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Pending => "pending",
            Self::Recovering => "recovering",
            Self::Firing => "firing",
            Self::Nodata => "nodata",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Clone, Debug, Subcommand)]
pub enum ResourceCommand {
    List {
        #[command(flatten)]
        page: PaginationArgs,
    },
    Get {
        id: String,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum ViewsCommand {
    List {
        #[arg(long, value_enum)]
        source_page: ViewSource,
        #[command(flatten)]
        page: PaginationArgs,
    },
    Get {
        id: String,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ViewSource {
    Traces,
    Logs,
    Metrics,
    Meter,
}

impl ViewSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Traces => "traces",
            Self::Logs => "logs",
            Self::Metrics => "metrics",
            Self::Meter => "meter",
        }
    }
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
pub struct LimitArgs {
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=10_000))]
    pub limit: u32,
}

#[derive(Clone, Debug, Args)]
pub struct PaginationArgs {
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=10_000))]
    pub limit: u32,
    #[arg(long, default_value_t = 0)]
    pub offset: u32,
}
