//! Documentation-only schema types.
//!
//! The domain enums stay free of framework code, so the published enum
//! schemas live here and DTO `String` fields point to them with
//! `value_type`. Unit tests keep these values in step with the parsers that
//! actually accept them.

use serde_json::{Value, json};
use utoipa::openapi::{RefOr, Schema};
use utoipa::{PartialSchema, ToSchema};

/// Published as `TaskStatus`.
#[derive(Debug, Clone, Copy, ToSchema)]
#[schema(as = TaskStatus, rename_all = "snake_case")]
pub enum TaskStatusSchema {
    Todo,
    InProgress,
    Done,
}

/// Published as `TaskPriority`.
#[derive(Debug, Clone, Copy, ToSchema)]
#[schema(as = TaskPriority, rename_all = "snake_case")]
pub enum TaskPrioritySchema {
    Low,
    Medium,
    High,
}

/// `ok` when the database answered within the health timeout, else `degraded`.
#[derive(Debug, Clone, Copy, ToSchema)]
#[schema(rename_all = "snake_case")]
pub enum HealthState {
    Ok,
    Degraded,
}

/// Database reachability.
#[derive(Debug, Clone, Copy, ToSchema)]
#[schema(rename_all = "snake_case")]
pub enum DatabaseState {
    Up,
    Down,
}

/// Sort field.
#[derive(Debug, Clone, Copy, ToSchema)]
#[schema(rename_all = "snake_case")]
pub enum SortFieldSchema {
    CreatedAt,
    UpdatedAt,
    DueDate,
    Priority,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, ToSchema)]
#[schema(rename_all = "snake_case")]
pub enum SortOrderSchema {
    Asc,
    Desc,
}

/// The inline schema of `T` with a `default`, emitted as one flat object
/// (utoipa would otherwise wrap an inline enum carrying a default in
/// `oneOf`).
fn with_default<T: PartialSchema>(default: &str) -> RefOr<Schema> {
    let mut schema = T::schema();
    if let RefOr::T(Schema::Object(object)) = &mut schema {
        object.default = Some(Value::from(default));
    }
    schema
}

/// `GET /tasks` `sort` parameter schema: the enum with `default: created_at`.
pub fn sort_param_schema() -> RefOr<Schema> {
    with_default::<SortFieldSchema>("created_at")
}

/// `GET /tasks` `order` parameter schema: the enum with `default: desc`.
pub fn order_param_schema() -> RefOr<Schema> {
    with_default::<SortOrderSchema>("desc")
}

/// Id used by every documented example.
pub const EXAMPLE_TASK_ID: &str = "3f2b6a1c-8d4e-4f7a-9b2c-1e5d7a9c0b3f";

/// `POST /tasks` request example (PRD F04).
pub fn create_task_request_example() -> Value {
    json!({"title": "Write PRD", "priority": "high", "due_date": "2026-12-01"})
}

/// `PATCH /tasks/{id}` request example (PRD F07).
pub fn update_task_request_example() -> Value {
    json!({"status": "done"})
}

/// The task created from [`create_task_request_example`].
pub fn task_example() -> Value {
    json!({
        "id": EXAMPLE_TASK_ID,
        "title": "Write PRD",
        "description": null,
        "status": "todo",
        "priority": "high",
        "due_date": "2026-12-01",
        "created_at": "2026-11-21T18:11:00.000Z",
        "updated_at": "2026-11-21T18:11:00.000Z"
    })
}

/// [`task_example`] after the [`update_task_request_example`] patch.
pub fn updated_task_example() -> Value {
    json!({
        "id": EXAMPLE_TASK_ID,
        "title": "Write PRD",
        "description": null,
        "status": "done",
        "priority": "high",
        "due_date": "2026-12-01",
        "created_at": "2026-11-21T18:11:00.000Z",
        "updated_at": "2026-11-22T09:30:00.000Z"
    })
}

/// A one-item page with the default `limit` and `offset`.
pub fn task_page_example() -> Value {
    json!({"items": [task_example()], "total": 1, "limit": 20, "offset": 0})
}

pub fn health_ok_example() -> Value {
    json!({"status": "ok", "database": "up", "version": env!("CARGO_PKG_VERSION")})
}

pub fn health_degraded_example() -> Value {
    json!({"status": "degraded", "database": "down", "version": env!("CARGO_PKG_VERSION")})
}

pub fn task_not_found_example() -> Value {
    json!({"error": {
        "code": "not_found",
        "message": format!("Task {EXAMPLE_TASK_ID} not found"),
        "details": []
    }})
}

pub fn invalid_id_example() -> Value {
    json!({"error": {"code": "invalid_id", "message": "Task id must be a valid UUID", "details": []}})
}

pub fn validation_error_example() -> Value {
    json!({"error": {
        "code": "validation_error",
        "message": "Request validation failed",
        "details": [{"field": "title", "message": "must be between 1 and 200 characters"}]
    }})
}

#[cfg(test)]
mod tests {

    use crate::domain::task::{TaskPriority, TaskStatus};
    use crate::http::tasks::list_params::parse_pairs;

    use super::*;

    fn enum_values<T: PartialSchema>() -> Vec<String> {
        let schema = serde_json::to_value(T::schema()).unwrap();
        assert_eq!(schema["type"], "string", "{schema}");
        schema["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect()
    }

    fn query_accepts(name: &str, value: &str) -> bool {
        parse_pairs(&[(name.to_owned(), value.to_owned())]).is_ok()
    }

    #[test]
    fn task_status_schema_matches_the_domain() {
        let values = enum_values::<TaskStatusSchema>();
        assert_eq!(values, ["todo", "in_progress", "done"]);
        for value in &values {
            assert!(TaskStatus::parse(value).is_ok(), "{value}");
        }
        for status in [TaskStatus::Todo, TaskStatus::InProgress, TaskStatus::Done] {
            assert!(values.iter().any(|v| v == status.as_str()), "{status:?}");
        }
    }

    #[test]
    fn task_priority_schema_matches_the_domain() {
        let values = enum_values::<TaskPrioritySchema>();
        assert_eq!(values, ["low", "medium", "high"]);
        for value in &values {
            assert!(TaskPriority::parse(value).is_ok(), "{value}");
        }
        for priority in [TaskPriority::Low, TaskPriority::Medium, TaskPriority::High] {
            assert!(
                values.iter().any(|v| v == priority.as_str()),
                "{priority:?}"
            );
        }
    }

    #[test]
    fn health_enums_have_the_served_values() {
        assert_eq!(enum_values::<HealthState>(), ["ok", "degraded"]);
        assert_eq!(enum_values::<DatabaseState>(), ["up", "down"]);
    }

    #[test]
    fn sort_and_order_schemas_match_the_query_parser() {
        let sorts = enum_values::<SortFieldSchema>();
        assert_eq!(sorts, ["created_at", "updated_at", "due_date", "priority"]);
        for value in &sorts {
            assert!(query_accepts("sort", value), "{value}");
        }
        let orders = enum_values::<SortOrderSchema>();
        assert_eq!(orders, ["asc", "desc"]);
        for value in &orders {
            assert!(query_accepts("order", value), "{value}");
        }
        assert!(!query_accepts("sort", "title"));
        assert!(!query_accepts("order", "up"));
    }
}
