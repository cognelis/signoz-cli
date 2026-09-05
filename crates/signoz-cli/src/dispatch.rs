use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use signoz_client::{
    ClientError, ErrorCode, FieldQuery, Operation, SigNozClient, TimeWindow, log_aggregate,
    log_search, meter_query, metric_metadata, metric_query, resolve_metric_aggregation,
    trace_aggregate, trace_get, trace_search,
};

use crate::args::*;
use crate::config::{ConfigFile, CredentialStore, SystemKeyring, default_config_path};
use crate::doctor::DoctorReport;
use crate::output::Meta;

pub struct DispatchResult {
    pub command: String,
    pub data: Value,
    pub meta: Meta,
    pub failure_code: Option<ErrorCode>,
}

#[derive(Debug)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub status: Option<u16>,
    pub hint: Option<String>,
}

impl AppError {
    pub(crate) fn config(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Config,
            message: message.into(),
            status: None,
            hint: None,
        }
    }

    pub(crate) fn validation(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Validation,
            message: message.into(),
            status: None,
            hint: None,
        }
    }

    pub(crate) fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl From<ClientError> for AppError {
    fn from(error: ClientError) -> Self {
        Self {
            code: error.code(),
            message: error.to_string(),
            status: error.status(),
            hint: error.hint().map(str::to_owned),
        }
    }
}

enum Prepared {
    Local(DispatchResult),
    Operation(&'static str, Operation),
    MetricUsage(String),
    MetricQuery {
        name: String,
        aggregation: Option<String>,
        filter: Option<String>,
        group_by: Vec<String>,
        step: u32,
        source: Option<String>,
        window: TimeWindow,
        limit: u32,
        offset: u32,
    },
}

pub async fn run(cli: &Cli) -> Result<DispatchResult, AppError> {
    if let Command::Completion { shell } = &cli.command {
        return Ok(DispatchResult {
            command: "completion".into(),
            data: Value::String(crate::completion::generate(*shell)),
            meta: Meta::default(),
            failure_code: None,
        });
    }
    if matches!(&cli.command, Command::Version) {
        return Ok(DispatchResult {
            command: "version".into(),
            data: json!({"version": env!("CARGO_PKG_VERSION")}),
            meta: Meta::default(),
            failure_code: None,
        });
    }
    if matches!(&cli.command, Command::Doctor) {
        return Ok(run_doctor(cli).await);
    }
    if let Command::Config {
        command: ConfigCommand::Path,
    } = &cli.command
    {
        let path = config_path()?;
        return Ok(DispatchResult {
            command: "config.path".into(),
            data: json!({"path": path.to_string_lossy()}),
            meta: Meta::default(),
            failure_code: None,
        });
    }
    if matches!(&cli.command, Command::Init) {
        let path = config_path()?;
        return crate::onboarding::run_init(
            cli.profile.as_deref(),
            cli.url.as_deref(),
            cli.timeout,
            &path,
            &SystemKeyring,
        )
        .await;
    }
    if let Command::Skills { command } = &cli.command {
        let results = match command {
            SkillsCommand::Install { target } => {
                crate::skill_install::install(*target).map_err(AppError::config)?
            }
        };
        return Ok(DispatchResult {
            command: "skills.install".into(),
            data: serde_json::to_value(results)
                .expect("Skill installer result paths are guaranteed strings"),
            meta: Meta::default(),
            failure_code: None,
        });
    }
    let now = Utc::now();
    let path = config_path()?;
    let mut config = ConfigFile::load(&path).map_err(AppError::config)?;
    let store = SystemKeyring;
    if let Command::Auth {
        command: AuthCommand::Login {
            api_key_stdin,
            no_verify,
        },
    } = &cli.command
    {
        return crate::onboarding::run_login(
            cli.profile.as_deref(),
            *api_key_stdin,
            *no_verify,
            cli.timeout,
            cli.output,
            &config,
            &store,
        )
        .await;
    }
    let prepared = prepare(
        &cli.command,
        cli.profile.as_deref(),
        &path,
        &mut config,
        &store,
        now,
    )?;
    let (command, action) = match prepared {
        Prepared::Local(result) => return Ok(result),
        Prepared::Operation(command, operation) => {
            (command.to_owned(), Remote::Operation(operation))
        }
        Prepared::MetricUsage(metric) => ("metrics.usage".to_owned(), Remote::MetricUsage(metric)),
        Prepared::MetricQuery {
            name,
            aggregation,
            filter,
            group_by,
            step,
            source,
            window,
            limit,
            offset,
        } => (
            "metrics.query".to_owned(),
            Remote::MetricQuery {
                name,
                aggregation,
                filter,
                group_by,
                step,
                source,
                window,
                limit,
                offset,
            },
        ),
    };

    let key = std::env::var("SIGNOZ_API_KEY").ok();
    let connection = config
        .resolve(
            cli.url.as_deref(),
            cli.profile.as_deref(),
            key.as_deref(),
            &store,
        )
        .map_err(AppError::config)?;
    let profile = connection.profile.clone();
    let client = SigNozClient::new_with_timeout(
        connection.origin,
        connection.credentials,
        Duration::from_secs(cli.timeout.max(1)),
    )?;
    let started = Instant::now();
    let mut response = match action {
        Remote::Operation(operation) => client.execute(operation).await?,
        Remote::MetricUsage(metric) => client.metric_usage(&metric).await?,
        Remote::MetricQuery {
            name,
            aggregation,
            filter,
            group_by,
            step,
            source,
            window,
            limit,
            offset,
        } => {
            let catalog = client
                .execute(Operation::metrics_search(
                    &name,
                    window.start_ms,
                    window.end_ms,
                    source.as_deref(),
                ))
                .await?;
            let metadata = metric_metadata(&catalog.value, &name).map_err(AppError::validation)?;
            let aggregation = resolve_metric_aggregation(&metadata, aggregation.as_deref())
                .map_err(AppError::validation)?;
            client
                .execute(Operation::query_raw(metric_query(
                    window,
                    &name,
                    &aggregation,
                    filter.as_deref(),
                    &group_by,
                    step,
                    source.as_deref(),
                    limit,
                    offset,
                )))
                .await?
        }
    };
    project_sensitive_response(&cli.command, &mut response.value)?;
    let mut meta = command_meta(&cli.command, now)?;
    meta.profile = profile;
    meta.status = Some(response.meta.status);
    meta.request_id = response.meta.request_id;
    meta.elapsed_ms = Some(started.elapsed().as_millis());
    if let Some((limit, offset)) = list_page(&cli.command) {
        let shape = list_shape(&cli.command).expect("list commands always have a response shape");
        apply_list_pagination(&mut meta, &mut response.value, limit, offset, shape)?;
    } else if matches!(
        &cli.command,
        Command::Metrics {
            command: MetricsCommand::List { .. }
        }
    ) {
        apply_limit_only_metrics_catalog_meta(&mut meta, &mut response.value)?;
    } else if matches!(&cli.command, Command::Fields { .. }) {
        apply_field_discovery_meta(&mut meta, &response.value);
    } else if matches!(
        &cli.command,
        Command::Rules {
            command: RulesCommand::History { .. }
        }
    ) {
        apply_cursor_pagination_meta(&mut meta, &response.value);
    } else if matches!(
        &cli.command,
        Command::Metrics {
            command: MetricsCommand::Top { .. }
        }
    ) {
        apply_limit_only_top_metrics_meta(&mut meta, &response.value);
    } else {
        apply_pagination_meta(&mut meta, &response.value);
    }
    Ok(DispatchResult {
        command,
        data: response.value,
        meta,
        failure_code: None,
    })
}

async fn run_doctor(cli: &Cli) -> DispatchResult {
    let mut report = DoctorReport::new(env!("CARGO_PKG_VERSION"));
    let mut meta = Meta::default();
    let path = match config_path() {
        Ok(path) => path,
        Err(_) => {
            report.configuration_failed();
            return doctor_result(report, meta, Some(ErrorCode::Config));
        }
    };
    let config = match ConfigFile::load(&path) {
        Ok(config) => config,
        Err(_) => {
            report.configuration_failed();
            return doctor_result(report, meta, Some(ErrorCode::Config));
        }
    };
    let target = match config.resolve_target(cli.url.as_deref(), cli.profile.as_deref()) {
        Ok(target) => target,
        Err(_) => {
            report.configuration_failed();
            return doctor_result(report, meta, Some(ErrorCode::Config));
        }
    };
    report.configuration_passed();
    meta.profile = target.profile.clone();

    let key = std::env::var("SIGNOZ_API_KEY").ok();
    let connection = match config.resolve_credentials(target, key.as_deref(), &SystemKeyring) {
        Ok(connection) => connection,
        Err(_) => {
            report.credentials_failed();
            return doctor_result(report, meta, Some(ErrorCode::Config));
        }
    };
    report.credentials_passed();

    let client = match SigNozClient::new_with_timeout(
        connection.origin,
        connection.credentials,
        Duration::from_secs(cli.timeout.max(1)),
    ) {
        Ok(client) => client,
        Err(error) => {
            report.api_failed(doctor_api_failure_message(error.code()));
            return doctor_result(report, meta, Some(error.code()));
        }
    };
    let started = Instant::now();
    match client.execute(Operation::auth_me()).await {
        Ok(mut response) => {
            meta.status = Some(response.meta.status);
            meta.request_id = response.meta.request_id;
            meta.elapsed_ms = Some(started.elapsed().as_millis());
            report.api_passed();
            match project_auth_identity(&mut response.value) {
                Ok(()) => {
                    report.authentication_passed();
                    doctor_result(report, meta, None)
                }
                Err(error) => {
                    report.authentication_failed(
                        "SigNoz returned an incompatible authentication response",
                    );
                    doctor_result(report, meta, Some(error.code))
                }
            }
        }
        Err(error) => {
            let code = error.code();
            meta.status = error.status();
            meta.elapsed_ms = Some(started.elapsed().as_millis());
            if error
                .status()
                .is_some_and(|status| (200..300).contains(&status))
            {
                report.api_passed();
                report.authentication_failed(
                    "SigNoz returned an incompatible authentication response",
                );
            } else if matches!(code, ErrorCode::Unauthorized | ErrorCode::Forbidden) {
                report.api_passed();
                report.authentication_failed(match code {
                    ErrorCode::Unauthorized => "the API key was not accepted by SigNoz",
                    ErrorCode::Forbidden => "the API key is not allowed to access SigNoz",
                    _ => unreachable!(),
                });
            } else {
                report.api_failed(doctor_api_failure_message(code));
            }
            doctor_result(report, meta, Some(code))
        }
    }
}

fn doctor_result(
    report: DoctorReport,
    meta: Meta,
    failure_code: Option<ErrorCode>,
) -> DispatchResult {
    DispatchResult {
        command: "doctor".into(),
        data: report.into_value(),
        meta,
        failure_code,
    }
}

fn doctor_api_failure_message(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Network => "SigNoz API is unreachable",
        ErrorCode::Timeout => "SigNoz API probe timed out",
        ErrorCode::Upstream => "SigNoz API returned an upstream error",
        ErrorCode::RateLimited => "SigNoz API rate-limited the diagnostic request",
        ErrorCode::NotFound
        | ErrorCode::RequestRejected
        | ErrorCode::FeatureUnsupported
        | ErrorCode::Unauthorized
        | ErrorCode::Forbidden => "SigNoz API rejected the diagnostic request",
        ErrorCode::ResponseTooLarge | ErrorCode::IncompatibleResponse => {
            "SigNoz API returned an incompatible diagnostic response"
        }
        ErrorCode::Validation | ErrorCode::Config => {
            "the diagnostic API client could not be configured"
        }
    }
}

enum Remote {
    Operation(Operation),
    MetricUsage(String),
    MetricQuery {
        name: String,
        aggregation: Option<String>,
        filter: Option<String>,
        group_by: Vec<String>,
        step: u32,
        source: Option<String>,
        window: TimeWindow,
        limit: u32,
        offset: u32,
    },
}

fn prepare(
    command: &Command,
    selected_profile: Option<&str>,
    config_path: &Path,
    config: &mut ConfigFile,
    store: &dyn CredentialStore,
    now: DateTime<Utc>,
) -> Result<Prepared, AppError> {
    match command {
        Command::Version => local("version", json!({"version": env!("CARGO_PKG_VERSION")})),
        Command::Completion { .. } => {
            unreachable!("completion is dispatched before command preparation")
        }
        Command::Init => unreachable!("init is dispatched before command preparation"),
        Command::Doctor => unreachable!("doctor is dispatched before command preparation"),
        Command::Config { command } => match command {
            ConfigCommand::Path => local(
                "config.path",
                json!({"path": config_path.to_string_lossy()}),
            ),
            ConfigCommand::Show => local(
                "config.show",
                serde_json::to_value(config)
                    .map_err(|error| AppError::config(error.to_string()))?,
            ),
        },
        Command::Skills { .. } => {
            unreachable!("skills are dispatched before command preparation")
        }
        Command::Auth { command } => match command {
            AuthCommand::Check => Ok(Prepared::Operation("auth.check", Operation::auth_me())),
            AuthCommand::Login { .. } => {
                unreachable!("auth login is dispatched before command preparation")
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
                config
                    .add_profile(name, url)
                    .map_err(AppError::validation)?;
                config.save(config_path).map_err(AppError::config)?;
                local("profile.add", json!({"name": name, "url": url}))
            }
            ProfileCommand::Update { name, url } => {
                let changed = config.update_profile(name, url, store)?;
                config.save(config_path).map_err(AppError::config)?;
                Ok(Prepared::Local(DispatchResult {
                    command: "profile.update".into(),
                    data: json!({"name": name, "url": config.profiles[name].url}),
                    meta: Meta {
                        warnings: if changed {
                            vec![format!(
                                "Profile URL changed and the saved API key was cleared; run `signoz-cli --profile {name} auth login` before querying this environment"
                            )]
                        } else {
                            Vec::new()
                        },
                        ..Meta::default()
                    },
                    failure_code: None,
                }))
            }
            ProfileCommand::List => Ok(Prepared::Local(DispatchResult {
                command: "profile.list".into(),
                data: serde_json::to_value(&config.profiles)
                    .map_err(|error| AppError::config(error.to_string()))?,
                meta: Meta {
                    profile: config.current_profile.clone(),
                    ..Meta::default()
                },
                failure_code: None,
            })),
            ProfileCommand::Show { name } => {
                let name = name
                    .clone()
                    .or_else(|| selected_profile.map(str::to_owned))
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
                config
                    .remove_profile(name, store)
                    .map_err(AppError::config)?;
                config.save(config_path).map_err(AppError::config)?;
                local("profile.remove", json!({"name": name, "removed": true}))
            }
        },
        Command::Metrics { command } => match command {
            MetricsCommand::List {
                search,
                source,
                time,
                page,
            } => {
                let value = window(time, now)?;
                operation(
                    "metrics.list",
                    Operation::metrics_list(
                        value.start_ms,
                        value.end_ms,
                        page.limit,
                        search.as_deref(),
                        source.map(|value| value.as_str()),
                    ),
                )
            }
            MetricsCommand::Query {
                name,
                aggregation,
                filter,
                group_by,
                step,
                source,
                time,
                page,
            } => {
                if group_by.iter().any(|value| value.trim().is_empty()) {
                    return Err(AppError::validation("--group-by values must not be empty"));
                }
                Ok(Prepared::MetricQuery {
                    name: name.clone(),
                    aggregation: aggregation.clone(),
                    filter: filter.clone(),
                    group_by: group_by.clone(),
                    step: *step,
                    source: source.map(MetricSource::as_str).map(str::to_owned),
                    window: window(time, now)?,
                    limit: page.limit,
                    offset: page.offset,
                })
            }
            MetricsCommand::Top { time, page } => {
                let window = window(time, now)?;
                operation(
                    "metrics.top",
                    Operation::metric_treemap(window.start_ms, window.end_ms, page.limit),
                )
            }
            MetricsCommand::Cardinality { name, time } => {
                let window = window(time, now)?;
                operation(
                    "metrics.cardinality",
                    Operation::metric_attributes(name, window.start_ms, window.end_ms),
                )
            }
            MetricsCommand::Usage { name } => Ok(Prepared::MetricUsage(name.clone())),
            MetricsCommand::Metadata { name } => {
                operation("metrics.metadata", Operation::metric_metadata(name))
            }
            MetricsCommand::Highlights { name } => {
                operation("metrics.highlights", Operation::metric_highlights(name))
            }
            MetricsCommand::Inspect { name, filter, time } => {
                let window = inspect_window(time, now)?;
                operation(
                    "metrics.inspect",
                    Operation::metric_inspect(
                        name,
                        window.start_ms,
                        window.end_ms,
                        filter.as_deref(),
                    ),
                )
            }
        },
        Command::Fields { command } => match command {
            FieldsCommand::Keys { signal, query } => {
                let window = window(&query.time, now)?;
                operation(
                    "fields.keys",
                    Operation::field_keys(signal.as_str(), field_query(query, window)),
                )
            }
            FieldsCommand::Values {
                signal,
                key,
                existing_query,
                query,
            } => {
                let window = window(&query.time, now)?;
                operation(
                    "fields.values",
                    Operation::field_values(
                        signal.as_str(),
                        key,
                        existing_query.as_deref(),
                        field_query(query, window),
                    ),
                )
            }
        },
        Command::Logs { command } => match command {
            LogsCommand::Search {
                search,
                filter,
                time,
                page,
            } => {
                let mut body = log_search(
                    window(time, now)?,
                    search.as_deref(),
                    page.limit,
                    page.offset,
                );
                merge_filter(&mut body, filter.as_deref());
                operation("logs.search", Operation::query_raw(body))
            }
            LogsCommand::Aggregate {
                filter,
                aggregation,
                aggregate_on,
                group_by,
                step,
                page,
                time,
            } => {
                let expression = aggregate_expression(*aggregation, aggregate_on.as_deref())?;
                validate_group_by(group_by)?;
                operation(
                    "logs.aggregate",
                    Operation::query_raw(log_aggregate(
                        window(time, now)?,
                        filter.as_deref(),
                        &expression,
                        group_by,
                        *step,
                        page.limit,
                    )),
                )
            }
        },
        Command::Traces { command } => match command {
            TracesCommand::Search { filter, time, page } => operation(
                "traces.search",
                Operation::query_raw(trace_search(
                    window(time, now)?,
                    filter.as_deref(),
                    page.limit,
                    page.offset,
                )),
            ),
            TracesCommand::Aggregate {
                filter,
                aggregation,
                aggregate_on,
                group_by,
                step,
                page,
                time,
            } => {
                let expression = aggregate_expression(*aggregation, aggregate_on.as_deref())?;
                validate_group_by(group_by)?;
                operation(
                    "traces.aggregate",
                    Operation::query_raw(trace_aggregate(
                        window(time, now)?,
                        filter.as_deref(),
                        &expression,
                        group_by,
                        *step,
                        page.limit,
                    )),
                )
            }
            TracesCommand::Get { trace_id, time } => operation(
                "traces.get",
                Operation::query_raw(trace_get(window(time, now)?, trace_id)),
            ),
        },
        Command::Services { command } => match command {
            ServicesCommand::List { time, .. } => {
                let value = window(time, now)?;
                operation(
                    "services.list",
                    Operation::services_list(value.start_ms, value.end_ms),
                )
            }
            ServicesCommand::Operations { service, time } => {
                let value = window(time, now)?;
                operation(
                    "services.operations",
                    Operation::service_operations(service, value.start_ms, value.end_ms),
                )
            }
            ServicesCommand::Dependencies { time } => {
                let value = window(time, now)?;
                operation(
                    "services.dependencies",
                    Operation::service_dependencies(value.start_ms, value.end_ms),
                )
            }
        },
        Command::Exceptions { command } => match command {
            ExceptionsCommand::List {
                service,
                exception_type,
                order,
                order_by,
                time,
                page,
            } => {
                let value = window(time, now)?;
                operation(
                    "exceptions.list",
                    Operation::exceptions_list(
                        value.start_ms,
                        value.end_ms,
                        order.as_str(),
                        order_by.as_str(),
                        page.limit,
                        page.offset,
                        exception_type.as_deref(),
                        service.as_deref(),
                    ),
                )
            }
            ExceptionsCommand::Count {
                service,
                exception_type,
                time,
            } => {
                let value = window(time, now)?;
                operation(
                    "exceptions.count",
                    Operation::exceptions_count(
                        value.start_ms,
                        value.end_ms,
                        exception_type.as_deref(),
                        service.as_deref(),
                    ),
                )
            }
            ExceptionsCommand::Group {
                group_id,
                timestamp,
            } => operation(
                "exceptions.group",
                Operation::exception_group(group_id, timestamp),
            ),
            ExceptionsCommand::Get {
                group_id,
                error_id,
                timestamp,
            } => operation(
                "exceptions.get",
                Operation::exception_get(group_id, error_id, timestamp),
            ),
            ExceptionsCommand::Navigate {
                group_id,
                error_id,
                timestamp,
            } => operation(
                "exceptions.navigate",
                Operation::exception_navigation(group_id, error_id, timestamp),
            ),
        },
        Command::Infrastructure { command } => match command {
            InfrastructureCommand::Check { resource } => operation(
                "infrastructure.check",
                Operation::infrastructure_check(resource.as_str()),
            ),
            InfrastructureCommand::List {
                resource,
                filter,
                status,
                group_by,
                order_by,
                order,
                time,
                page,
            } => {
                if status.is_some() && !matches!(resource, InfrastructureResource::Hosts) {
                    return Err(AppError::validation(
                        "--status is supported only for infrastructure hosts",
                    ));
                }
                let value = window(time, now)?;
                let body = infrastructure_list_body(
                    value,
                    filter.as_deref(),
                    status.map(InfrastructureStatus::as_str),
                    group_by,
                    order_by.as_deref(),
                    order.as_str(),
                    page.limit,
                    page.offset,
                )?;
                operation(
                    "infrastructure.list",
                    Operation::infrastructure_list(resource.as_path(), body),
                )
            }
        },
        Command::Meters { command } => {
            const ALL_METERS: &[&str] = &[
                "signoz.meter.log.count",
                "signoz.meter.log.size",
                "signoz.meter.span.count",
                "signoz.meter.span.size",
                "signoz.meter.metric.datapoint.count",
            ];
            match command {
                MetersCommand::Summary {
                    filter,
                    group_by,
                    step,
                    time,
                } => operation(
                    "meters.summary",
                    Operation::query_raw(meter_query(
                        meter_window(time, now)?,
                        ALL_METERS,
                        filter.as_deref(),
                        group_by,
                        *step,
                    )),
                ),
                MetersCommand::Query {
                    metric,
                    filter,
                    group_by,
                    step,
                    time,
                } => {
                    if !metric.starts_with("signoz.meter.") {
                        return Err(AppError::validation(
                            "meter metric names must start with 'signoz.meter.'",
                        ));
                    }
                    operation(
                        "meters.query",
                        Operation::query_raw(meter_query(
                            meter_window(time, now)?,
                            &[metric.as_str()],
                            filter.as_deref(),
                            group_by,
                            *step,
                        )),
                    )
                }
            }
        }
        Command::Alerts {
            command: AlertsCommand::List { .. },
        } => operation("alerts.list", Operation::alerts_list()),
        Command::Rules { command } => match command {
            RulesCommand::List { .. } => operation("rules.list", Operation::rules_list()),
            RulesCommand::Get { id } => operation("rules.get", Operation::rule_get(id)),
            RulesCommand::History {
                id,
                time,
                limit,
                cursor,
                order,
                state,
                filter,
            } => {
                let value = window(time, now)?;
                operation(
                    "rules.history",
                    Operation::rule_history(
                        id,
                        value.start_ms,
                        value.end_ms,
                        *limit,
                        order.as_str(),
                        cursor.as_deref(),
                        state.map(|value| value.as_str()),
                        filter.as_deref(),
                    ),
                )
            }
            RulesCommand::HistoryStats { id, time } => {
                let value = window(time, now)?;
                operation(
                    "rules.history-stats",
                    Operation::rule_history_stats(id, value.start_ms, value.end_ms),
                )
            }
            RulesCommand::HistoryStatus { id, time } => {
                let value = window(time, now)?;
                operation(
                    "rules.history-status",
                    Operation::rule_history_overall_status(id, value.start_ms, value.end_ms),
                )
            }
            RulesCommand::HistoryContributors { id, time } => {
                let value = window(time, now)?;
                operation(
                    "rules.history-contributors",
                    Operation::rule_history_top_contributors(id, value.start_ms, value.end_ms),
                )
            }
        },
        Command::Dashboards { command } => match command {
            DashboardsCommand::List {
                query,
                sort,
                order,
                page,
            } => operation(
                "dashboards.list",
                Operation::dashboards_list(
                    query.as_deref(),
                    sort.as_str(),
                    order.as_str(),
                    page.limit,
                    page.offset,
                ),
            ),
            DashboardsCommand::Get { id } => {
                operation("dashboards.get", Operation::dashboard_get(id))
            }
        },
        Command::Views { command } => match command {
            ViewsCommand::List { source_page, .. } => {
                operation("views.list", Operation::views_list(source_page.as_str()))
            }
            ViewsCommand::Get { id } => operation("views.get", Operation::view_get(id)),
        },
        Command::Channels { command } => resource(
            command,
            "channels",
            Operation::channels_list,
            Operation::channel_get,
        ),
        Command::Query { command } => match command {
            QueryCommand::Run { file } => {
                let body = read_query_body(file)?;
                operation("query.run", Operation::query_raw(body))
            }
            QueryCommand::Preview { file, explain } => {
                let body = read_query_body(file)?;
                operation("query.preview", Operation::query_preview(body, *explain))
            }
        },
    }
}

fn local(command: &str, data: Value) -> Result<Prepared, AppError> {
    Ok(Prepared::Local(DispatchResult {
        command: command.to_owned(),
        data,
        meta: Meta::default(),
        failure_code: None,
    }))
}

fn operation(command: &'static str, value: Operation) -> Result<Prepared, AppError> {
    Ok(Prepared::Operation(command, value))
}

fn window(args: &TimeArgs, now: DateTime<Utc>) -> Result<TimeWindow, AppError> {
    args.as_client_args()
        .resolve(now)
        .map_err(AppError::validation)
}

fn inspect_window(args: &TimeArgs, now: DateTime<Utc>) -> Result<TimeWindow, AppError> {
    let window = args
        .as_client_args_with_default("30m")
        .resolve(now)
        .map_err(AppError::validation)?;
    if window.end_ms.saturating_sub(window.start_ms) > 30 * 60 * 1_000 {
        return Err(AppError::validation(
            "metrics inspect supports a maximum 30-minute time range",
        ));
    }
    Ok(window)
}

fn meter_window(args: &TimeArgs, now: DateTime<Utc>) -> Result<TimeWindow, AppError> {
    const HOUR_MS: i64 = 60 * 60 * 1_000;
    let relative = args.start.is_none();
    let value = if args.since.is_none() && relative {
        signoz_client::TimeWindowArgs::since("7d")
    } else {
        args.as_client_args()
    };
    let resolved = value.resolve(now).map_err(AppError::validation)?;
    let end_ms = resolved.end_ms.div_euclid(HOUR_MS) * HOUR_MS;
    let (start_ms, end_ms) = if relative {
        let complete_hours = (resolved.end_ms - resolved.start_ms).div_euclid(HOUR_MS);
        if complete_hours < 1 {
            return Err(AppError::validation(
                "Cost Meter queries require at least one complete hour",
            ));
        }
        (
            end_ms.saturating_sub(complete_hours.saturating_mul(HOUR_MS)),
            end_ms,
        )
    } else {
        let start_ms = if resolved.start_ms.rem_euclid(HOUR_MS) == 0 {
            resolved.start_ms
        } else {
            resolved.start_ms.div_euclid(HOUR_MS) * HOUR_MS + HOUR_MS
        };
        (start_ms, end_ms)
    };
    if start_ms >= end_ms {
        return Err(AppError::validation(
            "Cost Meter time range contains no complete hourly buckets",
        ));
    }
    Ok(TimeWindow { start_ms, end_ms })
}

fn field_query(args: &FieldQueryArgs, window: TimeWindow) -> FieldQuery<'_> {
    FieldQuery {
        source: args.source.map(MetricSource::as_str),
        limit: args.page.limit,
        start_ms: window.start_ms,
        end_ms: window.end_ms,
        context: args.context.map(FieldContext::as_str),
        data_type: args.data_type.map(FieldDataType::as_str),
        metric_name: args.metric_name.as_deref(),
        metric_namespace: args.metric_namespace.as_deref(),
        search: args.search.as_deref(),
    }
}

#[allow(clippy::too_many_arguments)]
fn infrastructure_list_body(
    window: TimeWindow,
    filter: Option<&str>,
    status: Option<&str>,
    group_by: &[String],
    order_by: Option<&str>,
    order: &str,
    limit: u32,
    offset: u32,
) -> Result<Value, AppError> {
    if group_by.iter().any(|value| value.trim().is_empty()) {
        return Err(AppError::validation("--group-by values must not be empty"));
    }
    if order_by.is_some_and(|value| value.trim().is_empty()) {
        return Err(AppError::validation("--order-by must not be empty"));
    }
    let mut filter_value = json!({"expression": filter.unwrap_or_default()});
    if let Some(status) = status {
        filter_value["filterByStatus"] = Value::String(status.to_owned());
    }
    let mut body = json!({
        "start": window.start_ms,
        "end": window.end_ms,
        "limit": limit.max(1),
        "offset": offset,
        "filter": filter_value,
        "groupBy": group_by
            .iter()
            .map(|name| json!({"name": name}))
            .collect::<Vec<_>>()
    });
    if let Some(order_by) = order_by {
        body["orderBy"] = json!({
            "key": {"name": order_by},
            "direction": order,
        });
    }
    Ok(body)
}

fn aggregate_expression(
    aggregation: AggregateFunction,
    aggregate_on: Option<&str>,
) -> Result<String, AppError> {
    let field = aggregate_on
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if aggregation.requires_field() && field.is_none() {
        return Err(AppError::validation(format!(
            "--aggregate-on is required for --aggregation {}",
            aggregation.as_str()
        )));
    }
    Ok(match field {
        Some(field) => format!("{}({field})", aggregation.as_str()),
        None => format!("{}()", aggregation.as_str()),
    })
}

fn validate_group_by(group_by: &[String]) -> Result<(), AppError> {
    if group_by.iter().any(|value| value.trim().is_empty()) {
        return Err(AppError::validation("--group-by values must not be empty"));
    }
    Ok(())
}

fn command_meta(command: &Command, now: DateTime<Utc>) -> Result<Meta, AppError> {
    let (time, page) = match command {
        Command::Metrics { command } => match command {
            MetricsCommand::List { time, page, .. } => (Some(time), Some((page.limit, 0))),
            MetricsCommand::Query { time, page, .. } => {
                (Some(time), Some((page.limit, page.offset)))
            }
            MetricsCommand::Top { time, page } => (Some(time), Some((page.limit, 0))),
            MetricsCommand::Cardinality { time, .. } => (Some(time), None),
            MetricsCommand::Inspect { time, .. } => (Some(time), None),
            _ => (None, None),
        },
        Command::Logs { command } => match command {
            LogsCommand::Search { time, page, .. } => (Some(time), Some((page.limit, page.offset))),
            LogsCommand::Aggregate { time, .. } => (Some(time), None),
        },
        Command::Traces { command } => match command {
            TracesCommand::Search { time, page, .. } => {
                (Some(time), Some((page.limit, page.offset)))
            }
            TracesCommand::Aggregate { time, .. } => (Some(time), None),
            TracesCommand::Get { time, .. } => (Some(time), None),
        },
        Command::Fields { command } => match command {
            FieldsCommand::Keys { query, .. } | FieldsCommand::Values { query, .. } => {
                (Some(&query.time), Some((query.page.limit, 0)))
            }
        },
        Command::Services { command } => match command {
            ServicesCommand::List { time, page } => (Some(time), Some((page.limit, page.offset))),
            ServicesCommand::Operations { time, .. } => (Some(time), None),
            ServicesCommand::Dependencies { time } => (Some(time), None),
        },
        Command::Exceptions { command } => match command {
            ExceptionsCommand::List { time, page, .. } => {
                (Some(time), Some((page.limit, page.offset)))
            }
            ExceptionsCommand::Count { time, .. } => (Some(time), None),
            _ => (None, None),
        },
        Command::Infrastructure { command } => match command {
            InfrastructureCommand::List { time, page, .. } => {
                (Some(time), Some((page.limit, page.offset)))
            }
            InfrastructureCommand::Check { .. } => (None, None),
        },
        Command::Meters { command } => match command {
            MetersCommand::Summary { time, .. } | MetersCommand::Query { time, .. } => {
                (Some(time), None)
            }
        },
        Command::Alerts {
            command: AlertsCommand::List { page },
        } => (None, Some((page.limit, page.offset))),
        Command::Rules {
            command: RulesCommand::List { page },
        } => (None, Some((page.limit, page.offset))),
        Command::Rules {
            command: RulesCommand::History { time, .. },
        }
        | Command::Rules {
            command: RulesCommand::HistoryStats { time, .. },
        }
        | Command::Rules {
            command: RulesCommand::HistoryStatus { time, .. },
        }
        | Command::Rules {
            command: RulesCommand::HistoryContributors { time, .. },
        } => (Some(time), None),
        Command::Dashboards {
            command: DashboardsCommand::List { page, .. },
        }
        | Command::Channels {
            command: ResourceCommand::List { page },
        } => (None, Some((page.limit, page.offset))),
        Command::Views {
            command: ViewsCommand::List { page, .. },
        } => (None, Some((page.limit, page.offset))),
        _ => (None, None),
    };
    let mut meta = Meta::default();
    if let Some(time) = time {
        let resolved = match command {
            Command::Meters { .. } => meter_window(time, now)?,
            Command::Metrics {
                command: MetricsCommand::Inspect { .. },
            } => inspect_window(time, now)?,
            _ => window(time, now)?,
        };
        meta.start = timestamp_string(resolved.start_ms);
        meta.end = timestamp_string(resolved.end_ms);
    }
    if let Some((limit, offset)) = page {
        meta.limit = Some(limit);
        meta.offset = Some(offset);
    }
    if matches!(
        command,
        Command::Metrics {
            command: MetricsCommand::List { .. }
        }
    ) || matches!(command, Command::Fields { .. })
    {
        meta.offset = None;
    }
    if let Command::Rules {
        command: RulesCommand::History { limit, cursor, .. },
    } = command
    {
        meta.limit = Some(*limit);
        meta.cursor = cursor.clone();
    }
    Ok(meta)
}

fn timestamp_string(milliseconds: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp_millis(milliseconds)
        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn project_sensitive_response(command: &Command, response: &mut Value) -> Result<(), AppError> {
    match command {
        Command::Auth {
            command: AuthCommand::Check,
        } => project_auth_identity(response),
        Command::Channels {
            command: ResourceCommand::List { .. },
        } => {
            project_channel_summaries(response);
            Ok(())
        }
        _ => Ok(()),
    }
}

pub(crate) fn project_auth_identity(response: &mut Value) -> Result<(), AppError> {
    let source = response
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AppError::from(ClientError::new(
                ErrorCode::IncompatibleResponse,
                "SigNoz authentication response did not contain an identity object",
            ))
        })?;
    if !source
        .get("id")
        .and_then(Value::as_str)
        .is_some_and(|id| !id.trim().is_empty())
    {
        return Err(AppError::from(ClientError::new(
            ErrorCode::IncompatibleResponse,
            "SigNoz authentication response did not contain a non-empty identity id",
        )));
    }
    let mut identity = serde_json::Map::new();
    for field in ["id", "name", "displayName", "email", "orgId"] {
        if let Some(value) = source.get(field).filter(|value| value.is_string()) {
            identity.insert(field.to_owned(), value.clone());
        }
    }
    *response = json!({
        "authenticated": true,
        "principal": "service_account",
        "identity": identity,
    });
    Ok(())
}

fn project_channel_summaries(response: &mut Value) {
    let Some(channels) = response.get_mut("data").and_then(Value::as_array_mut) else {
        return;
    };
    for channel in channels {
        let Some(source) = channel.as_object() else {
            continue;
        };
        let mut summary = serde_json::Map::new();
        for field in ["id", "name", "type", "createdAt", "updatedAt"] {
            if let Some(value) = source.get(field) {
                summary.insert(field.to_owned(), value.clone());
            }
        }
        if !summary.contains_key("name") {
            let nested_name = source
                .get("data")
                .and_then(Value::as_str)
                .and_then(|data| serde_json::from_str::<Value>(data).ok())
                .and_then(|data| data.get("name").cloned());
            if let Some(name) = nested_name {
                summary.insert("name".into(), name);
            }
        }
        *channel = Value::Object(summary);
    }
}

fn list_page(command: &Command) -> Option<(u32, u32)> {
    let page = match command {
        Command::Services {
            command: ServicesCommand::List { page, .. },
        }
        | Command::Alerts {
            command: AlertsCommand::List { page },
        }
        | Command::Rules {
            command: RulesCommand::List { page },
        }
        | Command::Channels {
            command: ResourceCommand::List { page },
        }
        | Command::Views {
            command: ViewsCommand::List { page, .. },
        } => page,
        _ => return None,
    };
    Some((page.limit, page.offset))
}

#[derive(Clone, Copy)]
enum ListShape {
    Root,
    Data,
    TolerantData,
}

fn list_shape(command: &Command) -> Option<ListShape> {
    match command {
        Command::Services {
            command: ServicesCommand::List { .. },
        } => Some(ListShape::Root),
        Command::Alerts {
            command: AlertsCommand::List { .. },
        }
        | Command::Rules {
            command: RulesCommand::List { .. },
        } => Some(ListShape::Data),
        Command::Channels {
            command: ResourceCommand::List { .. },
        }
        | Command::Views {
            command: ViewsCommand::List { .. },
        } => Some(ListShape::TolerantData),
        _ => None,
    }
}

fn apply_list_pagination(
    meta: &mut Meta,
    response: &mut Value,
    limit: u32,
    offset: u32,
    shape: ListShape,
) -> Result<(), AppError> {
    let rows = match shape {
        ListShape::Root => response.as_array_mut(),
        ListShape::Data => response.get_mut("data").and_then(array_or_null),
        ListShape::TolerantData => tolerant_data_array(response),
    }
    .ok_or_else(|| {
        AppError::from(ClientError::new(
            ErrorCode::IncompatibleResponse,
            "SigNoz list response did not contain a recognized data array",
        ))
    })?;

    let fetched = rows.len();
    let total = u32::try_from(fetched).unwrap_or(u32::MAX);
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(rows.len());
    let end = start
        .saturating_add(usize::try_from(limit).unwrap_or(usize::MAX))
        .min(rows.len());
    *rows = rows[start..end].to_vec();

    meta.limit = Some(limit);
    meta.offset = Some(offset);
    meta.total = Some(total);
    meta.has_more = Some(end < fetched);
    meta.next_offset = meta
        .has_more
        .is_some_and(|has_more| has_more)
        .then_some(offset.saturating_add(limit));
    Ok(())
}

fn array_or_null(value: &mut Value) -> Option<&mut Vec<Value>> {
    if value.is_null() {
        *value = Value::Array(Vec::new());
    }
    value.as_array_mut()
}

fn tolerant_data_array(response: &mut Value) -> Option<&mut Vec<Value>> {
    let object = response.as_object_mut()?;
    let data = object
        .entry("data")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !data.is_array() {
        *data = Value::Array(Vec::new());
    }
    data.as_array_mut()
}

fn apply_pagination_meta(meta: &mut Meta, response: &Value) {
    let Some(limit) = meta.limit else { return };
    if let Some(total) = response
        .pointer("/data/total")
        .or_else(|| response.get("total"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
    {
        meta.total = Some(total);
    }
    if let Some(has_more) = response
        .pointer("/data/hasMore")
        .or_else(|| response.get("hasMore"))
        .and_then(Value::as_bool)
    {
        meta.has_more = Some(has_more);
    }
    let count = count_query_range_rows(response).or_else(|| {
        response
            .pointer("/data/result")
            .or_else(|| response.pointer("/data/items"))
            .or_else(|| response.pointer("/data/samples"))
            .or_else(|| response.pointer("/data/dashboards"))
            .or_else(|| response.pointer("/data/records"))
            .or_else(|| response.get("data"))
            .or_else(|| response.get("result"))
            .or_else(|| response.as_array().map(|_| response))
            .and_then(Value::as_array)
            .map(Vec::len)
    });
    if let Some(count) = count {
        let offset = meta.offset.unwrap_or(0);
        let inferred = meta
            .total
            .map(|total| offset.saturating_add(count as u32) < total)
            .unwrap_or(count as u32 >= limit);
        if meta.has_more.is_none() {
            meta.has_more = Some(inferred);
            if inferred {
                meta.warnings.push(
                    "result reached the requested limit; use --offset to check for more rows"
                        .into(),
                );
            }
        }
        if meta.has_more == Some(true) {
            meta.next_offset = Some(offset.saturating_add(count as u32));
        }
    } else {
        meta.warnings.push(
            "the response did not expose a recognized row array; the requested limit may have truncated results"
                .into(),
        );
    }
}

fn apply_limit_only_top_metrics_meta(meta: &mut Meta, response: &Value) {
    let Some(limit) = meta.limit else { return };
    let count = response
        .pointer("/data/samples")
        .and_then(Value::as_array)
        .map(Vec::len);
    match count {
        Some(count) if count as u32 >= limit => {
            meta.has_more = Some(true);
            meta.next_offset = None;
            meta.warnings.push(
                "metrics top reached its fixed upstream limit and has no offset pagination; narrow the time window before claiming completeness"
                    .into(),
            );
        }
        Some(_) => {
            meta.has_more = Some(false);
            meta.next_offset = None;
        }
        None => meta.warnings.push(
            "metrics top response did not expose data.samples; completeness is unknown".into(),
        ),
    }
}

fn apply_limit_only_metrics_catalog_meta(
    meta: &mut Meta,
    response: &mut Value,
) -> Result<(), AppError> {
    let limit = meta
        .limit
        .ok_or_else(|| AppError::validation("metrics list requires a positive catalog limit"))?;
    let rows = response
        .pointer_mut("/data/metrics")
        .and_then(array_or_null)
        .ok_or_else(|| {
            AppError::from(ClientError::new(
                ErrorCode::IncompatibleResponse,
                "SigNoz metrics response did not contain data.metrics",
            ))
        })?;
    let count = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    let capped = count >= limit;
    meta.total = (!capped).then_some(count);
    meta.has_more = Some(capped);
    meta.next_offset = None;
    if capped {
        meta.warnings.push(
            "the metrics catalog reached its fixed upstream limit and has no offset pagination; narrow --since, --search, or --source before claiming completeness"
                .into(),
        );
    }
    Ok(())
}

fn apply_field_discovery_meta(meta: &mut Meta, response: &Value) {
    let complete = response.pointer("/data/complete").and_then(Value::as_bool);
    if let Some(complete) = complete {
        meta.has_more = Some(!complete);
        meta.next_offset = None;
        if !complete {
            meta.warnings.push(
                "field discovery reached the server limit; narrow --search, --context, --type, metric filters, or the time window"
                    .into(),
            );
        }
    } else {
        meta.warnings.push(
            "field discovery response did not expose data.complete; completeness is unknown".into(),
        );
    }
}

fn apply_cursor_pagination_meta(meta: &mut Meta, response: &Value) {
    let next_cursor = response
        .pointer("/data/nextCursor")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    meta.has_more = Some(next_cursor.is_some());
    meta.next_cursor = next_cursor;
    meta.next_offset = None;
}

fn count_query_range_rows(response: &Value) -> Option<usize> {
    let results = response.pointer("/data/data/results")?.as_array()?;
    let mut count = 0usize;
    for result in results {
        let rows = result.get("rows")?;
        if rows.is_null() {
            continue;
        }
        count = count.checked_add(rows.as_array()?.len())?;
    }
    Some(count)
}

fn resource(
    command: &ResourceCommand,
    prefix: &'static str,
    list: fn() -> Operation,
    get: fn(&str) -> Operation,
) -> Result<Prepared, AppError> {
    match command {
        ResourceCommand::List { .. } => operation(
            match prefix {
                "dashboards" => "dashboards.list",
                _ => "channels.list",
            },
            list(),
        ),
        ResourceCommand::Get { id } => operation(
            match prefix {
                "dashboards" => "dashboards.get",
                "views" => "views.get",
                _ => "channels.get",
            },
            get(id),
        ),
    }
}

fn merge_filter(body: &mut Value, explicit: Option<&str>) {
    let Some(explicit) = explicit else { return };
    let expression = &mut body["compositeQuery"]["queries"][0]["spec"]["filter"]["expression"];
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

fn read_query_body(path: &Path) -> Result<Value, AppError> {
    let text = read_input(path)?;
    let mut body = serde_json::from_str(&text)
        .map_err(|error| AppError::validation(format!("invalid query JSON: {error}")))?;
    validate_query_builder_v5(&mut body)?;
    Ok(body)
}

fn validate_query_builder_v5(body: &mut Value) -> Result<(), AppError> {
    let object = body
        .as_object_mut()
        .ok_or_else(|| AppError::validation("Query Builder v5 input must be a JSON object"))?;
    if !object.contains_key("schemaVersion") {
        object.insert("schemaVersion".into(), Value::String("v1".into()));
    } else if object.get("schemaVersion").and_then(Value::as_str) != Some("v1") {
        return Err(AppError::validation(
            "Query Builder v5 input requires schemaVersion \"v1\"",
        ));
    }
    let start = object
        .get("start")
        .and_then(Value::as_i64)
        .ok_or_else(|| AppError::validation("Query Builder v5 input requires integer start"))?;
    let end = object
        .get("end")
        .and_then(Value::as_i64)
        .ok_or_else(|| AppError::validation("Query Builder v5 input requires integer end"))?;
    if start >= end {
        return Err(AppError::validation(
            "Query Builder v5 start must be before end",
        ));
    }
    let request_type = object
        .get("requestType")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::validation("Query Builder v5 input requires requestType"))?;
    if !matches!(request_type, "raw" | "scalar" | "time_series" | "trace") {
        return Err(AppError::validation(format!(
            "unsupported Query Builder v5 requestType '{request_type}'"
        )));
    }
    let queries = object
        .get("compositeQuery")
        .and_then(|value| value.get("queries"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            AppError::validation("Query Builder v5 input requires compositeQuery.queries")
        })?;
    if queries.is_empty() {
        return Err(AppError::validation(
            "Query Builder v5 compositeQuery.queries must not be empty",
        ));
    }
    if queries.iter().any(|query| {
        query.get("type").and_then(Value::as_str).is_none()
            || !query.get("spec").is_some_and(Value::is_object)
    }) {
        return Err(AppError::validation(
            "every Query Builder v5 query requires string type and object spec",
        ));
    }
    Ok(())
}

fn config_path() -> Result<PathBuf, AppError> {
    std::env::var_os("SIGNOZ_CONFIG")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| default_config_path().map_err(AppError::config))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use chrono::TimeZone;

    use super::*;

    #[test]
    fn profile_update_saves_the_new_destination_and_reports_credential_invalidation() {
        struct Store(Cell<bool>);
        impl CredentialStore for Store {
            fn get(&self, _: &str) -> Result<Option<String>, String> {
                panic!("updating the URL must not read a credential")
            }
            fn set(&self, _: &str, _: &str) -> Result<(), String> {
                panic!("updating the URL must not store a credential")
            }
            fn delete(&self, name: &str) -> Result<(), String> {
                assert_eq!(name, "prod");
                assert!(
                    self.0.replace(false),
                    "an unchanged URL must not clear credentials"
                );
                Ok(())
            }
        }
        let store = Store(Cell::new(true));
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        let mut config = ConfigFile::default();
        config
            .add_profile("prod", "https://old.example.com")
            .unwrap();
        config.save(&path).unwrap();
        let command = Command::Profile {
            command: ProfileCommand::Update {
                name: "prod".into(),
                url: "https://new.example.com/".into(),
            },
        };
        for changed in [true, false] {
            let Prepared::Local(result) =
                prepare(&command, None, &path, &mut config, &store, Utc::now()).unwrap()
            else {
                panic!("profile update must be local");
            };
            assert!(!store.0.get());
            assert_eq!(
                ConfigFile::load(&path).unwrap().profiles["prod"].url,
                "https://new.example.com"
            );
            assert_eq!(result.data["url"], "https://new.example.com");
            assert_eq!(!result.meta.warnings.is_empty(), changed);
            if changed {
                assert!(result.meta.warnings[0].contains("--profile prod auth login"));
            }
        }
    }

    #[test]
    fn relative_meter_windows_use_only_complete_hours() {
        let now = Utc.with_ymd_and_hms(2026, 7, 23, 13, 37, 0).unwrap();
        let args = TimeArgs {
            since: Some("7d".into()),
            start: None,
            end: None,
        };

        let window = meter_window(&args, now).unwrap();

        assert_eq!(window.end_ms % 3_600_000, 0);
        assert_eq!(window.end_ms - window.start_ms, 7 * 24 * 3_600_000);
    }

    #[test]
    fn meter_windows_reject_ranges_without_a_complete_hour() {
        let now = Utc.with_ymd_and_hms(2026, 7, 23, 13, 37, 0).unwrap();
        let args = TimeArgs {
            since: Some("30m".into()),
            start: None,
            end: None,
        };

        let error = meter_window(&args, now).unwrap_err();

        assert!(error.message.contains("complete hour"));
    }
}
