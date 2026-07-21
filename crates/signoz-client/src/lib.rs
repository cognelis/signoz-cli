//! Typed, bounded client primitives for SigNoz's read APIs.

pub mod api;
pub mod error;
pub mod http;
pub mod query;
pub mod time;
pub mod url;

pub use time::{TimeWindow, TimeWindowArgs};
pub use url::SigNozOrigin;
pub use error::{ClientError, ErrorCode};
pub use http::{Credentials, HttpTransport, JsonResponse, ResponseMeta};
pub use api::{Operation, SigNozClient};
pub use query::{
    log_aggregate, log_search, metric_query, trace_aggregate, trace_get, trace_search,
};
