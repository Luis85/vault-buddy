//! Audio capture engine for Knowledge Intake: devices → mixer → MP3
//! encoder → crash-safe .part writer. Obsidian never sees a half-written
//! file; the vault only ever contains hidden .part temps and final MP3s.

pub mod devices;
pub mod encoder;
pub mod mixer;
pub mod recovery;
pub mod session;
