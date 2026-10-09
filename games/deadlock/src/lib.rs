//! Deadlock support.
//!
//! Valve has no Game State Integration and no other official live API for Deadlock, and the game
//! is VAC-protected, so this module only ever reads what the game or Steam writes to disk (the
//! console log, Valve's own replay files, Steam's recording timeline), the game's window title
//! and the user's own key presses. No memory reading, no injection, no in-game overlay.
//!
//! So far it holds the first step of the work: the developer diagnostic that records every
//! candidate "match started / ended" signal with its time ([`diag`]), so the signals can be
//! chosen from a real match instead of guessed. The game integration itself comes after that.

pub mod diag;
pub mod paths;
mod platform;

/// Stable id used in settings and session files.
pub const ID: &str = "deadlock";
