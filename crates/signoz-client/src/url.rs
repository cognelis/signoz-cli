use std::fmt;

use url::Url;

/// A validated SigNoz HTTP(S) base URL with no credentials, query, or fragment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SigNozOrigin(String);

impl SigNozOrigin {
    pub fn parse(value: &str) -> Result<Self, String> {
        let parsed = Url::parse(value).map_err(|error| format!("invalid SigNoz URL: {error}"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("SigNoz URL must use http or https".into());
        }
        if parsed.host_str().is_none() {
            return Err("SigNoz URL must include a host".into());
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err("SigNoz URL must not contain user information".into());
        }
        if parsed.query().is_some() || parsed.fragment().is_some() {
            return Err("SigNoz URL must not contain a query or fragment".into());
        }
        Ok(Self(parsed.as_str().trim_end_matches('/').to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SigNozOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::SigNozOrigin;

    #[test]
    fn accepts_http_origins_and_removes_trailing_slash() {
        assert_eq!(
            SigNozOrigin::parse("http://localhost:3301/")
                .unwrap()
                .as_str(),
            "http://localhost:3301"
        );
    }

    #[test]
    fn preserves_supported_external_base_paths() {
        assert_eq!(
            SigNozOrigin::parse("https://example.com/observability/signoz/")
                .unwrap()
                .as_str(),
            "https://example.com/observability/signoz"
        );
    }

    #[test]
    fn rejects_userinfo_queries_fragments_and_non_http_schemes() {
        for value in [
            "https://user@example.com",
            "https://example.com?q=1",
            "https://example.com#fragment",
            "file:///tmp/signoz",
        ] {
            assert!(SigNozOrigin::parse(value).is_err(), "accepted {value}");
        }
    }
}
