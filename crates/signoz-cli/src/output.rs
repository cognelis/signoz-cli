use std::io::{self, IsTerminal};

use serde::Serialize;
use serde_json::Value;
use signoz_client::ErrorCode;

use crate::args::OutputFormat;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u128>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuccessEnvelope {
    pub schema_version: &'static str,
    pub command: String,
    pub data: Value,
    pub meta: Meta,
}

impl SuccessEnvelope {
    pub fn new(command: impl Into<String>, data: Value, meta: Meta) -> Self {
        Self {
            schema_version: "1",
            command: command.into(),
            data,
            meta,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorEnvelope<'a> {
    schema_version: &'static str,
    error: ErrorBody<'a>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: ErrorCode,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<&'a str>,
}

pub fn emit_success(
    format: OutputFormat,
    raw: bool,
    command: &str,
    data: Value,
    meta: Meta,
) -> Result<(), String> {
    if raw {
        println!(
            "{}",
            serde_json::to_string_pretty(&data).map_err(|error| error.to_string())?
        );
        return Ok(());
    }
    let effective = match format {
        OutputFormat::Auto if io::stdout().is_terminal() => OutputFormat::Table,
        OutputFormat::Auto => OutputFormat::Json,
        value => value,
    };
    match effective {
        OutputFormat::Json | OutputFormat::Auto => println!(
            "{}",
            serde_json::to_string_pretty(&SuccessEnvelope::new(command, data, meta))
                .map_err(|error| error.to_string())?
        ),
        OutputFormat::Table => print_table(&data),
    }
    Ok(())
}

pub fn emit_error(
    format: OutputFormat,
    raw: bool,
    code: ErrorCode,
    message: &str,
    status: Option<u16>,
    hint: Option<&str>,
) {
    let json_mode = raw || matches!(format, OutputFormat::Json) || !io::stderr().is_terminal();
    if json_mode {
        let envelope = ErrorEnvelope {
            schema_version: "1",
            error: ErrorBody {
                code,
                message,
                status,
                hint,
            },
        };
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&envelope)
                .unwrap_or_else(|_| "{\"error\":{\"code\":\"incompatible_response\"}}".into())
        );
    } else {
        eprintln!("error: {message}");
        if let Some(hint) = hint {
            eprintln!("hint: {hint}");
        }
    }
}

pub fn exit_code(code: ErrorCode) -> i32 {
    match code {
        ErrorCode::Validation => 2,
        ErrorCode::Config | ErrorCode::Unauthorized | ErrorCode::Forbidden => 3,
        ErrorCode::Network | ErrorCode::Timeout => 4,
        ErrorCode::NotFound | ErrorCode::RequestRejected | ErrorCode::RateLimited => 5,
        ErrorCode::Upstream => 6,
        ErrorCode::FeatureUnsupported
        | ErrorCode::ResponseTooLarge
        | ErrorCode::IncompatibleResponse => 7,
    }
}

fn print_table(value: &Value) {
    let rows = value
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![value.clone()]);
    if rows.is_empty() {
        println!("(no rows)");
        return;
    }
    if let Some(first) = rows.first().and_then(Value::as_object) {
        let headers: Vec<_> = first.keys().cloned().collect();
        println!("{}", headers.join("\t"));
        for row in rows {
            if let Some(object) = row.as_object() {
                let values = headers
                    .iter()
                    .map(|header| scalar(object.get(header).unwrap_or(&Value::Null)))
                    .collect::<Vec<_>>();
                println!("{}", values.join("\t"));
            }
        }
    } else {
        for row in rows {
            println!("{}", scalar(&row));
        }
    }
}

fn scalar(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}
