use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Validation,
    Config,
    Unauthorized,
    Forbidden,
    NotFound,
    RequestRejected,
    RateLimited,
    Network,
    Timeout,
    Upstream,
    FeatureUnsupported,
    ResponseTooLarge,
    IncompatibleResponse,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct ClientError {
    code: ErrorCode,
    message: String,
    retryable: bool,
    status: Option<u16>,
    hint: Option<String>,
}

impl ClientError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            status: None,
            hint: None,
        }
    }

    pub fn from_status(status: u16, body: &str, minimum_version: Option<&str>) -> Self {
        let code = match status {
            401 => ErrorCode::Unauthorized,
            403 => ErrorCode::Forbidden,
            404 if minimum_version.is_some() => ErrorCode::FeatureUnsupported,
            404 => ErrorCode::NotFound,
            429 => ErrorCode::RateLimited,
            400..=499 => ErrorCode::RequestRejected,
            502..=504 => ErrorCode::Upstream,
            _ => ErrorCode::Upstream,
        };
        let retryable = matches!(status, 429 | 502 | 503 | 504);
        let hint = (status == 404)
            .then_some(minimum_version)
            .flatten()
            .map(|requirement| {
                if requirement.starts_with("with ") {
                    format!("this command requires a SigNoz server {requirement}")
                } else {
                    format!("requires SigNoz {requirement} or newer")
                }
            });
        let message = if body.trim().is_empty() {
            format!("SigNoz returned HTTP {status}")
        } else {
            format!("SigNoz returned HTTP {status}: {}", body.trim())
        };
        Self {
            code,
            message,
            retryable,
            status: Some(status),
            hint,
        }
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn retryable(&self) -> bool {
        self.retryable
    }

    pub fn status(&self) -> Option<u16> {
        self.status
    }

    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    pub(crate) fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{ClientError, ErrorCode};

    #[test]
    fn maps_auth_and_transient_statuses() {
        assert_eq!(
            ClientError::from_status(401, "denied", None).code(),
            ErrorCode::Unauthorized
        );
        assert!(ClientError::from_status(503, "down", None).retryable());
        assert!(!ClientError::from_status(404, "missing", None).retryable());
        assert_eq!(
            ClientError::from_status(404, "missing", Some("with the compatibility API")).hint(),
            Some("this command requires a SigNoz server with the compatibility API")
        );
    }
}
