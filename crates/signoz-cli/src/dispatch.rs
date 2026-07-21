use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::Utc;
use serde_json::{Value, json};
use signoz_client::{
    ClientError, ErrorCode, Operation, SigNozClient, TimeWindow, log_aggregate, log_search,
    metric_query, trace_aggregate, trace_get, trace_search,
};

use crate::args::*;
use crate::config::{
    ConfigFile, CredentialStore, SystemKeyring, default_config_path,
};
use crate::output::Meta;

pub struct DispatchResult {
    pub command: String,
    pub data: Value,
    pub meta: Meta,
}

#[derive(Debug)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub hint: Option<String>,
}

impl AppError {
    fn config(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Config,
            message: message.into(),
            hint: None,
        }
    }

    fn validation(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Validation,
            message: message.into(),
            hint: None,
        }
    }
}

impl From<ClientError> for AppError {
    fn from(error: ClientError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
            hint: error.hint().map(str::to_owned),
        }
    }
}

enum Prepared {
    Local(DispatchResult),
    Operation(&'static str, Operation),
    MetricUsage(String),
}

pub async fn run(cli: &Cli) -> Result<DispatchResult, AppError> {
    let path = config_path()?;
    let mut config = ConfigFile::load(&path).map_err(AppError::config)?;
    let store = SystemKeyring;
    let prepared = prepare(&cli.command, cli.profile.as_deref(), &path, &mut config, &store)?;
    let (command, action) = match prepared {
        Prepared::Local(result) => return Ok(result),
        Prepared::Operation(command, operation) => (command.to_owned(), Remote::Operation(operation)),
        Prepared::MetricUsage(metric) => ("metrics.usage".to_owned(), Remote::MetricUsage(metric)),
    };

    let key = std::env::var("SIGNOZ_API_KEY").ok();
    let connection = config
        .resolve(cli.url.as_deref(), cli.profile.as_deref(), key.as_deref(), &store)
        .map_err(AppError::config)?;
    let profile = connection.profile.clone();
    let client = SigNozClient::new_with_timeout(
        connection.origin,
        connection.credentials,
        Duration::from_secs(cli.timeout.max(1)),
    )?;
    let started = Instant::now();
    let response = match action {
        Remote::Operation(operation) => client.execute(operation).await?,
        Remote::MetricUsage(metric) => client.metric_usage(&metric).await?,
    };
    Ok(DispatchResult {
        command,
        data: response.value,
        meta: Meta {
            profile,
            status: Some(response.meta.status),
            request_id: response.meta.request_id,
            elapsed_ms: Some(started.elapsed().as_millis()),
        },
    })
}

enum Remote {
    Operation(Operation),
    MetricUsage(String),
}

fn prepare(
    command: &Command,
    selected_profile: Option<&str>,
    config_path: &Path,
    config: &mut ConfigFile,
    store: &dyn CredentialStore,
) -> Result<Prepared, AppError> {
    match command {
        Command::Version => local("version", json!({"version": env!("CARGO_PKG_VERSION")})),
        Command::Auth { command } => match command {
            AuthCommand::Check => Ok(Prepared::Operation("auth.check", Operation::auth_me())),
            AuthCommand::Login { api_key_stdin } => {
                if !api_key_stdin {
                    return Err(AppError::validation("use --api-key-stdin"));
                }
                let profile = selected_profile
                    .map(str::to_owned)
                    .or_else(|| config.current_profile.clone())
                    .ok_or_else(|| AppError::config("select a profile before auth login"))?;
                if !config.profiles.contains_key(&profile) {
                    return Err(AppError::config(format!("profile '{profile}' does not exist")));
                }
                let mut key = String::new();
                io::stdin()
                    .read_to_string(&mut key)
                    .map_err(|error| AppError::config(error.to_string()))?;
                let key = key.trim();
                if key.is_empty() {
                    return Err(AppError::validation("API key from stdin is empty"));
                }
                store.set(&profile, key).map_err(AppError::config)?;
                local("auth.login", json!({"profile": profile, "stored": true}))
            }
            AuthCommand::Logout => {
                let profile = selected_profile
                    .map(str::to_owned)
                    .or_else(|| config.current_profile.clone())
                    .ok_or_else(|| AppError::config("select a profile before auth logout"))?;
                store.delete(&profile).map_err(AppError::config)?;
                local("auth.logout", json!({"profile": profile, "removed": true}))
            }
        },
        Command::Profile { command } => match command {
            ProfileCommand::Add { name, url } => {
                config.add_profile(name, url).map_err(AppError::validation)?;
                config.save(config_path).map_err(AppError::config)?;
                local("profile.add", json!({"name": name, "url": url}))
            }
            ProfileCommand::List => local(
                "profile.list",
                serde_json::to_value(&config.profiles).map_err(|error| AppError::config(error.to_string()))?,
            ),
            ProfileCommand::Show { name } => {
                let name = name
                    .clone()
                    .or_else(|| config.current_profile.clone())
                    .ok_or_else(|| AppError::config("no current profile"))?;
                let profile = config
                    .profiles
                    .get(&name)
                    .ok_or_else(|| AppError::config(format!("profile '{name}' does not exist")))?;
                local("profile.show", json!({"name": name, "url": profile.url}))
            }
            ProfileCommand::Use { name } => {
                config.use_profile(name).map_err(AppError::config)?;
                config.save(config_path).map_err(AppError::config)?;
                local("profile.use", json!({"currentProfile": name}))
            }
            ProfileCommand::Remove { name } => {
                config.remove_profile(name, store).map_err(AppError::config)?;
                config.save(config_path).map_err(AppError::config)?;
                local("profile.remove", json!({"name": name, "removed": true}))
            }
        },
        Command::Metrics { command } => match command {
            MetricsCommand::List => operation("metrics.list", Operation::metrics_list()),
            MetricsCommand::Query { name, aggregation, time, page } => operation(
                "metrics.query",
                Operation::query_raw(metric_query(window(time)?, name, aggregation, page.limit)),
            ),
            MetricsCommand::Top { name, time, .. } => {
                let window = window(time)?;
                operation("metrics.top", Operation::metric_treemap(name, window.start_ms, window.end_ms))
            }
            MetricsCommand::Cardinality { name, time } => {
                let _ = window(time)?;
                operation("metrics.cardinality", Operation::metric_attributes(name))
            }
            MetricsCommand::Usage { name } => Ok(Prepared::MetricUsage(name.clone())),
        },
        Command::Fields { command } => match command {
            FieldsCommand::Keys { signal } => operation("fields.keys", Operation::field_keys(signal.as_str())),
            FieldsCommand::Values { signal, key } => operation(
                "fields.values",
                Operation::field_values(signal.as_str(), key),
            ),
        },
        Command::Logs { command } => match command {
            LogsCommand::Search { search, filter, time, page } => {
                let mut body = log_search(window(time)?, search.as_deref(), page.limit);
                merge_filter(&mut body, filter.as_deref());
                operation("logs.search", Operation::query_raw(body))
            }
            LogsCommand::Aggregate { filter, aggregation, time, page } => operation(
                "logs.aggregate",
                Operation::query_raw(log_aggregate(
                    window(time)?,
                    filter.as_deref(),
                    aggregation,
                    page.limit,
                )),
            ),
        },
        Command::Traces { command } => match command {
            TracesCommand::Search { filter, time, page } => operation(
                "traces.search",
                Operation::query_raw(trace_search(window(time)?, filter.as_deref(), page.limit)),
            ),
            TracesCommand::Aggregate { filter, aggregation, time, page } => operation(
                "traces.aggregate",
                Operation::query_raw(trace_aggregate(
                    window(time)?,
                    filter.as_deref(),
                    aggregation,
                    page.limit,
                )),
            ),
            TracesCommand::Get { trace_id, time } => operation(
                "traces.get",
                Operation::query_raw(trace_get(window(time)?, trace_id)),
            ),
        },
        Command::Services { command } => match command {
            ServicesCommand::List { time } => {
                let value = window(time)?;
                operation("services.list", Operation::services_list(value.start_ms, value.end_ms))
            }
            ServicesCommand::Operations { service, time } => {
                let value = window(time)?;
                operation(
                    "services.operations",
                    Operation::service_operations(service, value.start_ms, value.end_ms),
                )
            }
        },
        Command::Alerts { command: AlertsCommand::List } => operation("alerts.list", Operation::alerts_list()),
        Command::Rules { command } => match command {
            RulesCommand::List => operation("rules.list", Operation::rules_list()),
            RulesCommand::Get { id } => operation("rules.get", Operation::rule_get(id)),
            RulesCommand::History { id, time } => {
                let value = window(time)?;
                operation("rules.history", Operation::rule_history(id, value.start_ms, value.end_ms))
            }
        },
        Command::Dashboards { command } => resource(command, "dashboards", Operation::dashboards_list, Operation::dashboard_get),
        Command::Views { command } => resource(command, "views", Operation::views_list, Operation::view_get),
        Command::Channels { command } => resource(command, "channels", Operation::channels_list, Operation::channel_get),
        Command::Query { command: QueryCommand::Run { file } } => {
            let text = read_input(file)?;
            let body = serde_json::from_str(&text)
                .map_err(|error| AppError::validation(format!("invalid query JSON: {error}")))?;
            operation("query.run", Operation::query_raw(body))
        }
    }
}

fn local(command: &str, data: Value) -> Result<Prepared, AppError> {
    Ok(Prepared::Local(DispatchResult {
        command: command.to_owned(),
        data,
        meta: Meta::default(),
    }))
}

fn operation(command: &'static str, value: Operation) -> Result<Prepared, AppError> {
    Ok(Prepared::Operation(command, value))
}

fn window(args: &TimeArgs) -> Result<TimeWindow, AppError> {
    args.as_client_args()
        .resolve(Utc::now())
        .map_err(AppError::validation)
}

fn resource(
    command: &ResourceCommand,
    prefix: &'static str,
    list: fn() -> Operation,
    get: fn(&str) -> Operation,
) -> Result<Prepared, AppError> {
    match command {
        ResourceCommand::List => operation(
            match prefix { "dashboards" => "dashboards.list", "views" => "views.list", _ => "channels.list" },
            list(),
        ),
        ResourceCommand::Get { id } => operation(
            match prefix { "dashboards" => "dashboards.get", "views" => "views.get", _ => "channels.get" },
            get(id),
        ),
    }
}

fn merge_filter(body: &mut Value, explicit: Option<&str>) {
    let Some(explicit) = explicit else { return };
    let expression = &mut body["compositeQuery"]["queries"][0]["builder"]["queryData"][0]["filter"]["expression"];
    let prior = expression.as_str().unwrap_or_default();
    *expression = Value::String(if prior.is_empty() {
        explicit.to_owned()
    } else {
        format!("({prior}) AND ({explicit})")
    });
}

fn read_input(path: &Path) -> Result<String, AppError> {
    if path == Path::new("-") {
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map_err(|error| AppError::validation(error.to_string()))?;
        Ok(text)
    } else {
        fs::read_to_string(path).map_err(|error| AppError::validation(error.to_string()))
    }
}

fn config_path() -> Result<PathBuf, AppError> {
    std::env::var_os("SIGNOZ_CONFIG")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| default_config_path().map_err(AppError::config))
}
