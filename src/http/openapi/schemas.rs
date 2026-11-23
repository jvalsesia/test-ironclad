//! Documentation-only schema types.
//!
//! The domain enums stay free of framework code, so the published enum
//! schemas live here and DTO `String` fields point to them with
//! `value_type`. Unit tests keep these values in step with the parsers that
//! actually accept them.

use serde_json::Value;
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

/// `ok` when the database answered within the health timeout, `degraded`
/// otherwise.
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
