//! Shared application state. Later features add their use cases here.

use std::sync::Arc;

use crate::application::DiagnosticsProbe;

#[derive(Clone)]
pub struct AppState {
    pub diagnostics: Arc<dyn DiagnosticsProbe>,
}

impl AppState {
    pub fn new(diagnostics: Arc<dyn DiagnosticsProbe>) -> Self {
        Self { diagnostics }
    }
}
