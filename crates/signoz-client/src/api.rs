use reqwest::Method;
use serde_json::Value;
use std::time::Duration;

use crate::{ClientError, Credentials, HttpTransport, JsonResponse, SigNozOrigin};

#[derive(Clone, Debug)]
pub struct Operation {
    method: Method,
    path: String,
    query: Vec<(String, String)>,
    body: Option<Value>,
    replay_safe: bool,
    minimum_version: Option<&'static str>,
}

#[derive(Clone, Copy, Debug)]
pub struct FieldQuery<'a> {
    pub source: Option<&'a str>,
    pub limit: u32,
    pub start_ms: i64,
    pub end_ms: i64,
    pub context: Option<&'a str>,
    pub data_type: Option<&'a str>,
    pub metric_name: Option<&'a str>,
    pub metric_namespace: Option<&'a str>,
    pub search: Option<&'a str>,
}

impl Operation {
    fn get(path: impl Into<String>) -> Self {
        Self {
            method: Method::GET,
            path: path.into(),
            query: Vec::new(),
            body: None,
            replay_safe: true,
            minimum_version: None,
        }
    }

    fn post(path: impl Into<String>, body: Value) -> Self {
        Self {
            method: Method::POST,
            path: path.into(),
            query: Vec::new(),
            body: Some(body),
            replay_safe: true,
            minimum_version: None,
        }
    }

    fn query(mut self, key: &str, value: impl ToString) -> Self {
        self.query.push((key.to_owned(), value.to_string()));
        self
    }

    fn minimum_version(mut self, version: &'static str) -> Self {
        self.minimum_version = Some(version);
        self
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn query_value(&self, key: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.as_str())
    }

    pub fn auth_me() -> Self {
        Self::get("/api/v1/service_accounts/me")
    }

    pub fn metrics_list(
        start_ms: i64,
        end_ms: i64,
        limit: u32,
        search: Option<&str>,
        source: Option<&str>,
    ) -> Self {
        let mut operation = Self::get("/api/v2/metrics")
            .query("start", start_ms)
            .query("end", end_ms)
            .query("limit", limit.max(1));
        if let Some(search) = search.filter(|value| !value.is_empty()) {
            operation = operation.query("searchText", search);
        }
        if let Some(source) = source.filter(|value| !value.is_empty()) {
            operation = operation.query("source", source);
        }
        operation
    }

    pub fn metrics_search(metric: &str, start_ms: i64, end_ms: i64, source: Option<&str>) -> Self {
        Self::metrics_list(start_ms, end_ms, 10, Some(metric), source)
    }

    pub fn field_keys(signal: &str, query: FieldQuery<'_>) -> Self {
        field_operation("/api/v1/fields/keys", signal, query)
    }

    pub fn field_values(
        signal: &str,
        key: &str,
        existing_query: Option<&str>,
        query: FieldQuery<'_>,
    ) -> Self {
        let mut operation =
            field_operation("/api/v1/fields/values", signal, query).query("name", key);
        if let Some(existing_query) = non_empty(existing_query) {
            operation = operation.query("existingQuery", existing_query);
        }
        operation
    }

    pub fn metric_treemap(start_ms: i64, end_ms: i64, limit: u32) -> Self {
        Self::post(
            "/api/v2/metrics/treemap",
            serde_json::json!({
                "start": start_ms,
                "end": end_ms,
                "limit": limit.max(1),
                "mode": "samples",
                "treemap": "samples",
                "filter": {"expression": ""}
            }),
        )
    }

    pub fn metric_attributes(metric: &str, start_ms: i64, end_ms: i64) -> Self {
        Self::get("/api/v2/metrics/attributes")
            .query("metricName", metric)
            .query("start", start_ms)
            .query("end", end_ms)
    }

    pub fn metric_metadata(metric: &str) -> Self {
        Self::get("/api/v2/metrics/metadata").query("metricName", metric)
    }

    pub fn metric_highlights(metric: &str) -> Self {
        Self::get("/api/v2/metrics/highlights").query("metricName", metric)
    }

    pub fn metric_inspect(metric: &str, start_ms: i64, end_ms: i64, filter: Option<&str>) -> Self {
        let mut body = serde_json::json!({
            "metricName": metric,
            "start": start_ms,
            "end": end_ms,
        });
        if let Some(filter) = filter.filter(|value| !value.trim().is_empty()) {
            body["filter"] = serde_json::json!({"expression": filter});
        }
        Self::post("/api/v2/metrics/inspect", body)
    }

    pub fn metric_dashboards(metric: &str) -> Self {
        Self::get("/api/v2/metrics/dashboards")
            .query("metricName", metric)
            .minimum_version("0.131")
    }

    pub fn metric_alerts(metric: &str) -> Self {
        Self::get("/api/v2/metrics/alerts")
            .query("metricName", metric)
            .minimum_version("0.131")
    }

    pub fn alerts_list() -> Self {
        Self::get("/api/v1/alerts")
    }

    pub fn rules_list() -> Self {
        Self::get("/api/v2/rules").minimum_version("0.120")
    }

    pub fn rule_get(id: &str) -> Self {
        Self::get(format!("/api/v2/rules/{}", segment(id))).minimum_version("0.120")
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rule_history(
        id: &str,
        start_ms: i64,
        end_ms: i64,
        limit: u32,
        order: &str,
        cursor: Option<&str>,
        state: Option<&str>,
        filter: Option<&str>,
    ) -> Self {
        let mut operation = Self::get(format!("/api/v2/rules/{}/history/timeline", segment(id)))
            .query("start", start_ms)
            .query("end", end_ms)
            .query("limit", limit.max(1))
            .query("order", order)
            .minimum_version("0.118");
        if let Some(cursor) = cursor.filter(|value| !value.is_empty()) {
            operation = operation.query("cursor", cursor);
        }
        if let Some(state) = state.filter(|value| !value.is_empty()) {
            operation = operation.query("state", state);
        }
        if let Some(filter) = filter.filter(|value| !value.is_empty()) {
            operation = operation.query("filterExpression", filter);
        }
        operation
    }

    pub fn rule_history_stats(id: &str, start_ms: i64, end_ms: i64) -> Self {
        Self::rule_history_range(id, "stats", start_ms, end_ms)
    }

    pub fn rule_history_overall_status(id: &str, start_ms: i64, end_ms: i64) -> Self {
        Self::rule_history_range(id, "overall_status", start_ms, end_ms)
    }

    pub fn rule_history_top_contributors(id: &str, start_ms: i64, end_ms: i64) -> Self {
        Self::rule_history_range(id, "top_contributors", start_ms, end_ms)
    }

    fn rule_history_range(id: &str, suffix: &str, start_ms: i64, end_ms: i64) -> Self {
        Self::get(format!("/api/v2/rules/{}/history/{suffix}", segment(id)))
            .query("start", start_ms)
            .query("end", end_ms)
            .minimum_version("0.118")
    }

    pub fn dashboards_list(
        query: Option<&str>,
        sort: &str,
        order: &str,
        limit: u32,
        offset: u32,
    ) -> Self {
        let mut operation = Self::get("/api/v2/dashboards")
            .query("sort", sort)
            .query("order", order)
            .query("limit", limit.max(1))
            .query("offset", offset);
        if let Some(query) = query.filter(|value| !value.trim().is_empty()) {
            operation = operation.query("query", query);
        }
        operation
    }

    pub fn dashboard_get(id: &str) -> Self {
        Self::get(format!("/api/v2/dashboards/{}", segment(id)))
    }

    pub fn services_list(start_ms: i64, end_ms: i64) -> Self {
        Self::post(
            "/api/v1/services",
            serde_json::json!({
                "start": nanos(start_ms),
                "end": nanos(end_ms)
            }),
        )
        .minimum_version("with the first-party service compatibility API")
    }

    pub fn service_operations(service: &str, start_ms: i64, end_ms: i64) -> Self {
        Self::post(
            "/api/v1/service/top_operations",
            serde_json::json!({
                "start": nanos(start_ms),
                "end": nanos(end_ms),
                "service": service,
                "tags": []
            }),
        )
        .minimum_version("with the first-party service compatibility API")
    }

    pub fn service_dependencies(start_ms: i64, end_ms: i64) -> Self {
        Self::post(
            "/api/v1/dependency_graph",
            serde_json::json!({
                "start": nanos(start_ms),
                "end": nanos(end_ms),
                "tags": []
            }),
        )
        .minimum_version("with the first-party service-map compatibility API")
    }

    #[allow(clippy::too_many_arguments)]
    pub fn exceptions_list(
        start_ms: i64,
        end_ms: i64,
        order: &str,
        order_by: &str,
        limit: u32,
        offset: u32,
        exception_type: Option<&str>,
        service: Option<&str>,
    ) -> Self {
        let mut body = serde_json::json!({
            "start": nanos(start_ms),
            "end": nanos(end_ms),
            "order": order,
            "orderParam": order_by,
            "limit": limit.max(1),
            "offset": offset,
            "tags": []
        });
        insert_optional_string(&mut body, "exceptionType", exception_type);
        insert_optional_string(&mut body, "serviceName", service);
        Self::post("/api/v1/listErrors", body)
            .minimum_version("with the first-party exceptions compatibility API")
    }

    pub fn exceptions_count(
        start_ms: i64,
        end_ms: i64,
        exception_type: Option<&str>,
        service: Option<&str>,
    ) -> Self {
        let mut body = serde_json::json!({
            "start": nanos(start_ms),
            "end": nanos(end_ms),
            "tags": []
        });
        insert_optional_string(&mut body, "exceptionType", exception_type);
        insert_optional_string(&mut body, "serviceName", service);
        Self::post("/api/v1/countErrors", body)
            .minimum_version("with the first-party exceptions compatibility API")
    }

    pub fn exception_group(group_id: &str, timestamp: &str) -> Self {
        Self::get("/api/v1/errorFromGroupID")
            .query("groupID", group_id)
            .query("timestamp", timestamp)
            .minimum_version("with the first-party exceptions compatibility API")
    }

    pub fn exception_get(group_id: &str, error_id: &str, timestamp: &str) -> Self {
        Self::get("/api/v1/errorFromErrorID")
            .query("groupID", group_id)
            .query("errorID", error_id)
            .query("timestamp", timestamp)
            .minimum_version("with the first-party exceptions compatibility API")
    }

    pub fn exception_navigation(group_id: &str, error_id: &str, timestamp: &str) -> Self {
        Self::get("/api/v1/nextPrevErrorIDs")
            .query("groupID", group_id)
            .query("errorID", error_id)
            .query("timestamp", timestamp)
            .minimum_version("with the first-party exceptions compatibility API")
    }

    pub fn infrastructure_check(resource: &str) -> Self {
        Self::get("/api/v2/infra_monitoring/checks").query("type", resource)
    }

    pub fn infrastructure_list(resource: &str, body: Value) -> Self {
        Self::post(format!("/api/v2/infra_monitoring/{resource}"), body)
    }

    pub fn views_list(source_page: &str) -> Self {
        Self::get("/api/v1/explorer/views").query("sourcePage", source_page)
    }

    pub fn view_get(id: &str) -> Self {
        Self::get(format!("/api/v1/explorer/views/{}", segment(id)))
    }

    pub fn channels_list() -> Self {
        Self::get("/api/v1/channels")
    }

    pub fn channel_get(id: &str) -> Self {
        Self::get(format!("/api/v1/channels/{}", segment(id)))
    }

    pub fn query_raw(body: Value) -> Self {
        Self::post("/api/v5/query_range", body)
    }

    pub fn query_preview(body: Value, explain: bool) -> Self {
        Self::post("/api/v5/query_range/preview", body).query("verbose", explain)
    }
}

pub struct SigNozClient {
    transport: HttpTransport,
}

impl SigNozClient {
    pub fn new(origin: SigNozOrigin, credentials: Credentials) -> Result<Self, ClientError> {
        Ok(Self {
            transport: HttpTransport::new(origin, credentials)?,
        })
    }

    pub fn new_with_timeout(
        origin: SigNozOrigin,
        credentials: Credentials,
        timeout: Duration,
    ) -> Result<Self, ClientError> {
        Ok(Self {
            transport: HttpTransport::new_with_timeout(origin, credentials, timeout)?,
        })
    }

    pub async fn execute(&self, operation: Operation) -> Result<JsonResponse, ClientError> {
        self.transport
            .execute_with_minimum_version(
                operation.method,
                &operation.path,
                &operation.query,
                operation.body,
                operation.replay_safe,
                operation.minimum_version,
            )
            .await
    }

    pub async fn metric_usage(&self, metric: &str) -> Result<JsonResponse, ClientError> {
        let dashboards = self.execute(Operation::metric_dashboards(metric)).await?;
        let alerts = self.execute(Operation::metric_alerts(metric)).await?;
        Ok(JsonResponse {
            value: serde_json::json!({
                "dashboards": dashboards.value,
                "alerts": alerts.value,
            }),
            meta: alerts.meta,
        })
    }
}

fn segment(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn nanos(milliseconds: i64) -> String {
    milliseconds.saturating_mul(1_000_000).to_string()
}

fn insert_optional_string(body: &mut Value, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        body[key] = Value::String(value.to_owned());
    }
}

fn field_operation(path: &str, signal: &str, query: FieldQuery<'_>) -> Operation {
    let mut operation = Operation::get(path)
        .query("signal", signal)
        .query("limit", query.limit.max(1))
        .query("startUnixMilli", query.start_ms)
        .query("endUnixMilli", query.end_ms);
    for (key, value) in [
        ("source", query.source),
        ("fieldContext", query.context),
        ("fieldDataType", query.data_type),
        ("metricName", query.metric_name),
        ("metricNamespace", query.metric_namespace),
        ("searchText", query.search),
    ] {
        if let Some(value) = non_empty(value) {
            operation = operation.query(key, value);
        }
    }
    operation
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::{FieldQuery, Operation};

    #[test]
    fn identifiers_are_encoded_as_single_path_segments() {
        assert_eq!(Operation::rule_get("a/b").path(), "/api/v2/rules/a%2Fb");
    }

    #[test]
    fn public_dashboard_reads_use_v2_and_preserve_list_controls() {
        let list = Operation::dashboards_list(Some("name = 'prod'"), "name", "asc", 25, 50);
        assert_eq!(list.path(), "/api/v2/dashboards");
        assert_eq!(list.query_value("query"), Some("name = 'prod'"));
        assert_eq!(list.query_value("sort"), Some("name"));
        assert_eq!(list.query_value("order"), Some("asc"));
        assert_eq!(list.query_value("limit"), Some("25"));
        assert_eq!(list.query_value("offset"), Some("50"));
        assert_eq!(
            Operation::dashboard_get("a/b").path(),
            "/api/v2/dashboards/a%2Fb"
        );
    }

    #[test]
    fn compatibility_and_public_domain_routes_are_explicit() {
        assert_eq!(
            Operation::service_dependencies(1, 2).path(),
            "/api/v1/dependency_graph"
        );
        assert_eq!(
            Operation::exception_group("group", "123").path(),
            "/api/v1/errorFromGroupID"
        );
        assert_eq!(
            Operation::infrastructure_check("hosts").path(),
            "/api/v2/infra_monitoring/checks"
        );
        assert_eq!(
            Operation::query_preview(serde_json::json!({}), false).path(),
            "/api/v5/query_range/preview"
        );
    }

    #[test]
    fn field_discovery_preserves_public_filters() {
        let query = FieldQuery {
            source: Some("meter"),
            limit: 25,
            start_ms: 1,
            end_ms: 2,
            context: Some("resource"),
            data_type: Some("string"),
            metric_name: Some("requests"),
            metric_namespace: Some("prod"),
            search: Some("service"),
        };
        let values =
            Operation::field_values("metrics", "service.name", Some("env = 'prod'"), query);
        assert_eq!(values.query_value("limit"), Some("25"));
        assert_eq!(values.query_value("source"), Some("meter"));
        assert_eq!(values.query_value("fieldContext"), Some("resource"));
        assert_eq!(values.query_value("fieldDataType"), Some("string"));
        assert_eq!(values.query_value("metricName"), Some("requests"));
        assert_eq!(values.query_value("metricNamespace"), Some("prod"));
        assert_eq!(values.query_value("searchText"), Some("service"));
        assert_eq!(values.query_value("existingQuery"), Some("env = 'prod'"));
    }
}
