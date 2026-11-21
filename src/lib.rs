//! Task API library: configuration plus the domain, application,
//! infrastructure and HTTP layers. `main.rs` is the composition root.

pub mod application;
pub mod config;
pub mod domain;
pub mod http;
pub mod infrastructure;

pub use config::{Config, ConfigError};
