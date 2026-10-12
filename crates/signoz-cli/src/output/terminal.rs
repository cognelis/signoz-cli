use std::borrow::Cow;
use std::collections::BTreeSet;
use std::io::{self, Write};

use console::{Term, measure_text_width, truncate_str};
use serde_json::{Map, Value, json};

pub(super) fn width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .or_else(|| {
            Term::stdout()
                .size_checked()
                .map(|(_, width)| usize::from(width))
        })
        .unwrap_or(100)
        .clamp(8, 512)
}

/// Return whether any displayed text was shortened; the JSON payload is never modified.
pub(super) fn write(
    output: &mut impl Write,
    command: &str,
    data: &Value,
    width: usize,
    current_profile: Option<&str>,
) -> io::Result<bool> {
    let mut renderer = Renderer {
        output,
        command,
        width: width.max(8),
        shortened: false,
    };
    if command == "profile.list"
        && let Some(profiles) = data.as_object()
    {
        renderer.profiles(profiles, current_profile)?;
    } else if command == "config.show"
        && let Some(profiles) = data.get("profiles").and_then(Value::as_object)
    {
        let mut details = data
            .as_object()
            .expect("profiles belongs to an object")
            .clone();
        details.remove("profiles");
        renderer.value(&Value::Object(details), 0, false)?;
        renderer.heading("Profiles", 0)?;
        renderer.profiles(
            profiles,
            data.get("current_profile").and_then(Value::as_str),
        )?;
    } else {
        renderer.value(data, 0, false)?;
    }
    Ok(renderer.shortened)
}

struct Renderer<'a, W> {
    output: &'a mut W,
    command: &'a str,
    width: usize,
    shortened: bool,
}

impl<W: Write> Renderer<'_, W> {
    fn profiles(&mut self, profiles: &Map<String, Value>, current: Option<&str>) -> io::Result<()> {
        let rows: Vec<Value> = profiles.iter().map(|(name, profile)| {
            let mut row = json!({"current": if current == Some(name) { "*" } else { "" }, "name": name, "url": profile.get("url").unwrap_or(&Value::Null)});
            // The column appears only once some profile is described.
            if let Some(description) = profile.get("description") {
                row["description"] = description.clone();
            }
            row
        }).collect();
        self.array(&rows, 0, false)
    }

    fn value(&mut self, value: &Value, indent: usize, query_rows: bool) -> io::Result<()> {
        // Only remove the known API envelope, never a domain object's other fields.
        let mut value = value;
        while let Some(object) = value.as_object() {
            if object.contains_key("data")
                && object
                    .keys()
                    .all(|key| matches!(key.as_str(), "data" | "status" | "statusCode"))
                && object
                    .get("status")
                    .is_none_or(|status| matches!(status.as_str(), Some("success" | "ok")))
                && object.get("statusCode").is_none_or(|status| {
                    status
                        .as_u64()
                        .is_some_and(|status| (200..300).contains(&status))
                })
            {
                value = &object["data"];
            } else {
                break;
            }
        }
        match value {
            Value::Array(rows) => self.array(rows, indent, query_rows),
            Value::Object(object) => self.details(object, indent),
            Value::Null => self.line(if query_rows { "(no rows)" } else { "(no data)" }, indent),
            scalar => self.line(&cell(scalar), indent),
        }
    }

    fn details(&mut self, object: &Map<String, Value>, indent: usize) -> io::Result<()> {
        if object.is_empty() {
            return self.line("(empty object)", indent);
        }
        let keys = ordered_keys(self.command, object.keys().map(String::as_str));
        let available = self.available(indent);
        let label_width = keys
            .iter()
            .filter(|key| !is_section(key, &object[**key]))
            .map(|key| measure_text_width(&safe_text(key)))
            .max()
            .unwrap_or(1)
            .min((available / 3).max(1))
            .min(30);
        for key in &keys {
            let value = &object[*key];
            if is_section(key, value) {
                continue;
            }
            let label = self.fit(&safe_text(key), label_width);
            let value = self.fit(
                &cell(value),
                available.saturating_sub(label_width + 2).max(1),
            );
            let padding = " ".repeat(label_width.saturating_sub(measure_text_width(&label)));
            self.line(&format!("{label}{padding}: {value}"), indent)?;
        }
        for key in keys {
            let value = &object[key];
            if is_section(key, value) {
                self.heading(key, indent)?;
                self.value(value, self.child_indent(indent), key == "rows")?;
            }
        }
        Ok(())
    }

    fn array(&mut self, rows: &[Value], indent: usize, query_rows: bool) -> io::Result<()> {
        if rows.is_empty() {
            return self.line("(no rows)", indent);
        }
        // Query results and time series contain their own row collections. Render each section.
        if !query_rows && rows.iter().any(contains_collection) {
            for (index, value) in rows.iter().enumerate() {
                self.heading(&format!("[{}]", index + 1), indent)?;
                self.value(value, self.child_indent(indent), false)?;
            }
            return Ok(());
        }
        let rows: Vec<Cow<'_, Value>> = rows
            .iter()
            .map(|row| {
                if query_rows {
                    expand_query_row(row)
                } else {
                    Cow::Borrowed(row)
                }
            })
            .collect();
        let columns = ordered_keys(
            self.command,
            rows.iter().flat_map(|row| match row.as_object() {
                Some(object) => object.keys().map(String::as_str).collect::<Vec<_>>(),
                None => vec!["value"],
            }),
        );
        if columns.is_empty() {
            for row in &rows {
                self.value(row, indent, false)?;
            }
            return Ok(());
        }
        let cells: Vec<Vec<String>> = rows
            .iter()
            .map(|row| {
                columns
                    .iter()
                    .map(|key| {
                        if let Some(object) = row.as_object() {
                            object.get(*key).map(cell).unwrap_or_else(|| "—".into())
                        } else if *key == "value" {
                            cell(row)
                        } else {
                            "—".into()
                        }
                    })
                    .collect()
            })
            .collect();
        let headers: Vec<_> = columns.iter().map(|key| safe_text(key)).collect();
        let mut widths: Vec<_> = headers
            .iter()
            .enumerate()
            .map(|(index, header)| {
                cells
                    .iter()
                    .map(|row| measure_text_width(&row[index]))
                    .chain(std::iter::once(measure_text_width(header)))
                    .max()
                    .unwrap_or(1)
                    .clamp(1, self.available(indent))
            })
            .collect();
        let minimums: Vec<_> = widths.iter().map(|width| (*width).min(8)).collect();
        let spacing = 2 * columns.len().saturating_sub(1);
        let available = self.available(indent);
        if minimums.iter().sum::<usize>() + spacing > available {
            // Keep every field visible, including fields that appear only in later records.
            for (index, row) in rows.iter().enumerate() {
                self.heading(&format!("[{}]", index + 1), indent)?;
                self.value(row, self.child_indent(indent), false)?;
            }
            return Ok(());
        }
        while widths.iter().sum::<usize>() + spacing > available {
            let index = (0..widths.len())
                .filter(|i| widths[*i] > minimums[*i])
                .max_by_key(|i| widths[*i])
                .expect("minimum widths fit");
            widths[index] -= 1;
        }
        self.table_line(&headers, &widths, indent)?;
        self.table_line(
            &widths
                .iter()
                .map(|width| "-".repeat(*width))
                .collect::<Vec<_>>(),
            &widths,
            indent,
        )?;
        for row in cells {
            self.table_line(&row, &widths, indent)?;
        }
        Ok(())
    }

    fn table_line(&mut self, cells: &[String], widths: &[usize], indent: usize) -> io::Result<()> {
        let mut line = String::new();
        for (index, (value, width)) in cells.iter().zip(widths).enumerate() {
            let value = self.fit(value, *width);
            line.push_str(&value);
            if index + 1 < cells.len() {
                line.push_str(&" ".repeat(width.saturating_sub(measure_text_width(&value)) + 2));
            }
        }
        self.line(&line, indent)
    }

    fn heading(&mut self, text: &str, indent: usize) -> io::Result<()> {
        self.line(&format!("{}:", safe_text(text)), indent)
    }

    fn fit(&mut self, value: &str, width: usize) -> String {
        let text = truncate_str(value, width.max(1), "…");
        self.shortened |= matches!(text, Cow::Owned(_));
        text.into_owned()
    }

    fn line(&mut self, text: &str, indent: usize) -> io::Result<()> {
        let indent = indent.min(self.width.saturating_sub(4));
        let text = self.fit(text, self.available(indent));
        writeln!(self.output, "{}{text}", " ".repeat(indent))
    }

    fn available(&self, indent: usize) -> usize {
        self.width.saturating_sub(indent).max(1)
    }
    fn child_indent(&self, indent: usize) -> usize {
        (indent + 2).min(self.width.saturating_sub(4))
    }
}

fn contains_collection(row: &Value) -> bool {
    row.as_object().is_some_and(|object| {
        object.contains_key("rows")
            || object.contains_key("series")
            || object.values().any(|value| {
                value
                    .as_array()
                    .is_some_and(|rows| rows.iter().any(|row| row.is_object() || row.is_array()))
            })
    })
}

fn is_section(key: &str, value: &Value) -> bool {
    value.is_object() || value.is_array() || (key == "rows" && value.is_null())
}

fn expand_query_row(row: &Value) -> Cow<'_, Value> {
    if let Some(object) = row.as_object()
        && let Some(data) = object.get("data").and_then(Value::as_object)
    {
        // A collision must retain both values and their original nesting.
        if data
            .keys()
            .any(|key| object.contains_key(key) && key != "data")
        {
            return Cow::Borrowed(row);
        }
        let mut expanded = object.clone();
        expanded.remove("data");
        expanded.extend(data.clone());
        return Cow::Owned(Value::Object(expanded));
    }
    Cow::Borrowed(row)
}

fn ordered_keys<'a>(command: &str, keys: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let preferred: &[&str] = match command {
        "doctor" => &["name", "status", "message"],
        "profile.list" | "config.show" => {
            &["current", "name", "description", "url", "current_profile"]
        }
        "skills.install" => &["agent", "status", "path"],
        "metrics.list" | "metrics.top" => &[
            "metricName",
            "type",
            "temporality",
            "isMonotonic",
            "samples",
        ],
        "logs.search" => &[
            "timestamp",
            "severity_text",
            "service.name",
            "body",
            "trace_id",
            "id",
        ],
        "traces.search" | "traces.get" => &[
            "timestamp",
            "service.name",
            "name",
            "duration_nano",
            "has_error",
            "trace_id",
            "span_id",
        ],
        _ => &["timestamp", "name", "id", "status", "type", "value"],
    };
    let mut remaining: BTreeSet<_> = keys.collect();
    let mut ordered = Vec::new();
    for preferred in preferred {
        if let Some(key) = remaining.take(preferred) {
            ordered.push(key);
        }
    }
    ordered.extend(remaining);
    ordered
}

fn cell(value: &Value) -> String {
    match value {
        Value::String(value) => safe_text(value),
        value => safe_text(&value.to_string()),
    }
}

pub(super) fn safe_text(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            ch if ch.is_control()
                || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') =>
            {
                result.extend(ch.escape_unicode());
            }
            ch => result.push(ch),
        }
    }
    result
}

#[cfg(test)]
mod tests;
