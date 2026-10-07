//! Watchtower: a morning overview of all your agent-built software projects.
//!
//! Facts are measured from git (`facts`), agents write interpretation (`cards`)
//! through the CLI (`cli`), and the window (`app`) shows both. `service` owns all
//! behavior; `store` persists it in one SQLite file.

#[cfg(feature = "desktop")]
pub mod app;
pub mod cards;
pub mod cli;
mod cli_output;
pub mod facts;
pub mod service;
pub mod store;
pub mod system;

pub use service::{Watchtower, WatchtowerError};
