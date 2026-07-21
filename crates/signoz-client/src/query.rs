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

pub fn log_aggregate(window: TimeWindow, filter: Option<&str>, aggregation: &str) -> Value {
    aggregate_query(window, "logs", filter.unwrap_or_default(), aggregation)
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

pub fn trace_aggregate(window: TimeWindow, filter: Option<&str>, aggregation: &str) -> Value {
    aggregate_query(window, "traces", filter.unwrap_or_default(), aggregation)
}

pub fn trace_get(window: TimeWindow, trace_id: &str) -> Value {
    trace_search(
        window,
        Some(&format!("trace_id = '{}'", escape_literal(trace_id))),
        10_000,
        0,
    )
}

pub fn metric_query(
    window: TimeWindow,
    metric: &str,
    aggregation: &MetricAggregation,
    limit: u32,
    offset: u32,
) -> Value {
    envelope(
        window,
        "time_series",
        json!({
            "name": "A",
            "signal": "metrics",
            "stepInterval": 60,
            "disabled": false,
            "filter": {"expression": ""},
            "limit": limit.max(1),
            "offset": offset,
            "order": [{"key": {"name": "__result"}, "direction": "desc"}],
            "having": {"expression": ""},
            "selectFields": [],
            "aggregations": [{
                "metricName": metric,
                "temporality": aggregation.temporality,
                "timeAggregation": aggregation.time_aggregation,
                "spaceAggregation": aggregation.space_aggregation
            }]
        }),
    )
}

fn aggregate_query(window: TimeWindow, signal: &str, filter: &str, aggregation: &str) -> Value {
    let expression = if aggregation.contains('(') {
        aggregation.to_owned()
    } else {
        format!("{aggregation}()")
    };
    envelope(
        window,
        "scalar",
        json!({
            "name": "A",
            "signal": signal,
            "disabled": false,
            "filter": {"expression": filter},
            "limit": 1,
            "order": [{"key": {"name": expression}, "direction": "desc"}],
            "having": {"expression": ""},
            "selectFields": [],
            "groupBy": [],
            "aggregations": [{"expression": expression}]
        }),
    )
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
        log_search, metric_metadata, metric_query, resolve_metric_aggregation, trace_search,
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
        let metrics = metric_query(window, "http.server.duration", &aggregation, 50, 2);
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
        assert_eq!(metrics["start"], 1_000);
        assert_eq!(metrics["end"], 2_000);
        assert!(metrics.to_string().contains("http.server.duration"));
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
}
