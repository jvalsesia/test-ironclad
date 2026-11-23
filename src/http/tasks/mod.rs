//! Task endpoints. Each endpoint file exposes `routes()`, registering its
//! annotated handler through `utoipa_axum::routes!` so the served path comes
//! from the OpenAPI annotation; this module merges them so later features
//! only add a line here.

use utoipa_axum::router::OpenApiRouter;

use super::state::AppState;

pub mod create;
pub mod delete;
pub mod dto;
pub mod get;
pub mod list;
pub mod list_params;
pub mod page;
pub mod task_id_path;
pub mod update;

pub use dto::{CreateTaskRequest, TaskResponse, UpdateTaskRequest};
pub use task_id_path::TaskIdPath;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .merge(create::routes())
        .merge(list::routes())
        .merge(get::routes())
        .merge(update::routes())
        .merge(delete::routes())
}
