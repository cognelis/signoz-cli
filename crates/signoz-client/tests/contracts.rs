use reqwest::Method;
use signoz_client::{Credentials, Operation, SigNozClient, SigNozOrigin};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn every_read_operation_uses_its_official_route() {
    let cases = [
        (Operation::auth_me(), Method::GET, "/api/v1/service_accounts/me"),
        (Operation::metrics_list(), Method::GET, "/api/v2/metrics"),
        (Operation::field_keys("logs"), Method::GET, "/api/v1/fields/keys"),
        (Operation::alerts_list(), Method::GET, "/api/v1/alerts"),
        (Operation::rules_list(), Method::GET, "/api/v2/rules"),
        (Operation::dashboards_list(), Method::GET, "/api/v1/dashboards"),
        (Operation::services_list(1, 2), Method::GET, "/api/v1/services"),
        (Operation::views_list(), Method::GET, "/api/v1/explorer/views"),
        (Operation::channels_list(), Method::GET, "/api/v1/channels"),
        (Operation::query_raw(serde_json::json!({"start": 1, "end": 2})), Method::POST, "/api/v5/query_range"),
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
