//! Row mapping between the `tasks` table and the domain entity.

use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

use crate::application::AppError;
use crate::domain::task::{
    DueDate, Task, TaskDescription, TaskId, TaskParts, TaskPriority, TaskStatus, TaskTitle,
};

pub struct TaskRow {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    pub due_date: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TryFrom<TaskRow> for Task {
    type Error = AppError;

    /// Trusts the database, but an unknown stored enum value is an
    /// internal error rather than a panic.
    fn try_from(row: TaskRow) -> Result<Self, AppError> {
        let status = TaskStatus::restore(&row.status).ok_or_else(|| {
            AppError::Internal(format!("unknown stored task status '{}'", row.status).into())
        })?;
        let priority = TaskPriority::restore(&row.priority).ok_or_else(|| {
            AppError::Internal(format!("unknown stored task priority '{}'", row.priority).into())
        })?;
        Ok(Task::restore(TaskParts {
            id: TaskId::new(row.id),
            title: TaskTitle::restore(row.title),
            description: row.description.map(TaskDescription::restore),
            status,
            priority,
            due_date: row.due_date.map(DueDate::restore),
            created_at: row.created_at,
            updated_at: row.updated_at,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(status: &str, priority: &str) -> TaskRow {
        let now = Utc::now();
        TaskRow {
            id: Uuid::nil(),
            title: "t".into(),
            description: None,
            status: status.into(),
            priority: priority.into(),
            due_date: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn valid_row_maps_to_a_task() {
        let task = Task::try_from(row("done", "low")).unwrap();
        assert_eq!(task.status(), TaskStatus::Done);
        assert_eq!(task.priority(), TaskPriority::Low);
    }

    #[test]
    fn unknown_enum_values_are_internal_errors() {
        assert!(matches!(
            Task::try_from(row("weird", "low")),
            Err(AppError::Internal(_))
        ));
        assert!(matches!(
            Task::try_from(row("todo", "weird")),
            Err(AppError::Internal(_))
        ));
    }
}
