//! UI hooks inside Phoenix (Phase 2, stage 2.3): the pure parts, tested on plain data.
//!
//! `context_menu` (2.3.5, 2.3.11) is wired into a live hook (`hooks::context_menu`).
//! `message_logger` (2.3.4) still waits on the address of `MessageRow::render`, which the private
//! recon notes do not have yet.

pub mod context_menu;
pub mod message_logger;
