use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Clone, Debug, Parser)]
#[command(
    name = "signoz-cli",
    version,
    about = "Query SigNoz from the command line"
)]
pub struct Cli {
    /// Select a saved environment.
    #[arg(long, global = true)]
    pub profile: Option<String>,
    /// Override the instance URL.
    #[arg(long, global = true)]
    pub url: Option<String>,
    /// Select output format.
    ///
    /// Auto uses a human-readable display on a terminal and JSON when stdout is redirected.
    #[arg(long, global = true, value_enum, default_value = "auto")]
    pub output: OutputFormat,
    /// Set the request timeout in seconds.
    #[arg(long, global = true, default_value_t = 60)]
    pub timeout: u64,
    /// Print JSON data only.
    ///
    /// Emit only the JSON data payload, omitting metadata; overrides --output.
    #[arg(long, global = true)]
    pub raw: bool,
    /// Show request diagnostics.
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
    /// Show the CLI version.
    Version,
    /// Generate shell completions.
    ///
    /// Print a shell completion script to stdout (always shell code, never a JSON envelope).
    #[command(
        after_help = "zsh, current session:\n  autoload -Uz compinit && compinit\n  source <(signoz-cli completion zsh)\n\nRegenerate saved scripts after upgrading the CLI. Completion covers commands, options, enum values, and file paths; profile names and remote resources are not dynamically queried."
    )]
    Completion {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Set up an environment.
    ///
    /// Configure a SigNoz profile, authenticate, and install agent Skills.
    Init,
    /// Check connection and credentials.
    ///
    /// Diagnose local configuration, credentials, connectivity, and authentication.
    Doctor,
    /// View local configuration.
    ///
    /// Inspect the local configuration without exposing credentials.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Install coding-agent skills.
    Skills {
        #[command(subcommand)]
        command: SkillsCommand,
    },
    /// Manage API-key authentication.
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// Manage SigNoz environments.
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Explore and query metrics.
    Metrics {
        #[command(subcommand)]
        command: MetricsCommand,
    },
    /// Discover query fields.
    Fields {
        #[command(subcommand)]
        command: FieldsCommand,
    },
    /// Search and aggregate logs.
    Logs {
        #[command(subcommand)]
        command: LogsCommand,
    },
    /// Search and inspect traces.
    Traces {
        #[command(subcommand)]
        command: TracesCommand,
    },
    /// Inspect services and dependencies.
    Services {
        #[command(subcommand)]
        command: ServicesCommand,
    },
    /// Inspect application exceptions.
    ///
    /// Inspect grouped exceptions and their trace-linked occurrences.
    Exceptions {
        #[command(subcommand)]
        command: ExceptionsCommand,
    },
    /// Inspect hosts and Kubernetes.
    ///
    /// Inspect host and Kubernetes infrastructure inventory.
    #[command(visible_alias = "infra")]
    Infrastructure {
        #[command(subcommand)]
        command: InfrastructureCommand,
    },
    /// Inspect ingestion usage.
    ///
    /// Query Cost Meter ingestion metrics.
    Meters {
        #[command(subcommand)]
        command: MetersCommand,
    },
    /// List alerts.
    Alerts {
        #[command(subcommand)]
        command: AlertsCommand,
    },
    /// Inspect alert rules and history.
    Rules {
        #[command(subcommand)]
        command: RulesCommand,
    },
    /// Browse dashboards.
    Dashboards {
        #[command(subcommand)]
        command: DashboardsCommand,
    },
    /// Browse saved explorer views.
    Views {
        #[command(subcommand)]
        command: ViewsCommand,
    },
    /// Inspect notification channels.
    Channels {
        #[command(subcommand)]
        command: ResourceCommand,
    },
    /// Run or preview query payloads.
    Query {
        #[command(subcommand)]
        command: QueryCommand,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum SkillsCommand {
    /// Install or update agent skills.
    Install {
        /// Choose which agents receive the skill.
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
    /// Store an API key.
    Login {
        /// Read the API key from stdin.
        ///
        /// Read the API key from standard input instead of a masked prompt.
        #[arg(long)]
        api_key_stdin: bool,
        /// Store the key without verification.
        ///
        /// Store the API key without verifying it against SigNoz.
        #[arg(long)]
        no_verify: bool,
    },
    /// Check the authenticated identity.
    Check,
    /// Remove a saved API key.
    Logout,
}

#[derive(Clone, Debug, Subcommand)]
pub enum ProfileCommand {
    /// Add an environment.
    Add {
        name: String,
        /// Set the instance URL.
        #[arg(long)]
        url: String,
    },
    /// Change an environment URL.
    Update {
        name: String,
        /// Set the instance URL.
        #[arg(long)]
        url: String,
    },
    /// List saved environments.
    List,
    /// Show an environment.
    Show { name: Option<String> },
    /// Set the default environment.
    Use { name: String },
    /// Remove an environment.
    Remove { name: String },
}

#[derive(Clone, Debug, Subcommand)]
pub enum ConfigCommand {
    /// Show the config file path.
    ///
    /// Print the operating-system configuration file path.
    Path,
    /// Show configuration settings.
    ///
    /// Show the configuration with credentials omitted.
    Show,
}

#[derive(Clone, Debug, Subcommand)]
pub enum MetricsCommand {
    /// List metric names.
    List {
        /// Search for text.
        #[arg(long)]
        search: Option<String>,
        /// Select a metric source.
        #[arg(long, value_enum)]
        source: Option<MetricSource>,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: LimitArgs,
    },
    /// Query a metric.
    Query {
        name: String,
        /// Choose an aggregation.
        ///
        /// Override the metadata-derived spatial aggregation.
        #[arg(long)]
        aggregation: Option<String>,
        /// Filter with a query expression.
        #[arg(long)]
        filter: Option<String>,
        /// Group results by this field.
        #[arg(long = "group-by")]
        group_by: Vec<String>,
        /// Set the time interval in seconds.
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u32).range(1..=2_592_000))]
        step: u32,
        /// Select a metric source.
        #[arg(long, value_enum)]
        source: Option<MetricSource>,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
    /// Rank metrics by sample volume.
    Top {
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: LimitArgs,
    },
    /// Inspect metric cardinality.
    Cardinality {
        name: String,
        #[command(flatten)]
        time: TimeArgs,
    },
    /// Find dashboards and alerts using a metric.
    Usage { name: String },
    /// Show metric metadata.
    Metadata { name: String },
    /// Show metric highlights.
    Highlights { name: String },
    /// Inspect a metric (up to 30 minutes).
    ///
    /// Inspect a metric over at most 30 minutes (default: --since 30m).
    Inspect {
        name: String,
        /// Filter with a query expression.
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
    /// List available field names.
    Keys {
        /// Select logs, traces, or metrics.
        #[arg(long, value_enum)]
        signal: Signal,
        #[command(flatten)]
        query: FieldQueryArgs,
    },
    /// Find values for a field.
    Values {
        /// Select logs, traces, or metrics.
        #[arg(long, value_enum)]
        signal: Signal,
        key: String,
        /// Constrain suggested values.
        ///
        /// Existing Query Builder filter used to constrain suggested values.
        #[arg(long)]
        existing_query: Option<String>,
        #[command(flatten)]
        query: FieldQueryArgs,
    },
}

#[derive(Clone, Debug, Args)]
pub struct FieldQueryArgs {
    /// Select a metric source.
    #[arg(long, value_enum)]
    pub source: Option<MetricSource>,
    /// Select the field context.
    #[arg(long, value_enum)]
    pub context: Option<FieldContext>,
    /// Select the field data type.
    #[arg(long = "type", value_enum)]
    pub data_type: Option<FieldDataType>,
    /// Filter by metric name.
    #[arg(long)]
    pub metric_name: Option<String>,
    /// Filter by metric namespace.
    #[arg(long)]
    pub metric_namespace: Option<String>,
    /// Search for text.
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
    /// Search log records.
    Search {
        /// Search for text.
        #[arg(long)]
        search: Option<String>,
        /// Filter with a query expression.
        #[arg(long)]
        filter: Option<String>,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
    /// Aggregate log records.
    Aggregate {
        /// Filter with a query expression.
        #[arg(long)]
        filter: Option<String>,
        /// Choose an aggregation.
        #[arg(long, value_enum, default_value = "count")]
        aggregation: AggregateFunction,
        /// Aggregate this field.
        #[arg(long)]
        aggregate_on: Option<String>,
        /// Group results by this field.
        #[arg(long = "group-by")]
        group_by: Vec<String>,
        /// Set the time interval in seconds.
        ///
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
    /// Search trace spans.
    Search {
        /// Filter with a query expression.
        #[arg(long)]
        filter: Option<String>,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
    /// Aggregate trace spans.
    Aggregate {
        /// Filter with a query expression.
        #[arg(long)]
        filter: Option<String>,
        /// Choose an aggregation.
        #[arg(long, value_enum, default_value = "count")]
        aggregation: AggregateFunction,
        /// Aggregate this field.
        #[arg(long)]
        aggregate_on: Option<String>,
        /// Group results by this field.
        #[arg(long = "group-by")]
        group_by: Vec<String>,
        /// Set the time interval in seconds.
        ///
        /// Return a time series at this interval in seconds instead of a scalar/table result.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=2_592_000))]
        step: Option<u32>,
        #[command(flatten)]
        page: LimitArgs,
        #[command(flatten)]
        time: TimeArgs,
    },
    /// Inspect a trace.
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
    /// List observed services.
    List {
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
    /// List operations for a service.
    Operations {
        service: String,
        #[command(flatten)]
        time: TimeArgs,
    },
    /// Show service dependencies.
    ///
    /// List service dependency edges using SigNoz's compatibility API.
    Dependencies {
        #[command(flatten)]
        time: TimeArgs,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum ExceptionsCommand {
    /// List exception groups.
    List {
        /// Filter by service.
        #[arg(long)]
        service: Option<String>,
        /// Filter by exception type.
        #[arg(long)]
        exception_type: Option<String>,
        /// Set sort direction.
        #[arg(long, value_enum, default_value = "descending")]
        order: ExceptionOrder,
        /// Choose the sort field.
        #[arg(long, value_enum, default_value = "last-seen")]
        order_by: ExceptionOrderBy,
        #[command(flatten)]
        time: TimeArgs,
        #[command(flatten)]
        page: PaginationArgs,
    },
    /// Count exceptions.
    Count {
        /// Filter by service.
        #[arg(long)]
        service: Option<String>,
        /// Filter by exception type.
        #[arg(long)]
        exception_type: Option<String>,
        #[command(flatten)]
        time: TimeArgs,
    },
    /// Inspect an exception group.
    Group {
        group_id: String,
        /// Set the exception timestamp.
        #[arg(long)]
        timestamp: String,
    },
    /// Inspect an exception occurrence.
    Get {
        group_id: String,
        error_id: String,
        /// Set the exception timestamp.
        #[arg(long)]
        timestamp: String,
    },
    /// Find adjacent exception occurrences.
    Navigate {
        group_id: String,
        error_id: String,
        /// Set the exception timestamp.
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
    /// Check infrastructure availability.
    Check {
        #[arg(value_enum)]
        resource: InfrastructureCheckResource,
    },
    /// List infrastructure resources.
    List {
        #[arg(value_enum)]
        resource: InfrastructureResource,
        /// Filter with a query expression.
        #[arg(long)]
        filter: Option<String>,
        /// Filter infrastructure status.
        #[arg(long, value_enum)]
        status: Option<InfrastructureStatus>,
        /// Group results by this field.
        #[arg(long = "group-by")]
        group_by: Vec<String>,
        /// Choose the sort field.
        #[arg(long)]
        order_by: Option<String>,
        /// Set sort direction.
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
    /// Summarize ingestion usage.
    Summary {
        /// Filter with a query expression.
        #[arg(long)]
        filter: Option<String>,
        /// Group results by this field.
        #[arg(long = "group-by")]
        group_by: Vec<String>,
        /// Set the time interval in seconds.
        #[arg(long, default_value_t = 86_400, value_parser = clap::value_parser!(u32).range(3_600..=2_592_000))]
        step: u32,
        #[command(flatten)]
        time: TimeArgs,
    },
    /// Query an ingestion metric.
    Query {
        metric: String,
        /// Filter with a query expression.
        #[arg(long)]
        filter: Option<String>,
        /// Group results by this field.
        #[arg(long = "group-by")]
        group_by: Vec<String>,
        /// Set the time interval in seconds.
        #[arg(long, default_value_t = 86_400, value_parser = clap::value_parser!(u32).range(3_600..=2_592_000))]
        step: u32,
        #[command(flatten)]
        time: TimeArgs,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum AlertsCommand {
    /// List alerts.
    List {
        #[command(flatten)]
        page: PaginationArgs,
    },
}

#[derive(Clone, Debug, Subcommand)]
pub enum RulesCommand {
    /// List alert rules.
    List {
        #[command(flatten)]
        page: PaginationArgs,
    },
    /// Show an alert rule.
    Get { id: String },
    /// List alert state changes.
    History {
        id: String,
        #[command(flatten)]
        time: TimeArgs,
        /// Limit the number of results.
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=10_000))]
        limit: u32,
        /// Continue from a pagination cursor.
        #[arg(long)]
        cursor: Option<String>,
        /// Set sort direction.
        #[arg(long, value_enum, default_value = "asc")]
        order: SortOrder,
        /// Filter alert state.
        #[arg(long, value_enum)]
        state: Option<AlertState>,
        /// Filter with a query expression.
        #[arg(long)]
        filter: Option<String>,
    },
    /// Summarize alert history.
    HistoryStats {
        id: String,
        #[command(flatten)]
        time: TimeArgs,
    },
    /// Show overall alert status.
    HistoryStatus {
        id: String,
        #[command(flatten)]
        time: TimeArgs,
    },
    /// Show top alert contributors.
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
    /// List resources.
    List {
        #[command(flatten)]
        page: PaginationArgs,
    },
    /// Show a resource.
    Get { id: String },
}

#[derive(Clone, Debug, Subcommand)]
pub enum DashboardsCommand {
    /// List dashboards.
    List {
        /// Filter dashboards.
        #[arg(long)]
        query: Option<String>,
        /// Choose the sort field.
        #[arg(long, value_enum, default_value = "updated-at")]
        sort: DashboardSort,
        /// Set sort direction.
        #[arg(long, value_enum, default_value = "desc")]
        order: SortOrder,
        #[command(flatten)]
        page: PaginationArgs,
    },
    /// Show a dashboard.
    Get { id: String },
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
    /// List saved views.
    List {
        /// Select the saved-view source.
        #[arg(long, value_enum)]
        source_page: ViewSource,
        #[command(flatten)]
        page: PaginationArgs,
    },
    /// Show a saved view.
    Get { id: String },
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
    /// Run a query payload.
    Run {
        /// Read query JSON from a file or stdin (-).
        #[arg(long, default_value = "-", value_hint = clap::ValueHint::FilePath)]
        file: PathBuf,
    },
    /// Preview and validate a query.
    Preview {
        /// Read query JSON from a file or stdin (-).
        #[arg(long, default_value = "-", value_hint = clap::ValueHint::FilePath)]
        file: PathBuf,
        /// Include SQL and query estimates.
        ///
        /// Include rendered SQL and ClickHouse estimates in the server response.
        #[arg(long)]
        explain: bool,
    },
}

#[derive(Clone, Debug, Args)]
pub struct TimeArgs {
    /// Look back by a duration, such as 30m.
    #[arg(long, conflicts_with_all = ["start", "end"])]
    pub since: Option<String>,
    /// Set start time (RFC3339 or @unix-ms).
    #[arg(long, requires = "end")]
    pub start: Option<String>,
    /// Set end time (RFC3339 or @unix-ms).
    #[arg(long, requires = "start")]
    pub end: Option<String>,
}

impl TimeArgs {
    pub fn as_client_args(&self) -> signoz_client::TimeWindowArgs {
        self.as_client_args_with_default("1h")
    }

    pub(crate) fn as_client_args_with_default(&self, since: &str) -> signoz_client::TimeWindowArgs {
        if self.since.is_none() && self.start.is_none() && self.end.is_none() {
            signoz_client::TimeWindowArgs::since(since)
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
    /// Limit the number of results.
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=10_000))]
    pub limit: u32,
}

#[derive(Clone, Debug, Args)]
pub struct PaginationArgs {
    /// Limit the number of results.
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=10_000))]
    pub limit: u32,
    /// Skip this many results.
    #[arg(long, default_value_t = 0)]
    pub offset: u32,
}
