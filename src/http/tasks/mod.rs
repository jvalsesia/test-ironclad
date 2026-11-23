//! Task endpoints. Each endpoint file exposes `routes()`; this module
//! merges them so later features only add a line here.

use axum::Router;

use super::state::AppState;

pub mod create;
pub mod dto;
pub mod list_params;
pub mod page;

pub use dto::{CreateTaskRequest, TaskResponse};

pub fn routes() -> Router<AppState> {
    Router::new().merge(create::routes())
}
