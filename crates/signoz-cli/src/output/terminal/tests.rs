use super::*;

fn render(command: &str, data: &Value, width: usize) -> (String, bool) {
    let mut output = Vec::new();
    let shortened = write(&mut output, command, data, width, Some("prod")).unwrap();
    (String::from_utf8(output).unwrap(), shortened)
}

#[test]
fn unwraps_catalogs_and_resource_lists_into_aligned_rows() {
    for (command, key) in [
        ("metrics.list", "metrics"),
        ("dashboards.list", "dashboards"),
        ("infrastructure.list", "records"),
    ] {
        let data = json!({"status": "success", "data": {key: [
            {"name": "checkout", "id": "a"}, {"name": "billing", "id": "b"}
        ], "total": 2}});
        let (output, shortened) = render(command, &data, 100);
        assert!(!shortened);
        assert!(output.contains("checkout"));
        assert!(output.contains("billing"));
        assert!(output.contains("total: 2"));
        assert!(!output.contains("\"data\""));
        assert!(!output.contains('{'));
        let lines: Vec<_> = output
            .lines()
            .filter(|line| line.contains("checkout") || line.contains("billing"))
            .collect();
        assert_eq!(lines.len(), 2);
    }
}

#[test]
fn query_rows_keep_timestamps_order_and_fields_from_later_rows() {
    let data = json!({"data": {"data": {"results": [{"rows": [
        {"timestamp": "2026-09-06T01:00Z", "data": {"body": "first", "severity_text": "ERROR"}},
        {"timestamp": "2026-09-06T01:01Z", "data": {"body": "second", "severity_text": "WARN", "extra": "later-field"}}
    ]}]}}});
    let before = data.clone();
    let (output, shortened) = render("logs.search", &data, 140);
    let header = output
        .lines()
        .find(|line| line.contains("severity_text"))
        .unwrap();
    assert!(header.find("timestamp").unwrap() < header.find("severity_text").unwrap());
    assert!(header.find("severity_text").unwrap() < header.find("body").unwrap());
    assert!(header.contains("extra"));
    assert!(output.contains("later-field"));
    assert!(output.contains("2026-09-06T01:00Z"));
    assert!(!output.contains("\"body\""));
    assert!(!shortened);
    assert_eq!(data, before);
}

#[test]
fn multiple_queries_and_time_series_keep_every_section() {
    let data = json!({"data": {"data": {"results": [
        {"queryName": "A", "rows": [{"data": {"body": "first-query"}}]},
        {"queryName": "B", "series": [{
            "labels": {"service.name": "checkout"},
            "values": [{"timestamp": 1000, "value": 11}, {"timestamp": 2000, "value": 22}]
        }]}
    ]}}});
    let (output, shortened) = render("query.run", &data, 100);
    for token in [
        "first-query",
        "queryName",
        "A",
        "B",
        "checkout",
        "1000",
        "2000",
        "11",
        "22",
    ] {
        assert!(output.contains(token), "missing {token}: {output}");
    }
    assert!(!shortened);
    assert!(!output.contains("\"values\""));
}

#[test]
fn unicode_cells_align_by_display_width_and_controls_are_visible() {
    let data = json!([{"name": "支付服务", "value": "one"}, {"name": "api", "value": "two"}]);
    let (output, shortened) = render("services.list", &data, 60);
    let first = output.lines().find(|line| line.contains("one")).unwrap();
    let second = output.lines().find(|line| line.contains("two")).unwrap();
    assert_eq!(
        measure_text_width(&first[..first.find("one").unwrap()]),
        measure_text_width(&second[..second.find("two").unwrap()])
    );
    assert!(!shortened);

    let data = json!([{"body": "first\nsecond\t\u{1b}[31m\r\u{202e}end"}]);
    let (output, shortened) = render("logs.search", &data, 100);
    assert!(!output.contains('\u{1b}'));
    assert!(!output.contains('\r'));
    assert!(!output.contains('\t'));
    assert!(!output.contains('\u{202e}'));
    assert!(output.contains("first\\nsecond\\t\\u{1b}"));
    assert_eq!(output.lines().count(), 3);
    assert!(!shortened);
}

#[test]
fn long_values_shorten_visibly_without_changing_json() {
    let data = json!([{"body": "很长的中文日志".repeat(20)}]);
    let before = data.clone();
    for width in [8, 24, 80] {
        let (output, shortened) = render("logs.search", &data, width);
        assert!(shortened);
        assert!(output.contains('…'));
        assert!(
            output.lines().all(|line| measure_text_width(line) <= width),
            "{output}"
        );
    }
    assert_eq!(data, before);
}

#[test]
fn narrow_displays_use_record_details_without_dropping_columns() {
    let data = json!([
        {"a": "first-long", "b": "second-long", "c": "third-long"},
        {"a": "fourth-long", "d": "later-field"}
    ]);
    let (output, shortened) = render("query.run", &data, 24);
    assert!(output.contains("[1]:"));
    assert!(output.contains("[2]:"));
    for token in ["a:", "b:", "c:", "d:", "first-long", "later-field"] {
        assert!(output.contains(token), "missing {token}: {output}");
    }
    assert!(!shortened);
    assert!(output.lines().all(|line| measure_text_width(line) <= 24));
}

#[test]
fn profiles_and_configuration_have_names_and_current_environment() {
    let profiles = json!({"prod": {"url": "https://prod.example.com"}, "test": {"url": "https://test.example.com"}});
    let (output, shortened) = render("profile.list", &profiles, 100);
    assert!(output.lines().next().unwrap().starts_with("current"));
    assert!(
        output
            .lines()
            .any(|line| line.starts_with('*') && line.contains("prod"))
    );
    assert!(output.contains("https://test.example.com"));
    assert!(!shortened);

    let (output, shortened) = render(
        "config.show",
        &json!({"current_profile": "prod", "profiles": profiles}),
        100,
    );
    assert!(output.contains("current_profile: prod"));
    assert!(output.contains("Profiles:"));
    assert!(output.contains("https://prod.example.com"));
    assert!(!shortened);
}

#[test]
fn empty_mixed_and_unfamiliar_shapes_remain_visible() {
    for data in [
        json!({"data": {"metrics": []}}),
        json!({"data": {"data": {"results": [{"rows": null}]}}}),
    ] {
        let (output, _) = render("query.run", &data, 100);
        assert!(output.contains("(no rows)"));
    }
    let (output, _) = render(
        "query.run",
        &json!([{"name": "record"}, false, null, "text"]),
        100,
    );
    for token in ["record", "false", "null", "text"] {
        assert!(output.contains(token));
    }
    let (output, _) = render(
        "query.run",
        &json!({"status": "failed", "data": {"detail": "reason"}}),
        100,
    );
    assert!(output.contains("failed"));
    assert!(output.contains("reason"));
    let (output, _) = render(
        "query.run",
        &json!({"data": {"detail": "first"}, "other": "second"}),
        100,
    );
    assert!(output.contains("first"));
    assert!(output.contains("second"));
    let collision = json!({"timestamp": "outer", "data": {"timestamp": "inner"}});
    assert_eq!(expand_query_row(&collision).as_ref(), &collision);
}
