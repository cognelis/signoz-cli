//! Typed, bounded client primitives for SigNoz's read APIs.

pub mod time;
pub mod url;

pub use time::{TimeWindow, TimeWindowArgs};
pub use url::SigNozOrigin;
