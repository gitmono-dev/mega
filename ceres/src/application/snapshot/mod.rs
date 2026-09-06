//! Immutable source resolution, independent of HTTP and transport implementations.
//!
//! Resolving one source is not a published namespace snapshot: registry history,
//! publication and retention are separate capabilities.

pub mod catalog;
pub mod object;
pub(crate) mod source;
