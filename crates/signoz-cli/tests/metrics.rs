use assert_cmd::Command;
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn inspect_defaults_to_thirty_minutes_and_preserves_explicit_windows() {
    let directory = tempfile::tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v2/metrics/inspect"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
        .expect(4)
        .mount(&server)
        .await;

    for (args, duration) in [
        (vec![], 1_800_000),
        (vec!["--since", "15m"], 900_000),
        (vec!["--since", "30m"], 1_800_000),
        (vec!["--start", "@1000", "--end", "@1801000"], 1_800_000),
    ] {
        let output = Command::cargo_bin("signoz-cli")
            .unwrap()
            .args([
                "--url",
                &server.uri(),
                "--output",
                "json",
                "metrics",
                "inspect",
                "requests",
            ])
            .args(&args)
            .env("SIGNOZ_CONFIG", directory.path().join("config.toml"))
            .env("SIGNOZ_API_KEY", "test-key")
            .assert()
            .success()
            .get_output()
            .clone();
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
        let requests = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&requests.last().unwrap().body).unwrap();
        let start = body["start"].as_i64().unwrap();
        let end = body["end"].as_i64().unwrap();
        assert_eq!(end - start, duration);
        assert_eq!(body["metricName"], "requests");
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(envelope["meta"]["start"].as_str().unwrap())
                .unwrap()
                .timestamp_millis(),
            start
        );
        assert_eq!(
            chrono::DateTime::parse_from_rfc3339(envelope["meta"]["end"].as_str().unwrap())
                .unwrap()
                .timestamp_millis(),
            end
        );
        if args.first() == Some(&"--start") {
            assert_eq!(start, 1000);
        }
    }

    for args in [
        vec!["--since", "31m"],
        vec!["--start", "@1000", "--end", "@1801001"],
    ] {
        Command::cargo_bin("signoz-cli")
            .unwrap()
            .args([
                "--url",
                &server.uri(),
                "--output",
                "json",
                "metrics",
                "inspect",
                "requests",
            ])
            .args(args)
            .env("SIGNOZ_CONFIG", directory.path().join("config.toml"))
            .env("SIGNOZ_API_KEY", "test-key")
            .assert()
            .code(2)
            .stdout("")
            .stderr(predicates::str::contains("maximum 30-minute time range"));
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[tokio::test]
async fn metric_catalog_and_query_use_the_same_source() {
    for source in [None, Some("meter")] {
        let directory = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        let mut catalog = Mock::given(method("GET"))
            .and(path("/api/v2/metrics"))
            .and(query_param("searchText", "requests"));
        if let Some(source) = source {
            catalog = catalog.and(query_param("source", source));
        }
        catalog
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"metrics": [{
                    "metricName": "requests", "type": "sum",
                    "temporality": "cumulative", "isMonotonic": true
                }]}
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v5/query_range"))
            .and(body_partial_json(json!({
                "compositeQuery": {"queries": [{"spec": {"aggregations": [{
                    "metricName": "requests", "temporality": "cumulative",
                    "timeAggregation": "rate", "spaceAggregation": "sum"
                }]}}]}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"data": {"results": [{"rows": []}]}}
            })))
            .expect(1)
            .mount(&server)
            .await;
        let mut command = Command::cargo_bin("signoz-cli").unwrap();
        command
            .args([
                "--url",
                &server.uri(),
                "--output",
                "json",
                "metrics",
                "query",
                "requests",
            ])
            .env("SIGNOZ_CONFIG", directory.path().join("config.toml"))
            .env("SIGNOZ_API_KEY", "test-key");
        if let Some(source) = source {
            command.args(["--source", source]);
        }
        command.assert().success().stderr("");

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        let catalog_source = requests[0]
            .url
            .query_pairs()
            .find(|(key, _)| key == "source")
            .map(|(_, value)| value.into_owned());
        let body: Value = serde_json::from_slice(&requests[1].body).unwrap();
        assert_eq!(catalog_source.as_deref(), source);
        assert_eq!(
            body.pointer("/compositeQuery/queries/0/spec/source")
                .and_then(Value::as_str),
            source
        );
    }
}
