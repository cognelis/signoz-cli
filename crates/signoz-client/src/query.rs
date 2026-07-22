use serde_json::{Value, json};

use crate::TimeWindow;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetricMetadata {
    pub metric_type: String,
    pub temporality: String,
    pub is_monotonic: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetricAggregation {
    pub temporality: String,
    pub time_aggregation: String,
    pub space_aggregation: String,
}

pub fn metric_metadata(response: &Value, metric: &str) -> Result<MetricMetadata, String> {
    let candidates = response
        .pointer("/data/metrics")
        .or_else(|| response.pointer("/data/items"))
        .or_else(|| response.get("data"))
        .or_else(|| response.as_array().map(|_| response))
        .and_then(Value::as_array)
        .ok_or_else(|| "metric catalog response has no metrics array".to_owned())?;
    let found = candidates
        .iter()
        .find(|value| value.get("metricName").and_then(Value::as_str) == Some(metric))
        .ok_or_else(|| format!("metric '{metric}' was not found in the SigNoz catalog"))?;
    let metric_type = found
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(
        metric_type.as_str(),
        "gauge" | "sum" | "histogram" | "exponential_histogram" | "exponentialhistogram"
    ) {
        return Err(format!(
            "metric '{metric}' has unsupported or missing type '{metric_type}'"
        ));
    }
    let temporality = found
        .get("temporality")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            format!(
                "metric '{metric}' metadata is missing temporality; refusing to guess an aggregation"
            )
        })?
        .to_ascii_lowercase();
    let is_monotonic = match metric_type.as_str() {
        "sum" => found
            .get("isMonotonic")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                format!(
                    "metric '{metric}' sum metadata is missing isMonotonic; refusing to guess an aggregation"
                )
            })?,
        _ => found
            .get("isMonotonic")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };
    Ok(MetricMetadata {
        metric_type: if metric_type == "exponentialhistogram" {
            "exponential_histogram".into()
        } else {
            metric_type
        },
        temporality,
        is_monotonic,
    })
}

pub fn resolve_metric_aggregation(
    metadata: &MetricMetadata,
    space_override: Option<&str>,
) -> Result<MetricAggregation, String> {
    let (time_default, space_default, allowed): (&str, &str, &[&str]) = match metadata
        .metric_type
        .as_str()
    {
        "gauge" => ("avg", "sum", &["sum", "avg", "min", "max", "count"]),
        "sum" if metadata.is_monotonic => ("rate", "sum", &["sum", "avg", "min", "max", "count"]),
        "sum" => ("avg", "sum", &["sum", "avg", "min", "max", "count"]),
        "histogram" | "exponential_histogram" => ("", "p99", &["p50", "p75", "p90", "p95", "p99"]),
        value => return Err(format!("unsupported metric type '{value}'")),
    };
    let space = space_override.unwrap_or(space_default).to_ascii_lowercase();
    if !allowed.contains(&space.as_str()) {
        return Err(format!(
            "aggregation '{space}' is not valid for metric type '{}'; valid values: {}",
            metadata.metric_type,
            allowed.join(", ")
        ));
    }
    Ok(MetricAggregation {
        temporality: metadata.temporality.clone(),
        time_aggregation: time_default.into(),
        space_aggregation: space,
    })
}

pub fn log_search(window: TimeWindow, search: Option<&str>, limit: u32, offset: u32) -> Value {
    let filter = search
        .map(|value| format!("body CONTAINS '{}'", escape_literal(value)))
        .unwrap_or_default();
    envelope(
        window,
        "raw",
        json!({
            "name": "A",
            "signal": "logs",
            "disabled": false,
            "filter": {"expression": filter},
            "limit": limit.max(1),
            "offset": offset,
            "order": [
                {"key": {"name": "timestamp"}, "direction": "desc"},
                {"key": {"name": "id"}, "direction": "desc"}
            ],
            "having": {"expression": ""},
            "selectFields": []
        }),
    )
}

pub fn log_aggregate(
    window: TimeWindow,
    filter: Option<&str>,
    aggregation: &str,
    group_by: &[String],
    step_seconds: Option<u32>,
    limit: u32,
) -> Value {
    aggregate_query(
        window,
        "logs",
        filter.unwrap_or_default(),
        aggregation,
        group_by,
        step_seconds,
        limit,
    )
}

pub fn trace_search(window: TimeWindow, filter: Option<&str>, limit: u32, offset: u32) -> Value {
    envelope(
        window,
        "raw",
        json!({
            "name": "A",
            "signal": "traces",
            "disabled": false,
            "filter": {"expression": filter.unwrap_or_default()},
            "limit": limit.max(1),
            "offset": offset,
            "order": [{"key": {"name": "timestamp"}, "direction": "desc"}],
            "having": {"expression": ""},
            "selectFields": [
                {"name": "service.name", "fieldDataType": "string", "signal": "traces", "fieldContext": "resource"},
                {"name": "name", "fieldDataType": "string", "signal": "traces", "fieldContext": "span"},
                {"name": "has_error", "fieldDataType": "bool", "signal": "traces", "fieldContext": "span"},
                {"name": "duration_nano", "fieldDataType": "number", "signal": "traces", "fieldContext": "span"},
                {"name": "status_message", "fieldDataType": "string", "signal": "traces", "fieldContext": "span"},
                {"name": "trace_id", "fieldDataType": "string", "signal": "traces", "fieldContext": "span"},
                {"name": "span_id", "fieldDataType": "string", "signal": "traces", "fieldContext": "span"}
            ]
        }),
    )
}

pub fn trace_aggregate(
    window: TimeWindow,
    filter: Option<&str>,
    aggregation: &str,
    group_by: &[String],
    step_seconds: Option<u32>,
    limit: u32,
) -> Value {
    aggregate_query(
        window,
        "traces",
        filter.unwrap_or_default(),
        aggregation,
        group_by,
        step_seconds,
        limit,
    )
}

pub fn trace_get(window: TimeWindow, trace_id: &str) -> Value {
    trace_search(
        window,
        Some(&format!("trace_id = '{}'", escape_literal(trace_id))),
        10_000,
        0,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn metric_query(
    window: TimeWindow,
    metric: &str,
    aggregation: &MetricAggregation,
    filter: Option<&str>,
    group_by: &[String],
    step_seconds: u32,
    source: Option<&str>,
    limit: u32,
    offset: u32,
) -> Value {
    let mut spec = json!({
        "name": "A",
        "signal": "metrics",
        "stepInterval": step_seconds.max(1),
        "disabled": false,
        "filter": {"expression": filter.unwrap_or_default()},
        "limit": limit.max(1),
        "offset": offset,
        "order": [{"key": {"name": "__result"}, "direction": "desc"}],
        "having": {"expression": ""},
        "selectFields": [],
        "groupBy": group_by.iter().map(|name| json!({"name": name})).collect::<Vec<_>>(),
        "aggregations": [{
            "metricName": metric,
            "temporality": aggregation.temporality,
            "timeAggregation": aggregation.time_aggregation,
            "spaceAggregation": aggregation.space_aggregation
        }]
    });
    if let Some(source) = source.filter(|value| !value.trim().is_empty()) {
        spec["source"] = Value::String(source.to_owned());
    }
    envelope(window, "time_series", spec)
}

pub fn meter_query(
    window: TimeWindow,
    metrics: &[&str],
    filter: Option<&str>,
    group_by: &[String],
    step_seconds: u32,
) -> Value {
    let group_by: Vec<_> = group_by.iter().map(|name| json!({"name": name})).collect();
    let queries: Vec<_> = metrics
        .iter()
        .enumerate()
        .map(|(index, metric)| {
            json!({
                "type": "builder_query",
                "spec": {
                    "name": format!("M{}", index + 1),
                    "signal": "metrics",
                    "source": "meter",
                    "stepInterval": step_seconds.max(3_600),
                    "aggregations": [{
                        "metricName": metric,
                        "timeAggregation": "increase",
                        "spaceAggregation": "sum"
                    }],
                    "filter": {"expression": filter.unwrap_or_default()},
                    "groupBy": group_by,
                    "disabled": false
                }
            })
        })
        .collect();
    json!({
        "schemaVersion": "v1",
        "start": window.start_ms,
        "end": window.end_ms,
        "requestType": "time_series",
        "compositeQuery": {"queries": queries},
        "formatOptions": {"formatTableResultForUI": false, "fillGaps": false},
        "variables": {}
    })
}

fn aggregate_query(
    window: TimeWindow,
    signal: &str,
    filter: &str,
    aggregation: &str,
    group_by: &[String],
    step_seconds: Option<u32>,
    limit: u32,
) -> Value {
    let expression = if aggregation.contains('(') {
        aggregation.to_owned()
    } else {
        format!("{aggregation}()")
    };
    let request_type = if step_seconds.is_some() {
        "time_series"
    } else {
        "scalar"
    };
    let mut spec = json!({
        "name": "A",
        "signal": signal,
        "disabled": false,
        "filter": {"expression": filter},
        "limit": if group_by.is_empty() { 1 } else { limit.max(1) },
        "order": [{"key": {"name": expression}, "direction": "desc"}],
        "having": {"expression": ""},
        "selectFields": [],
        "groupBy": group_by.iter().map(|name| json!({"name": name})).collect::<Vec<_>>(),
        "aggregations": [{"expression": expression}]
    });
    if let Some(step_seconds) = step_seconds {
        spec["stepInterval"] = json!(step_seconds.max(1));
    }
    envelope(window, request_type, spec)
}

fn envelope(window: TimeWindow, request_type: &str, spec: Value) -> Value {
    json!({
        "schemaVersion": "v1",
        "start": window.start_ms,
        "end": window.end_ms,
        "requestType": request_type,
        "compositeQuery": {
            "queries": [{
                "type": "builder_query",
                "spec": spec
            }]
        },
        "formatOptions": {"formatTableResultForUI": false, "fillGaps": false},
        "variables": {}
    })
}

fn escape_literal(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        log_aggregate, log_search, meter_query, metric_metadata, metric_query,
        resolve_metric_aggregation, trace_search,
    };
    use crate::TimeWindow;

    #[test]
    fn query_builders_set_signal_window_limit_and_order() {
        let window = TimeWindow {
            start_ms: 1_000,
            end_ms: 2_000,
        };
        let logs = log_search(window, Some("error"), 25, 7);
        let traces = trace_search(window, Some("service.name = 'api'"), 10, 3);
        let metadata = metric_metadata(
            &json!({"data": {"metrics": [{
                "metricName": "http.server.duration",
                "type": "histogram",
                "temporality": "delta",
                "isMonotonic": false
            }]}}),
            "http.server.duration",
        )
        .unwrap();
        let aggregation = resolve_metric_aggregation(&metadata, None).unwrap();
        let metrics = metric_query(
            window,
            "http.server.duration",
            &aggregation,
            Some("service.name = 'api'"),
            &["service.name".into()],
            300,
            None,
            50,
            2,
        );
        assert_eq!(logs["schemaVersion"], "v1");
        assert_eq!(logs["requestType"], "raw");
        assert_eq!(
            logs["compositeQuery"]["queries"][0]["spec"]["signal"],
            "logs"
        );
        assert_eq!(
            traces["compositeQuery"]["queries"][0]["spec"]["signal"],
            "traces"
        );
        assert_eq!(
            logs["compositeQuery"]["queries"][0]["spec"]["order"][0]["key"]["name"],
            "timestamp"
        );
        assert_eq!(logs["compositeQuery"]["queries"][0]["spec"]["offset"], 7);
        assert_eq!(
            metrics["compositeQuery"]["queries"][0]["spec"]["aggregations"][0]["metricName"],
            "http.server.duration"
        );
        assert_eq!(
            metrics["compositeQuery"]["queries"][0]["spec"]["aggregations"][0]["spaceAggregation"],
            "p99"
        );
        assert_eq!(
            metrics["compositeQuery"]["queries"][0]["spec"]["filter"]["expression"],
            "service.name = 'api'"
        );
        assert_eq!(
            metrics["compositeQuery"]["queries"][0]["spec"]["groupBy"][0]["name"],
            "service.name"
        );
        assert_eq!(
            metrics["compositeQuery"]["queries"][0]["spec"]["stepInterval"],
            300
        );
        assert_eq!(metrics["start"], 1_000);
        assert_eq!(metrics["end"], 2_000);
        assert!(metrics.to_string().contains("http.server.duration"));
    }

    #[test]
    fn grouped_aggregations_can_return_bounded_time_series() {
        let query = log_aggregate(
            TimeWindow {
                start_ms: 1_000,
                end_ms: 2_000,
            },
            Some("severity_text = 'ERROR'"),
            "count()",
            &["service.name".into()],
            Some(300),
            25,
        );
        let spec = &query["compositeQuery"]["queries"][0]["spec"];
        assert_eq!(query["requestType"], "time_series");
        assert_eq!(spec["stepInterval"], 300);
        assert_eq!(spec["limit"], 25);
        assert_eq!(spec["groupBy"][0]["name"], "service.name");
    }

    #[test]
    fn metric_aggregation_defaults_follow_metric_semantics() {
        let gauge = super::MetricMetadata {
            metric_type: "gauge".into(),
            temporality: "unspecified".into(),
            is_monotonic: false,
        };
        let counter = super::MetricMetadata {
            metric_type: "sum".into(),
            temporality: "cumulative".into(),
            is_monotonic: true,
        };
        let non_monotonic_sum = super::MetricMetadata {
            metric_type: "sum".into(),
            temporality: "delta".into(),
            is_monotonic: false,
        };
        assert_eq!(
            resolve_metric_aggregation(&gauge, None)
                .unwrap()
                .time_aggregation,
            "avg"
        );
        assert_eq!(
            resolve_metric_aggregation(&counter, None)
                .unwrap()
                .time_aggregation,
            "rate"
        );
        assert_eq!(
            resolve_metric_aggregation(&non_monotonic_sum, None)
                .unwrap()
                .time_aggregation,
            "avg"
        );
        assert!(resolve_metric_aggregation(&gauge, Some("p99")).is_err());
    }

    #[test]
    fn metric_metadata_rejects_missing_fields_instead_of_guessing() {
        let missing_temporality = json!({"data": {"metrics": [{
            "metricName": "cpu",
            "type": "gauge"
        }]}});
        let missing_monotonicity = json!({"data": {"metrics": [{
            "metricName": "requests",
            "type": "sum",
            "temporality": "cumulative"
        }]}});
        assert!(metric_metadata(&missing_temporality, "cpu").is_err());
        assert!(metric_metadata(&missing_monotonicity, "requests").is_err());
    }

    #[test]
    fn meter_queries_use_the_documented_source_and_increase_aggregation() {
        let body = meter_query(
            TimeWindow {
                start_ms: 1_000,
                end_ms: 2_000,
            },
            &["signoz.meter.log.size", "signoz.meter.span.size"],
            Some("service.name = 'api'"),
            &["service.name".into()],
            86_400,
        );
        let queries = body["compositeQuery"]["queries"].as_array().unwrap();
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0]["spec"]["source"], "meter");
        assert_eq!(
            queries[0]["spec"]["aggregations"][0]["timeAggregation"],
            "increase"
        );
        assert_eq!(queries[0]["spec"]["stepInterval"], 86_400);
        assert_eq!(queries[0]["spec"]["groupBy"][0]["name"], "service.name");
    }
}
