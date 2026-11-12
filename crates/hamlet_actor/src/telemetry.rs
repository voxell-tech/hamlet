// This file was changed for Hamlet. See NOTICE in this crate.
//! OTLP export of actor spans.
//!
//! Add [`layer`] to the application's `tracing` subscriber. It
//! exports only when `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` is set, and
//! returns `None` otherwise.
//!
//! ```ignore
//! use tracing_subscriber::prelude::*;
//!
//! tracing_subscriber::registry()
//!     .with(hamlet_actor::telemetry::layer()?)
//!     .init();
//! ```
//!
//! Call [`shutdown`] to flush pending spans before the host exits.

pub use hamlet_actor_core::telemetry::export::{
    layer, shutdown_best_effort as shutdown,
};
