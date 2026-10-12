use serde::Serialize;
use serde_json::Value;

const CONFIGURATION: usize = 1;
const CREDENTIALS: usize = 2;
const API: usize = 3;
const AUTHENTICATION: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Pass,
    Fail,
    Skipped,
}

#[derive(Clone, Debug, Serialize)]
pub struct DoctorCheck {
    name: &'static str,
    status: DoctorStatus,
    message: String,
}

#[derive(Debug)]
pub struct DoctorReport {
    checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn new(version: &str) -> Self {
        Self {
            checks: vec![
                DoctorCheck {
                    name: "version",
                    status: DoctorStatus::Pass,
                    message: format!("signoz-cli {version}"),
                },
                skipped("configuration", "connection target has not been checked"),
                skipped("credentials", "credentials have not been checked"),
                skipped("api", "API connectivity has not been checked"),
                skipped("authentication", "authentication has not been checked"),
            ],
        }
    }

    pub fn configuration_passed(&mut self) {
        self.set(
            CONFIGURATION,
            DoctorStatus::Pass,
            "connection target is configured and valid",
        );
    }

    pub fn configuration_failed(&mut self) {
        self.set(
            CONFIGURATION,
            DoctorStatus::Fail,
            "connection configuration is missing, unreadable, or invalid",
        );
        self.set(
            CREDENTIALS,
            DoctorStatus::Skipped,
            "not checked because configuration failed",
        );
        self.set(
            API,
            DoctorStatus::Skipped,
            "not checked because configuration failed",
        );
        self.set(
            AUTHENTICATION,
            DoctorStatus::Skipped,
            "not checked because configuration failed",
        );
    }

    /// `source` names where the API key came from, such as the credential store.
    pub fn credentials_passed(&mut self, source: &str) {
        self.set(
            CREDENTIALS,
            DoctorStatus::Pass,
            format!("an API key is available from {source}"),
        );
    }

    pub fn credentials_failed(&mut self) {
        self.set(
            CREDENTIALS,
            DoctorStatus::Fail,
            "an API key is missing or unavailable",
        );
        self.set(
            API,
            DoctorStatus::Skipped,
            "not checked because credentials failed",
        );
        self.set(
            AUTHENTICATION,
            DoctorStatus::Skipped,
            "not checked because credentials failed",
        );
    }

    pub fn api_passed(&mut self) {
        self.set(API, DoctorStatus::Pass, "SigNoz API is reachable");
    }

    pub fn api_failed(&mut self, message: &'static str) {
        self.set(API, DoctorStatus::Fail, message);
        self.set(
            AUTHENTICATION,
            DoctorStatus::Skipped,
            "not checked because the API probe failed",
        );
    }

    pub fn authentication_passed(&mut self) {
        self.set(
            AUTHENTICATION,
            DoctorStatus::Pass,
            "service account authentication succeeded",
        );
    }

    pub fn authentication_failed(&mut self, message: &'static str) {
        self.set(AUTHENTICATION, DoctorStatus::Fail, message);
    }

    pub fn into_value(self) -> Value {
        serde_json::to_value(self.checks).expect("doctor checks always serialize")
    }

    fn set(&mut self, index: usize, status: DoctorStatus, message: impl Into<String>) {
        self.checks[index].status = status;
        self.checks[index].message = message.into();
    }
}

fn skipped(name: &'static str, message: &'static str) -> DoctorCheck {
    DoctorCheck {
        name,
        status: DoctorStatus::Skipped,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_shape_is_ordered_and_stable() {
        let report = DoctorReport::new("1.2.3").into_value();
        let checks = report.as_array().unwrap();
        assert_eq!(checks.len(), 5);
        assert_eq!(checks[0]["name"], "version");
        assert_eq!(checks[CONFIGURATION]["name"], "configuration");
        assert_eq!(checks[CREDENTIALS]["name"], "credentials");
        assert_eq!(checks[API]["name"], "api");
        assert_eq!(checks[AUTHENTICATION]["name"], "authentication");
    }
}
