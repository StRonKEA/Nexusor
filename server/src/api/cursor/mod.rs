//! Wires the Cursor-facing API routes.

pub mod bidi;
pub mod handlers;
mod local_controls;
pub mod proxy;
mod run_sse;

pub use handlers::{router, CURSOR_MAX_BODY_BYTES};
