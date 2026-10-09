//! Converts between canonical Messages and Cursor checkpoint message data.

mod decode;
mod encode;

pub use decode::{decode, decode_pending};
pub use encode::{stable_messages, staged_final, staged_tool_round};

pub const REPLAY_ENVELOPE_PREFIX: &str = "nexusor:v1:";
pub const LEGACY_REPLAY_ENVELOPE_PREFIX: &str = "cursor-byok:v1:";

#[cfg(test)]
mod tests;
