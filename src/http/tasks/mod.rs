//! Task endpoints. Each endpoint file exposes `routes()`; this module
//! merges them so later features only add a line here.

use axum::Router;

use super::state::AppState;

pub mod create;
pub mod delete;
pub mod dto;
pub mod get;
pub mod task_id_path;

pub use dto::{CreateTaskRequest, TaskResponse};
pub use task_id_path::TaskIdPath;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(create::routes())
        .merge(get::routes())
        .merge(delete::routes())
}
