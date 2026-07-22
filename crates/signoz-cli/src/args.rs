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
    /// Configure a SigNoz profile, authenticate, and install agent Skills.
    Init,
    /// Diagnose local configuration, credentials, connectivity, and authentication.
    Doctor,
    /// Inspect the local configuration without exposing credentials.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Skills {
        #[command(subcommand)]
        command: SkillsCommand,
    },
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
    /// Inspect grouped exceptions and their trace-linked occurrences.
    Exceptions {
        #[command(subcommand)]
        command: ExceptionsCommand,
    },
    /// Inspect host and Kubernetes infrastructure inventory.
    #[command(visible_alias = "infra")]
    Infrastructure {
        #[command(subcommand)]
        command: InfrastructureCommand,
    },
    /// Query Cost Meter ingestion metrics.
    Meters {
        #[command(subcommand)]
        command: MetersCommand,
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
        command: DashboardsCommand,
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
pub enum SkillsCommand {
    Install {
        #[arg(long, value_enum, default_value_t = SkillTarget::All)]
        target: SkillTarget,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum SkillTarget {
    #[default]
    All,
    Codex,
    Claude,
}

#[derive(Clone, Debug, Subcommand)]
pub enum AuthCommand {
    Login {
        /// Read the API key from standard input instead of a masked prompt.
        #[arg(long)]
        api_key_stdin: bool,
        /// Store the API key without verifying it against SigNoz.
        #[arg(long)]
        no_verify: bool,
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
    Update {
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
pub enum ConfigCommand {
    /// Print the operating-system configuration file path.
    Path,
    /// Show the configuration with credentials omitted.
    Show,
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
        #[arg(long)]
        filter: Option<String>,
        #[arg(long = "group-by")]
        group_by: Vec<String>,
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u32).range(1..=2_592_000))]
        step: u32,
        #[arg(long, value_enum)]
        source: Option<MetricSource>,
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
    Metadata {
        name: String,
    },
    Highlights {
        name: String,
    },
    Inspect {
        name: String,
        #[arg(long)]
        filter: Option<String>,
        #[command(flatten)]
        time: TimeArgs,
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
        #[command(flatten)]
        query: FieldQueryArgs,
    },
    Values {
        #[arg(long, value_enum)]
        signal: Signal,
        key: String,
        /// Existing Query Builder filter used to constrain suggested values.
        #[arg(long)]
        existing_query: Option<String>,
        #[command(flatten)]
        query: FieldQueryArgs,
    },
}

#[derive(Clone, Debug, Args)]
pub struct FieldQueryArgs {
    #[arg(long, value_enum)]
    pub source: Option<MetricSource>,
    #[arg(long, value_enum)]
    pub context: Option<FieldContext>,
    #[arg(long = "type", value_enum)]
    pub data_type: Option<FieldDataType>,
    #[arg(long)]
    pub metric_name: Option<String>,
    #[arg(long)]
    pub metric_namespace: Option<String>,
    #[arg(long)]
    pub search: Option<String>,
    #[command(flatten)]
    pub time: TimeArgs,
    #[command(flatten)]
    pub page: LimitArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum FieldContext {
    Metric,
    Log,
    Span,
    Resource,
    Attribute,
    Body,
}

impl FieldContext {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Metric => "metric",
            Self::Log => "log",
            Self::Span => "span",
            Self::Resource => "resource",
            Self::Attribute => "attribute",
            Self::Body => "body",
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum FieldDataType {
    String,
    Bool,
    Float64,
    Int64,
    Number,
}

impl FieldDataType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Bool => "bool",
            Self::Float64 => "float64",
            Self::Int64 => "int64",
            Self::Number => "number",
        }
    }
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
        #[arg(long = "group-by")]
        group_by: Vec<String>,
        /// Return a time series at this interval in seconds instead of a scalar/table result.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=2_592_000))]
        step: Option<u32>,
        #[command(flatten)]
        page: LimitArgs,
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
        #[arg(long = "group-by")]
        group_by: Vec<String>,
        /// Return a time series at this interval in seconds instead of a scalar/table result.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=2_592_000))]
        step: Option<u32>,
        #[command(flatten)]
        page: LimitArgs,
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
    /// List service dependency edges using SigNoz's compatibility API.
    Dependencies {
        #[command(flatten)]
        time: TimeArgs,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum ExceptionsCommand {
    List {
        #[arg(long)]
        service: Option<String>,
        #[arg(long)]
        exception_type: Option<String>,
        #[arg(long, value_enum, default_value = "descending")]
        order: ExceptionOrder,
        #[arg(long, value_enum, default_value = "last-seen")]
        order_by: ExceptionOrderBy,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
    Count {
        #[arg(long)]
        service: Option<String>,
        #[arg(long)]
        exception_type: Option<String>,
        #[command(flatten)]
        time: TimeArgs,
    },
    Group {
        group_id: String,
        #[arg(long)]
        timestamp: String,
    },
    Get {
        group_id: String,
        error_id: String,
        #[arg(long)]
        timestamp: String,
    },
    Navigate {
        group_id: String,
        error_id: String,
        #[arg(long)]
        timestamp: String,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ExceptionOrder {
    Ascending,
    Descending,
}

impl ExceptionOrder {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ascending => "ascending",
            Self::Descending => "descending",
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ExceptionOrderBy {
    Service,
    Count,
    LastSeen,
    FirstSeen,
    Type,
}

impl ExceptionOrderBy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Service => "serviceName",
            Self::Count => "exceptionCount",
            Self::LastSeen => "lastSeen",
            Self::FirstSeen => "firstSeen",
            Self::Type => "exceptionType",
        }
    }
}

#[derive(Clone, Debug, Subcommand)]
pub enum InfrastructureCommand {
    Check {
        #[arg(value_enum)]
        resource: InfrastructureCheckResource,
    },
    List {
        #[arg(value_enum)]
        resource: InfrastructureResource,
        #[arg(long)]
        filter: Option<String>,
        #[arg(long, value_enum)]
        status: Option<InfrastructureStatus>,
        #[arg(long = "group-by")]
        group_by: Vec<String>,
        #[arg(long)]
        order_by: Option<String>,
        #[arg(long, value_enum, default_value = "desc")]
        order: SortOrder,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum InfrastructureResource {
    Hosts,
    Clusters,
    Nodes,
    Pods,
    Deployments,
    Daemonsets,
    Statefulsets,
    Jobs,
    Namespaces,
    #[value(name = "kube-containers")]
    KubeContainers,
    Pvcs,
}

impl InfrastructureResource {
    pub fn as_path(self) -> &'static str {
        match self {
            Self::Hosts => "hosts",
            Self::Clusters => "clusters",
            Self::Nodes => "nodes",
            Self::Pods => "pods",
            Self::Deployments => "deployments",
            Self::Daemonsets => "daemonsets",
            Self::Statefulsets => "statefulsets",
            Self::Jobs => "jobs",
            Self::Namespaces => "namespaces",
            Self::KubeContainers => "kube_containers",
            Self::Pvcs => "pvcs",
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum InfrastructureCheckResource {
    Hosts,
    Processes,
    Pods,
    Nodes,
    Deployments,
    Daemonsets,
    Statefulsets,
    Jobs,
    Namespaces,
    Clusters,
    Volumes,
    #[value(name = "kube-containers")]
    KubeContainers,
}

impl InfrastructureCheckResource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hosts => "hosts",
            Self::Processes => "processes",
            Self::Pods => "pods",
            Self::Nodes => "nodes",
            Self::Deployments => "deployments",
            Self::Daemonsets => "daemonsets",
            Self::Statefulsets => "statefulsets",
            Self::Jobs => "jobs",
            Self::Namespaces => "namespaces",
            Self::Clusters => "clusters",
            Self::Volumes => "volumes",
            Self::KubeContainers => "kube_containers",
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum InfrastructureStatus {
    Active,
    Inactive,
}

impl InfrastructureStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
        }
    }
}

#[derive(Clone, Debug, Subcommand)]
pub enum MetersCommand {
    Summary {
        #[arg(long)]
        filter: Option<String>,
        #[arg(long = "group-by")]
        group_by: Vec<String>,
        #[arg(long, default_value_t = 86_400, value_parser = clap::value_parser!(u32).range(3_600..=2_592_000))]
        step: u32,
        #[command(flatten)]
        time: TimeArgs,
    },
    Query {
        metric: String,
        #[arg(long)]
        filter: Option<String>,
        #[arg(long = "group-by")]
        group_by: Vec<String>,
        #[arg(long, default_value_t = 86_400, value_parser = clap::value_parser!(u32).range(3_600..=2_592_000))]
        step: u32,
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
    HistoryStats {
        id: String,
        #[command(flatten)]
        time: TimeArgs,
    },
    HistoryStatus {
        id: String,
        #[command(flatten)]
        time: TimeArgs,
    },
    HistoryContributors {
        id: String,
        #[command(flatten)]
        time: TimeArgs,
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
pub enum DashboardsCommand {
    List {
        #[arg(long)]
        query: Option<String>,
        #[arg(long, value_enum, default_value = "updated-at")]
        sort: DashboardSort,
        #[arg(long, value_enum, default_value = "desc")]
        order: SortOrder,
        #[command(flatten)]
        page: PaginationArgs,
    },
    Get {
        id: String,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum DashboardSort {
    UpdatedAt,
    CreatedAt,
    Name,
}

impl DashboardSort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UpdatedAt => "updated_at",
            Self::CreatedAt => "created_at",
            Self::Name => "name",
        }
    }
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
    Preview {
        #[arg(long, default_value = "-")]
        file: PathBuf,
        /// Include rendered SQL and ClickHouse estimates in the server response.
        #[arg(long)]
        explain: bool,
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
