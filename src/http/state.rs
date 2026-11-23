//! Shared application state. Later features add their use cases here.

use std::sync::Arc;

use crate::application::DiagnosticsProbe;
use crate::application::tasks::TaskRepository;

#[derive(Clone)]
pub struct AppState {
    pub diagnostics: Arc<dyn DiagnosticsProbe>,
    pub tasks: Arc<dyn TaskRepository>,
}

impl AppState {
    pub fn new(diagnostics: Arc<dyn DiagnosticsProbe>, tasks: Arc<dyn TaskRepository>) -> Self {
        Self { diagnostics, tasks }
    }
}
