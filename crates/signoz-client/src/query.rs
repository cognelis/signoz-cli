use serde_json::{Value, json};

use crate::TimeWindow;

pub fn log_search(window: TimeWindow, search: Option<&str>, limit: u32) -> Value {
    query_builder(
        window,
        "logs",
        "raw",
        search.map(|value| format!("body contains '{}'", escape_literal(value))),
        limit,
        "timestamp",
        None,
    )
}

pub fn log_aggregate(
    window: TimeWindow,
    filter: Option<&str>,
    aggregation: &str,
    limit: u32,
) -> Value {
    query_builder(
        window,
        "logs",
        "builder",
        filter.map(str::to_owned),
        limit,
        "value",
        Some(aggregation),
    )
}

pub fn trace_search(window: TimeWindow, filter: Option<&str>, limit: u32) -> Value {
    query_builder(
        window,
        "traces",
        "raw",
        filter.map(str::to_owned),
        limit,
        "timestamp",
        None,
    )
}

pub fn trace_aggregate(
    window: TimeWindow,
    filter: Option<&str>,
    aggregation: &str,
    limit: u32,
) -> Value {
    query_builder(
        window,
        "traces",
        "builder",
        filter.map(str::to_owned),
        limit,
        "value",
        Some(aggregation),
    )
}

pub fn trace_get(window: TimeWindow, trace_id: &str) -> Value {
    trace_search(
        window,
        Some(&format!("trace_id = '{}'", escape_literal(trace_id))),
        10_000,
    )
}

pub fn metric_query(
    window: TimeWindow,
    metric: &str,
    aggregation: &str,
    limit: u32,
) -> Value {
    query_builder(
        window,
        "metrics",
        "builder",
        None,
        limit,
        "value",
        Some(&format!("{aggregation}:{metric}")),
    )
}

fn query_builder(
    window: TimeWindow,
    signal: &str,
    request_type: &str,
    filter: Option<String>,
    limit: u32,
    order_by: &str,
    aggregation: Option<&str>,
) -> Value {
    json!({
        "start": window.start_ms,
        "end": window.end_ms,
        "requestType": request_type,
        "compositeQuery": {
            "queryType": "builder",
            "panelType": "table",
            "queries": [{
                "type": "builder_query",
                "spec": {
                    "name": "A",
                    "signal": signal,
                    "stepInterval": 60,
                    "disabled": false
                },
                "builder": {
                    "queryData": [{
                        "queryName": "A",
                        "dataSource": signal,
                        "aggregateOperator": aggregation,
                        "filter": {"expression": filter.unwrap_or_default()},
                        "limit": limit.max(1),
                        "orderBy": [{"columnName": order_by, "order": "desc"}]
                    }]
                }
            }]
        }
    })
}

fn escape_literal(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use super::{log_search, metric_query, trace_search};
    use crate::TimeWindow;

    #[test]
    fn query_builders_set_signal_window_limit_and_order() {
        let window = TimeWindow {
            start_ms: 1_000,
            end_ms: 2_000,
        };
        let logs = log_search(window, Some("error"), 25);
        let traces = trace_search(window, Some("service.name = 'api'"), 10);
        let metrics = metric_query(window, "http.server.duration", "avg", 50);
        assert_eq!(logs["requestType"], "raw");
        assert_eq!(logs["compositeQuery"]["queries"][0]["builder"]["queryData"][0]["dataSource"], "logs");
        assert_eq!(traces["compositeQuery"]["queries"][0]["builder"]["queryData"][0]["dataSource"], "traces");
        assert_eq!(metrics["start"], 1_000);
        assert_eq!(metrics["end"], 2_000);
        assert!(metrics.to_string().contains("http.server.duration"));
    }
}
