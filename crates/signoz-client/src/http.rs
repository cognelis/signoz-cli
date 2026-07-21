use std::time::Duration;

use reqwest::Method;
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use serde_json::Value;

use crate::{ClientError, ErrorCode, SigNozOrigin};

const DEFAULT_MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

pub struct Credentials(SecretString);

impl Credentials {
    pub fn new(value: impl Into<String>) -> Self {
        Self(SecretString::from(value.into()))
    }

    fn expose(&self) -> &str {
        self.0.expose_secret()
    }

    fn redact(&self, value: &str) -> String {
        let secret = self.expose();
        if secret.is_empty() {
            value.to_owned()
        } else {
            value.replace(secret, "[REDACTED]")
        }
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Credentials([REDACTED])")
    }
}

#[derive(Clone, Debug)]
pub struct ResponseMeta {
    pub status: u16,
    pub request_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct JsonResponse {
    pub value: Value,
    pub meta: ResponseMeta,
}

pub struct HttpTransport {
    origin: SigNozOrigin,
    credentials: Credentials,
    client: reqwest::Client,
    timeout: Duration,
    max_response_bytes: usize,
}

impl HttpTransport {
    pub fn new(origin: SigNozOrigin, credentials: Credentials) -> Result<Self, ClientError> {
        Self::new_with_timeout(origin, credentials, Duration::from_secs(60))
    }

    pub fn new_with_timeout(
        origin: SigNozOrigin,
        credentials: Credentials,
        timeout: Duration,
    ) -> Result<Self, ClientError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("signoz-cli/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| ClientError::new(ErrorCode::Config, error.to_string()))?;
        Ok(Self {
            origin,
            credentials,
            client,
            timeout,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        })
    }

    pub fn with_max_response_bytes(mut self, value: usize) -> Self {
        self.max_response_bytes = value;
        self
    }

    pub async fn get(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<JsonResponse, ClientError> {
        self.execute(Method::GET, path, query, None, true, None)
            .await
    }

    pub async fn post_json<T: Serialize + ?Sized>(
        &self,
        path: &str,
        body: &T,
        replay_safe: bool,
    ) -> Result<JsonResponse, ClientError> {
        let value = serde_json::to_value(body)
            .map_err(|error| ClientError::new(ErrorCode::Validation, error.to_string()))?;
        self.execute(Method::POST, path, &[], Some(value), replay_safe, None)
            .await
    }

    pub async fn execute_with_minimum_version(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<Value>,
        replay_safe: bool,
        minimum_version: Option<&str>,
    ) -> Result<JsonResponse, ClientError> {
        self.execute(method, path, query, body, replay_safe, minimum_version)
            .await
    }

    async fn execute(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<Value>,
        replay_safe: bool,
        minimum_version: Option<&str>,
    ) -> Result<JsonResponse, ClientError> {
        match tokio::time::timeout(
            self.timeout,
            self.execute_attempts(method, path, query, body, replay_safe, minimum_version),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(ClientError::new(
                ErrorCode::Timeout,
                format!(
                    "command exceeded the {} second timeout",
                    self.timeout.as_secs_f64()
                ),
            )),
        }
    }

    async fn execute_attempts(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<Value>,
        replay_safe: bool,
        minimum_version: Option<&str>,
    ) -> Result<JsonResponse, ClientError> {
        let url = format!("{}{}", self.origin.as_str(), path);
        let attempts = if replay_safe { 3 } else { 1 };
        for attempt in 0..attempts {
            let mut request = self
                .client
                .request(method.clone(), &url)
                .query(query)
                .header("SIGNOZ-API-KEY", self.credentials.expose());
            if let Some(body) = &body {
                request = request.json(body);
            }
            match request.send().await {
                Ok(mut response) => {
                    let status = response.status();
                    let request_id = response
                        .headers()
                        .get("x-request-id")
                        .and_then(|value| value.to_str().ok())
                        .map(|value| self.credentials.redact(value));
                    let retry_after = retry_after(response.headers());
                    let bytes = read_bounded(&mut response, self.max_response_bytes)
                        .await
                        .map_err(|error| {
                            if error.code() == ErrorCode::ResponseTooLarge {
                                error.with_status(status.as_u16())
                            } else {
                                error
                            }
                        })?;
                    let text = String::from_utf8_lossy(&bytes);
                    if !status.is_success() {
                        let safe_text = self.credentials.redact(&text);
                        let safe_text = truncate_error_body(&safe_text, 4_096);
                        let error =
                            ClientError::from_status(status.as_u16(), &safe_text, minimum_version);
                        if error.retryable() && attempt + 1 < attempts {
                            tokio::time::sleep(retry_after.unwrap_or_else(|| retry_delay(attempt)))
                                .await;
                            continue;
                        }
                        return Err(error);
                    }
                    let value = serde_json::from_slice(&bytes).map_err(|error| {
                        ClientError::new(
                            ErrorCode::IncompatibleResponse,
                            format!("SigNoz returned invalid JSON: {error}"),
                        )
                        .with_status(status.as_u16())
                    })?;
                    return Ok(JsonResponse {
                        value,
                        meta: ResponseMeta {
                            status: status.as_u16(),
                            request_id,
                        },
                    });
                }
                Err(error) => {
                    let is_timeout = error.is_timeout();
                    if attempt + 1 < attempts {
                        tokio::time::sleep(retry_delay(attempt)).await;
                        continue;
                    }
                    return Err(ClientError::new(
                        if is_timeout {
                            ErrorCode::Timeout
                        } else {
                            ErrorCode::Network
                        },
                        error.to_string(),
                    ));
                }
            }
        }
        unreachable!("retry loop always returns")
    }
}

async fn read_bounded(
    response: &mut reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, ClientError> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(ClientError::new(
            ErrorCode::ResponseTooLarge,
            format!("response exceeds {limit} bytes"),
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        ClientError::new(
            ErrorCode::Network,
            format!("failed to read response: {error}"),
        )
    })? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(ClientError::new(
                ErrorCode::ResponseTooLarge,
                format!("response exceeds {limit} bytes"),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn retry_delay(attempt: usize) -> Duration {
    Duration::from_millis(if attempt == 0 { 100 } else { 400 })
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let seconds: u64 = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .parse()
        .ok()?;
    Some(Duration::from_secs(seconds.min(5)))
}

fn truncate_error_body(value: &str, limit: usize) -> String {
    let mut characters = value.chars();
    let prefix: String = characters.by_ref().take(limit).collect();
    if characters.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

#[cfg(test)]
mod tests {
    use super::Credentials;

    #[test]
    fn credentials_are_redacted_from_debug() {
        let credentials = Credentials::new("never-print-this");
        assert!(!format!("{credentials:?}").contains("never-print-this"));
    }
}
