use reqwest::Method;
use serde_json::Value;

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

    pub fn auth_me() -> Self {
        Self::get("/api/v1/service_accounts/me")
    }

    pub fn metrics_list() -> Self {
        Self::get("/api/v2/metrics")
    }

    pub fn field_keys(signal: &str) -> Self {
        Self::get("/api/v1/fields/keys").query("signal", signal)
    }

    pub fn field_values(signal: &str, key: &str) -> Self {
        Self::get("/api/v1/fields/values")
            .query("signal", signal)
            .query("key", key)
    }

    pub fn metric_treemap(metric: &str, start_ms: i64, end_ms: i64) -> Self {
        Self::get("/api/v2/metrics/treemap")
            .query("metricName", metric)
            .query("start", start_ms)
            .query("end", end_ms)
    }

    pub fn metric_attributes(metric: &str) -> Self {
        Self::get("/api/v2/metrics/attributes").query("metricName", metric)
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

    pub fn rule_history(id: &str, start_ms: i64, end_ms: i64) -> Self {
        Self::get(format!(
            "/api/v2/rules/{}/history/timeline",
            segment(id)
        ))
        .query("start", start_ms)
        .query("end", end_ms)
        .minimum_version("0.118")
    }

    pub fn dashboards_list() -> Self {
        Self::get("/api/v1/dashboards")
    }

    pub fn dashboard_get(id: &str) -> Self {
        Self::get(format!("/api/v1/dashboards/{}", segment(id)))
    }

    pub fn services_list(start_ms: i64, end_ms: i64) -> Self {
        Self::get("/api/v1/services")
            .query("start", start_ms)
            .query("end", end_ms)
    }

    pub fn service_operations(service: &str, start_ms: i64, end_ms: i64) -> Self {
        Self::get(format!(
            "/api/v1/services/{}/operations",
            segment(service)
        ))
        .query("start", start_ms)
        .query("end", end_ms)
    }

    pub fn views_list() -> Self {
        Self::get("/api/v1/explorer/views")
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

#[cfg(test)]
mod tests {
    use super::Operation;

    #[test]
    fn identifiers_are_encoded_as_single_path_segments() {
        assert_eq!(Operation::rule_get("a/b").path(), "/api/v2/rules/a%2Fb");
    }
}
