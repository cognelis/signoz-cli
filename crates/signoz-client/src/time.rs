use chrono::{DateTime, Utc};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeWindowArgs {
    since: Option<String>,
    start: Option<String>,
    end: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimeWindow {
    pub start_ms: i64,
    pub end_ms: i64,
}

impl TimeWindowArgs {
    pub fn since(value: impl Into<String>) -> Self {
        Self {
            since: Some(value.into()),
            start: None,
            end: None,
        }
    }

    pub fn absolute(start: Option<&str>, end: Option<&str>) -> Self {
        Self {
            since: None,
            start: start.map(str::to_owned),
            end: end.map(str::to_owned),
        }
    }

    pub fn from_parts(since: Option<&str>, start: Option<&str>, end: Option<&str>) -> Self {
        Self {
            since: since.map(str::to_owned),
            start: start.map(str::to_owned),
            end: end.map(str::to_owned),
        }
    }

    pub fn resolve(&self, now: DateTime<Utc>) -> Result<TimeWindow, String> {
        if let Some(since) = &self.since {
            if self.start.is_some() {
                return Err("--since conflicts with --start".into());
            }
            let duration = parse_duration_ms(since)?;
            let end_ms = match self.end.as_deref() {
                Some(end) => parse_timestamp_ms(end)?,
                None => now.timestamp_millis(),
            };
            let start_ms = end_ms
                .checked_sub(duration)
                .ok_or_else(|| "relative time range is out of bounds".to_owned())?;
            return Ok(TimeWindow { start_ms, end_ms });
        }
        match (&self.start, &self.end) {
            (Some(start), Some(end)) => {
                let start_ms = parse_timestamp_ms(start)?;
                let end_ms = parse_timestamp_ms(end)?;
                if start_ms >= end_ms {
                    return Err("start time must be before end time".into());
                }
                Ok(TimeWindow { start_ms, end_ms })
            }
            (None, None) => Err("provide --since or both --start and --end".into()),
            _ => Err("--start and --end must be provided together".into()),
        }
    }
}

fn parse_duration_ms(value: &str) -> Result<i64, String> {
    let split = value
        .find(|character: char| !character.is_ascii_digit())
        .ok_or_else(|| "duration requires one of ms, s, m, h, or d".to_owned())?;
    let amount: i64 = value[..split]
        .parse()
        .map_err(|_| "duration must start with a positive integer".to_owned())?;
    if amount <= 0 {
        return Err("duration must be positive".into());
    }
    let multiplier = match &value[split..] {
        "ms" => 1,
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return Err("duration requires one of ms, s, m, h, or d".into()),
    };
    amount
        .checked_mul(multiplier)
        .ok_or_else(|| "duration is too large".to_owned())
}

fn parse_timestamp_ms(value: &str) -> Result<i64, String> {
    if let Some(raw) = value.strip_prefix('@') {
        return raw
            .parse()
            .map_err(|_| "Unix timestamp must be @ followed by milliseconds".into());
    }
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.timestamp_millis())
        .map_err(|_| "timestamp must be RFC3339 or @<unix-ms>".into())
}

#[cfg(test)]
mod tests {
    use super::TimeWindowArgs;
    use chrono::DateTime;

    #[test]
    fn anchors_lookback_to_an_explicit_end_instead_of_now() {
        let args = TimeWindowArgs::from_parts(Some("30m"), None, Some("2026-11-12T09:00:00+08:00"));
        let first = args
            .resolve(DateTime::from_timestamp_millis(0).unwrap())
            .unwrap();
        let later = args
            .resolve(DateTime::from_timestamp_millis(1_000_000).unwrap())
            .unwrap();
        assert_eq!(first, later);
        assert_eq!(
            first.end_ms,
            DateTime::parse_from_rfc3339("2026-11-12T01:00:00Z")
                .unwrap()
                .timestamp_millis()
        );
        assert_eq!(first.end_ms - first.start_ms, 1_800_000);
        let now = DateTime::from_timestamp_millis(0).unwrap();
        assert!(
            TimeWindowArgs::from_parts(Some("1ms"), None, Some("@-9223372036854775808"))
                .resolve(now)
                .is_err()
        );
        assert!(
            TimeWindowArgs::from_parts(Some("1m"), Some("@1"), Some("@2"))
                .resolve(now)
                .is_err()
        );
        assert!(
            TimeWindowArgs::from_parts(Some("1m"), None, Some("bad"))
                .resolve(now)
                .is_err()
        );
    }

    #[test]
    fn resolves_relative_and_absolute_windows() {
        let now = DateTime::parse_from_rfc3339("2026-07-22T01:00:00Z")
            .unwrap()
            .to_utc();
        assert_eq!(
            TimeWindowArgs::since("1h").resolve(now).unwrap().start_ms,
            now.timestamp_millis() - 3_600_000
        );
        assert!(
            TimeWindowArgs::absolute(Some("@1000"), None)
                .resolve(now)
                .is_err()
        );
    }

    #[test]
    fn rejects_invalid_windows_and_accepts_unix_millis() {
        let now = DateTime::parse_from_rfc3339("2026-07-22T01:00:00Z")
            .unwrap()
            .to_utc();
        assert!(TimeWindowArgs::since("0h").resolve(now).is_err());
        assert!(TimeWindowArgs::since("1w").resolve(now).is_err());
        let window = TimeWindowArgs::absolute(Some("@1000"), Some("@2000"))
            .resolve(now)
            .unwrap();
        assert_eq!((window.start_ms, window.end_ms), (1000, 2000));
    }

    #[test]
    fn relative_windows_reject_underflow_and_preserve_representable_boundaries() {
        let now = DateTime::from_timestamp_millis(-2).unwrap();
        assert!(
            TimeWindowArgs::since(format!("{}ms", i64::MAX))
                .resolve(now)
                .is_err()
        );

        let now = DateTime::from_timestamp_millis(-1).unwrap();
        let window = TimeWindowArgs::since(format!("{}ms", i64::MAX))
            .resolve(now)
            .unwrap();
        assert_eq!(window.start_ms, i64::MIN);
        assert_eq!(window.end_ms, -1);
    }
}
