//! HTTP layer: routes, handlers, error mapping and middleware.

pub mod error;
pub mod extract;
pub mod health;
pub mod middleware;
pub mod router;
pub mod server;
pub mod state;
pub mod test_routes;

pub use error::{ApiError, ErrorCode};
pub use extract::ApiJson;
pub use router::{RouterOptions, build_router};
pub use state::AppState;
