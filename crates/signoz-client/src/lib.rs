//! Typed, bounded client primitives for SigNoz's read APIs.

pub mod error;
pub mod http;
pub mod time;
pub mod url;

pub use time::{TimeWindow, TimeWindowArgs};
pub use url::SigNozOrigin;
pub use error::{ClientError, ErrorCode};
pub use http::{Credentials, HttpTransport, JsonResponse, ResponseMeta};
