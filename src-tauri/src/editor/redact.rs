//! The editor's content-free log values (Task 58; F-50): `redact_path`,
//! `redact_name` and `redact_paths_in`. Since hardening Task 11 they live
//! in `vault_buddy_core::editor::redact` (its module doc is the contract),
//! so the core and screen crates' log lines can use them too; this
//! re-export keeps every shell call site's `redact::…` path unchanged.

pub use vault_buddy_core::editor::redact::{redact_name, redact_path, redact_paths_in};
