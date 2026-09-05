use std::io::{self, IsTerminal, Write};

use serde::Serialize;
use serde_json::Value;
use signoz_client::ErrorCode;

use crate::args::OutputFormat;

mod terminal;

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
) -> io::Result<()> {
    let effective = effective_format(format, io::stdout().is_terminal());
    let mut output = io::BufWriter::new(io::stdout().lock());
    let mut diagnostics = io::stderr().lock();
    write_success(
        &mut output,
        &mut diagnostics,
        effective,
        raw,
        command,
        data,
        meta,
    )?;
    output.flush()
}

fn write_success(
    output: &mut impl Write,
    diagnostics: &mut impl Write,
    format: OutputFormat,
    raw: bool,
    command: &str,
    data: Value,
    meta: Meta,
) -> io::Result<()> {
    if command == "completion" {
        let script = data.as_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "completion output must be a script",
            )
        })?;
        return output.write_all(script.as_bytes());
    }
    if raw {
        return write_json(output, &data);
    }
    match format {
        OutputFormat::Json | OutputFormat::Auto => {
            write_json(output, &SuccessEnvelope::new(command, data, meta))
        }
        OutputFormat::Table => {
            let shortened = terminal::write(
                output,
                command,
                &data,
                terminal::width(),
                meta.profile.as_deref(),
            )?;
            output.flush()?;
            write_table_meta(diagnostics, &meta)?;
            if shortened {
                writeln!(
                    diagnostics,
                    "hint: displayed values were shortened; use --output json for full values"
                )?;
            }
            Ok(())
        }
    }
}

fn write_json(output: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *output, value)?;
    writeln!(output)
}

fn effective_format(format: OutputFormat, stdout_is_terminal: bool) -> OutputFormat {
    match format {
        OutputFormat::Auto if stdout_is_terminal => OutputFormat::Table,
        OutputFormat::Auto => OutputFormat::Json,
        explicit => explicit,
    }
}

fn write_table_meta(diagnostics: &mut impl Write, meta: &Meta) -> io::Result<()> {
    if let Some(profile) = &meta.profile {
        writeln!(diagnostics, "Profile: {}", terminal::safe_text(profile))?;
    }
    if let (Some(start), Some(end)) = (&meta.start, &meta.end) {
        writeln!(
            diagnostics,
            "Time: {} to {}",
            terminal::safe_text(start),
            terminal::safe_text(end)
        )?;
    }
    if meta.has_more == Some(true) {
        writeln!(
            diagnostics,
            "Results: partial; more results may be available"
        )?;
        if let Some(offset) = meta.next_offset {
            writeln!(diagnostics, "hint: continue with --offset {offset}")?;
        }
        if let Some(cursor) = &meta.next_cursor {
            writeln!(
                diagnostics,
                "hint: continue with --cursor '{}'",
                terminal::safe_text(cursor).replace('\'', "'\\''")
            )?;
        }
    }
    for warning in &meta.warnings {
        writeln!(diagnostics, "warning: {}", terminal::safe_text(warning))?;
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
    let mut diagnostics = io::stderr().lock();
    let json_mode = raw
        || matches!(
            effective_format(format, io::stdout().is_terminal()),
            OutputFormat::Json
        );
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
        let _ = write_json(&mut diagnostics, &envelope);
    } else {
        let _ = writeln!(diagnostics, "error: {}", terminal::safe_text(message));
        if let Some(hint) = hint {
            let _ = writeln!(diagnostics, "hint: {}", terminal::safe_text(hint));
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn auto_format_depends_on_stdout_and_explicit_formats_always_win() {
        assert!(matches!(
            effective_format(OutputFormat::Auto, true),
            OutputFormat::Table
        ));
        assert!(matches!(
            effective_format(OutputFormat::Auto, false),
            OutputFormat::Json
        ));
        for terminal in [true, false] {
            assert!(matches!(
                effective_format(OutputFormat::Json, terminal),
                OutputFormat::Json
            ));
            assert!(matches!(
                effective_format(OutputFormat::Table, terminal),
                OutputFormat::Table
            ));
        }
    }

    fn incomplete_meta() -> Meta {
        Meta {
            profile: Some("prod".into()),
            has_more: Some(true),
            next_offset: Some(100),
            warnings: vec!["query reached its limit".into()],
            ..Meta::default()
        }
    }

    #[test]
    fn table_output_preserves_profile_completeness_and_warnings_on_stderr() {
        let mut output = Vec::new();
        let mut diagnostics = Vec::new();
        write_success(
            &mut output,
            &mut diagnostics,
            OutputFormat::Table,
            false,
            "logs.search",
            json!([{"body": "hello"}]),
            incomplete_meta(),
        )
        .unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "body\n-----\nhello\n");
        let diagnostics = String::from_utf8(diagnostics).unwrap();
        assert!(diagnostics.contains("Profile: prod"));
        assert!(diagnostics.contains("Results: partial"));
        assert!(diagnostics.contains("--offset 100"));
        assert!(diagnostics.contains("warning: query reached its limit"));
    }

    #[test]
    fn cursor_and_limit_only_results_do_not_suggest_offset_pagination() {
        for cursor in [None, Some("next-token".into())] {
            let meta = Meta {
                has_more: Some(true),
                next_cursor: cursor.clone(),
                ..Meta::default()
            };
            let mut diagnostics = Vec::new();
            write_table_meta(&mut diagnostics, &meta).unwrap();
            let text = String::from_utf8(diagnostics).unwrap();
            assert!(text.contains("Results: partial"));
            assert!(!text.contains("--offset"));
            assert_eq!(text.contains("--cursor"), cursor.is_some());
        }
    }

    #[test]
    fn json_keeps_its_envelope_and_raw_keeps_only_data_without_diagnostics() {
        for (format, raw) in [
            (OutputFormat::Json, false),
            (OutputFormat::Auto, false),
            (OutputFormat::Table, true),
        ] {
            let mut output = Vec::new();
            let mut diagnostics = Vec::new();
            let data = json!([{"body": "hello"}]);
            write_success(
                &mut output,
                &mut diagnostics,
                format,
                raw,
                "logs.search",
                data.clone(),
                incomplete_meta(),
            )
            .unwrap();
            let value: Value = serde_json::from_slice(&output).unwrap();
            if raw {
                assert_eq!(value, data);
            } else {
                assert_eq!(value["data"], data);
                assert_eq!(value["schemaVersion"], "1");
                assert_eq!(value["meta"]["profile"], "prod");
                assert_eq!(value["meta"]["hasMore"], true);
            }
            assert!(diagnostics.is_empty());
        }
    }

    #[test]
    fn serialization_and_tables_preserve_io_error_kinds() {
        struct FailedWriter(io::ErrorKind);
        impl Write for FailedWriter {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(self.0.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        for kind in [io::ErrorKind::BrokenPipe, io::ErrorKind::PermissionDenied] {
            for (format, raw) in [
                (OutputFormat::Json, false),
                (OutputFormat::Table, false),
                (OutputFormat::Json, true),
            ] {
                let error = write_success(
                    &mut FailedWriter(kind),
                    &mut Vec::new(),
                    format,
                    raw,
                    "demo",
                    json!([{"a": 1}]),
                    Meta::default(),
                )
                .unwrap_err();
                assert_eq!(error.kind(), kind);
            }
        }
    }
}
