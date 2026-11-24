//! The `GET /tasks` page envelope.

use serde::Serialize;
use utoipa::ToSchema;

use crate::application::tasks::{TaskPage, TaskQuery};

use super::dto::TaskResponse;

/// `{"items","total","limit","offset"}`, in that field order. `limit` and
/// `offset` echo the effective values (defaults applied). Published in the
/// OpenAPI document as `TaskPage`.
#[derive(Debug, Serialize, ToSchema)]
#[schema(as = TaskPage, description = "One page of tasks")]
pub struct TaskPageResponse {
    /// Tasks on this page, in the requested order
    pub items: Vec<TaskResponse>,
    /// Number of tasks matching the filters, across all pages
    #[schema(minimum = 0)]
    pub total: i64,
    /// Effective page size
    #[schema(minimum = 1, maximum = 100)]
    pub limit: i64,
    /// Effective number of skipped tasks
    #[schema(minimum = 0, maximum = 10000)]
    pub offset: i64,
}

impl TaskPageResponse {
    pub fn new(page: &TaskPage, query: &TaskQuery) -> Self {
        Self {
            items: page.items.iter().map(TaskResponse::from).collect(),
            total: page.total,
            limit: query.limit,
            offset: query.offset,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    use crate::domain::task::{Task, TaskId, TaskParts, TaskPriority, TaskStatus, TaskTitle};

    use super::*;

    #[test]
    fn envelope_field_order() {
        let page = TaskPage {
            items: Vec::new(),
            total: 0,
        };
        let json =
            serde_json::to_string(&TaskPageResponse::new(&page, &TaskQuery::default())).unwrap();
        assert_eq!(json, r#"{"items":[],"total":0,"limit":20,"offset":0}"#);
    }

    #[test]
    fn items_use_task_response() {
        let created = Utc.with_ymd_and_hms(2026, 11, 21, 18, 11, 0).unwrap();
        let task = Task::restore(TaskParts {
            id: TaskId::new(Uuid::nil()),
            title: TaskTitle::restore("Write PRD".into()),
            description: None,
            status: TaskStatus::Todo,
            priority: TaskPriority::High,
            due_date: None,
            created_at: created,
            updated_at: created,
        });
        let expected = serde_json::to_value(TaskResponse::from(&task)).unwrap();
        let page = TaskPage {
            items: vec![task],
            total: 31,
        };
        let query = TaskQuery {
            limit: 7,
            offset: 14,
            ..TaskQuery::default()
        };
        let json = serde_json::to_value(TaskPageResponse::new(&page, &query)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"items":[expected],"total":31,"limit":7,"offset":14})
        );
    }
}
