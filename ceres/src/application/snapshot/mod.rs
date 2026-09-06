//! Immutable source resolution, independent of HTTP and transport implementations.
//!
//! Resolving one source is not a published namespace snapshot: registry history,
//! publication and retention are separate capabilities.

pub(crate) mod source;
