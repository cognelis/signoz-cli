use reqwest::Method;
use signoz_client::{Credentials, Operation, SigNozClient, SigNozOrigin};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn every_read_operation_uses_its_official_route() {
    let cases = [
        (
            Operation::auth_me(),
            Method::GET,
            "/api/v1/service_accounts/me",
        ),
        (
            Operation::metrics_list(1, 2, 100, None, None),
            Method::GET,
            "/api/v2/metrics",
        ),
        (
            Operation::metrics_search("requests", 1, 2),
            Method::GET,
            "/api/v2/metrics",
        ),
        (
            Operation::field_keys("logs"),
            Method::GET,
            "/api/v1/fields/keys",
        ),
        (
            Operation::field_values("logs", "service.name"),
            Method::GET,
            "/api/v1/fields/values",
        ),
        (
            Operation::metric_attributes("requests", 1, 2),
            Method::GET,
            "/api/v2/metrics/attributes",
        ),
        (
            Operation::metric_dashboards("requests"),
            Method::GET,
            "/api/v2/metrics/dashboards",
        ),
        (
            Operation::metric_alerts("requests"),
            Method::GET,
            "/api/v2/metrics/alerts",
        ),
        (Operation::alerts_list(), Method::GET, "/api/v1/alerts"),
        (Operation::rules_list(), Method::GET, "/api/v2/rules"),
        (
            Operation::rule_get("rule"),
            Method::GET,
            "/api/v2/rules/rule",
        ),
        (
            Operation::rule_history("rule", 1, 2, 20, "asc", None, None, None),
            Method::GET,
            "/api/v2/rules/rule/history/timeline",
        ),
        (
            Operation::dashboards_list(),
            Method::GET,
            "/api/v1/dashboards",
        ),
        (
            Operation::dashboard_get("dash"),
            Method::GET,
            "/api/v1/dashboards/dash",
        ),
        (
            Operation::services_list(1, 2),
            Method::POST,
            "/api/v1/services",
        ),
        (
            Operation::service_operations("checkout", 1, 2),
            Method::POST,
            "/api/v1/service/top_operations",
        ),
        (
            Operation::views_list("logs"),
            Method::GET,
            "/api/v1/explorer/views",
        ),
        (
            Operation::view_get("view"),
            Method::GET,
            "/api/v1/explorer/views/view",
        ),
        (Operation::channels_list(), Method::GET, "/api/v1/channels"),
        (
            Operation::channel_get("channel"),
            Method::GET,
            "/api/v1/channels/channel",
        ),
        (
            Operation::query_raw(serde_json::json!({"start": 1, "end": 2})),
            Method::POST,
            "/api/v5/query_range",
        ),
    ];
    for (operation, expected_method, expected_path) in cases {
        let server = MockServer::start().await;
        Mock::given(method(expected_method.as_str()))
            .and(path(expected_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": []})))
            .expect(1)
            .mount(&server)
            .await;
        let client = SigNozClient::new(
            SigNozOrigin::parse(&server.uri()).unwrap(),
            Credentials::new("secret"),
        )
        .unwrap();
        client.execute(operation).await.unwrap();
    }
}

#[tokio::test]
async fn post_routes_match_the_official_service_and_metric_contracts() {
    use wiremock::matchers::body_partial_json;

    let cases = [
        (
            Operation::services_list(1, 2),
            "/api/v1/services",
            serde_json::json!({"start": "1000000", "end": "2000000"}),
        ),
        (
            Operation::service_operations("checkout", 1, 2),
            "/api/v1/service/top_operations",
            serde_json::json!({
                "start": "1000000",
                "end": "2000000",
                "service": "checkout",
                "tags": []
            }),
        ),
        (
            Operation::metric_treemap(1, 2, 25),
            "/api/v2/metrics/treemap",
            serde_json::json!({"start": 1, "end": 2, "limit": 25, "mode": "samples"}),
        ),
    ];
    for (operation, expected_path, body) in cases {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(expected_path))
            .and(body_partial_json(body))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": []})))
            .mount(&server)
            .await;
        let client = SigNozClient::new(
            SigNozOrigin::parse(&server.uri()).unwrap(),
            Credentials::new("secret"),
        )
        .unwrap();
        client.execute(operation).await.unwrap();
    }
}

#[tokio::test]
async fn metric_usage_combines_dashboard_and_alert_references() {
    use wiremock::matchers::query_param;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v2/metrics/dashboards"))
        .and(query_param("metricName", "http.requests"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"dashboardName": "API"}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/metrics/alerts"))
        .and(query_param("metricName", "http.requests"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"alertName": "High traffic"}]
        })))
        .mount(&server)
        .await;
    let client = SigNozClient::new(
        SigNozOrigin::parse(&server.uri()).unwrap(),
        Credentials::new("secret"),
    )
    .unwrap();
    let response = client.metric_usage("http.requests").await.unwrap();
    assert_eq!(
        response.value["dashboards"]["data"][0]["dashboardName"],
        "API"
    );
    assert_eq!(
        response.value["alerts"]["data"][0]["alertName"],
        "High traffic"
    );
}

#[test]
fn cardinality_and_field_values_use_the_official_query_names() {
    let cardinality = Operation::metric_attributes("run.googleapis.com/request", 1, 2);
    assert_eq!(
        cardinality.query_value("metricName"),
        Some("run.googleapis.com/request")
    );
    assert_eq!(cardinality.query_value("start"), Some("1"));
    let values = Operation::field_values("logs", "service.name");
    assert_eq!(values.query_value("name"), Some("service.name"));
    assert_eq!(values.query_value("key"), None);
    let views = Operation::views_list("traces");
    assert_eq!(views.query_value("sourcePage"), Some("traces"));
    let metrics = Operation::metrics_search("http.requests", 10, 20);
    assert_eq!(metrics.query_value("searchText"), Some("http.requests"));
    assert_eq!(metrics.query_value("start"), Some("10"));
    assert_eq!(metrics.query_value("end"), Some("20"));
    let catalog = Operation::metrics_list(10, 20, 50, Some("http"), Some("meter"));
    assert_eq!(catalog.query_value("start"), Some("10"));
    assert_eq!(catalog.query_value("end"), Some("20"));
    assert_eq!(catalog.query_value("limit"), Some("50"));
    assert_eq!(catalog.query_value("searchText"), Some("http"));
    assert_eq!(catalog.query_value("source"), Some("meter"));
    let history = Operation::rule_history(
        "rule",
        10,
        20,
        50,
        "desc",
        Some("opaque-cursor"),
        Some("firing"),
        Some("severity = 'critical'"),
    );
    assert_eq!(history.query_value("limit"), Some("50"));
    assert_eq!(history.query_value("order"), Some("desc"));
    assert_eq!(history.query_value("cursor"), Some("opaque-cursor"));
    assert_eq!(history.query_value("state"), Some("firing"));
    assert_eq!(
        history.query_value("filterExpression"),
        Some("severity = 'critical'")
    );
}
