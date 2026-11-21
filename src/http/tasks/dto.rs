//! Request and response bodies for the task endpoints.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::task::{RawNewTask, Task};

/// `POST /tasks` body. Every field is optional text so a missing or `null`
/// title is a domain validation error (422) while a wrongly typed value is
/// the extractor's 400. Unknown fields are ignored.
#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub due_date: Option<String>,
}

impl From<CreateTaskRequest> for RawNewTask {
    fn from(request: CreateTaskRequest) -> Self {
        Self {
            title: request.title,
            description: request.description,
            status: request.status,
            priority: request.priority,
            due_date: request.due_date,
        }
    }
}

/// The task representation shared by every task endpoint.
#[derive(Debug, Serialize)]
pub struct TaskResponse {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    pub due_date: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// RFC 3339 UTC with exactly three fractional digits; microseconds are
/// truncated, not rounded.
fn format_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

impl From<&Task> for TaskResponse {
    fn from(task: &Task) -> Self {
        Self {
            id: task.id().value().to_string(),
            title: task.title().as_str().to_owned(),
            description: task.description().map(|d| d.as_str().to_owned()),
            status: task.status().as_str().to_owned(),
            priority: task.priority().as_str().to_owned(),
            due_date: task
                .due_date()
                .map(|d| d.value().format("%Y-%m-%d").to_string()),
            created_at: format_timestamp(task.created_at()),
            updated_at: format_timestamp(task.updated_at()),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use uuid::Uuid;

    use crate::domain::task::{TaskId, TaskParts, TaskPriority, TaskStatus, TaskTitle};

    use super::*;

    fn task_at(created: DateTime<Utc>) -> Task {
        Task::restore(TaskParts {
            id: TaskId::new(Uuid::nil()),
            title: TaskTitle::restore("Write PRD".into()),
            description: None,
            status: TaskStatus::Todo,
            priority: TaskPriority::High,
            due_date: None,
            created_at: created,
            updated_at: created,
        })
    }

    #[test]
    fn response_field_order_and_nulls() {
        let created = Utc.with_ymd_and_hms(2026, 11, 21, 18, 11, 0).unwrap();
        let json = serde_json::to_string(&TaskResponse::from(&task_at(created))).unwrap();
        assert_eq!(
            json,
            r#"{"id":"00000000-0000-0000-0000-000000000000","title":"Write PRD","description":null,"status":"todo","priority":"high","due_date":null,"created_at":"2026-11-21T18:11:00.000Z","updated_at":"2026-11-21T18:11:00.000Z"}"#
        );
    }

    #[test]
    fn timestamps_use_millisecond_utc_format() {
        let created = Utc
            .with_ymd_and_hms(2026, 11, 21, 18, 11, 0)
            .unwrap()
            .checked_add_signed(chrono::Duration::microseconds(123_999))
            .unwrap();
        let response = TaskResponse::from(&task_at(created));
        assert_eq!(response.created_at, "2026-11-21T18:11:00.123Z");
    }
}
