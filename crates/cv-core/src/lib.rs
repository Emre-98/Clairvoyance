//! Clairvoyance core. Game-agnostic: no game-specific code may live in this crate.

pub mod engine;
pub mod events;
pub mod game;
pub mod input;
pub mod library;
pub mod modes;
pub mod recorder;
pub mod scoreboard;
pub mod session;
pub mod settings;

pub use events::{EventKind, GameEvent};
pub use game::GameIntegration;
pub use recorder::Recorder;
pub use session::GameSession;
pub use settings::Settings;
