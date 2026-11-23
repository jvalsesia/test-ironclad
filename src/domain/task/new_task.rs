//! Creation aggregate: applies defaults and reports every invalid field.

use crate::domain::shared::{DomainError, FieldError};

use super::fields::{
    DueDate, TITLE_REQUIRED_MESSAGE, TaskDescription, TaskPriority, TaskStatus, TaskTitle,
};

/// Unvalidated creation input. `None` means absent or `null`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawNewTask {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub due_date: Option<String>,
}

/// Validated creation input, ready to be persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTask {
    pub title: TaskTitle,
    pub description: Option<TaskDescription>,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub due_date: Option<DueDate>,
}

impl NewTask {
    /// Validates every field. Errors are reported in field order: `title`,
    /// `description`, `status`, `priority`, `due_date`.
    pub fn parse(raw: RawNewTask) -> Result<Self, DomainError> {
        let mut errors: Vec<FieldError> = Vec::new();
        let title = match raw.title.as_deref() {
            Some(text) => take(&mut errors, TaskTitle::parse(text)),
            None => take(
                &mut errors,
                Err(FieldError::new("title", TITLE_REQUIRED_MESSAGE)),
            ),
        };
        let description = raw
            .description
            .as_deref()
            .map(|text| take(&mut errors, TaskDescription::parse(text)))
            .unwrap_or(Some(None));
        let status = raw
            .status
            .as_deref()
            .map(|text| take(&mut errors, TaskStatus::parse(text)))
            .unwrap_or(Some(TaskStatus::Todo));
        let priority = raw
            .priority
            .as_deref()
            .map(|text| take(&mut errors, TaskPriority::parse(text)))
            .unwrap_or(Some(TaskPriority::Medium));
        let due_date = raw
            .due_date
            .as_deref()
            .map(|text| take(&mut errors, DueDate::parse(text).map(Some)))
            .unwrap_or(Some(None));

        match (title, description, status, priority, due_date) {
            (Some(title), Some(description), Some(status), Some(priority), Some(due_date)) => {
                Ok(Self {
                    title,
                    description,
                    status,
                    priority,
                    due_date,
                })
            }
            _ => Err(DomainError::Validation(errors)),
        }
    }
}

fn take<T>(errors: &mut Vec<FieldError>, result: Result<T, FieldError>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(err) => {
            errors.push(err);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(title: &str) -> RawNewTask {
        RawNewTask {
            title: Some(title.to_owned()),
            ..RawNewTask::default()
        }
    }

    fn fields(err: DomainError) -> Vec<(String, String)> {
        let DomainError::Validation(list) = err else {
            panic!("unexpected error: {err:?}");
        };
        list.into_iter().map(|e| (e.field, e.message)).collect()
    }

    #[test]
    fn minimal_payload_applies_defaults() {
        let task = NewTask::parse(raw("Write PRD")).unwrap();
        assert_eq!(task.title.as_str(), "Write PRD");
        assert_eq!(task.status, TaskStatus::Todo);
        assert_eq!(task.priority, TaskPriority::Medium);
        assert!(task.description.is_none());
        assert!(task.due_date.is_none());
    }

    #[test]
    fn all_fields_are_kept() {
        let task = NewTask::parse(RawNewTask {
            title: Some("Ship it".into()),
            description: Some(" Final review ".into()),
            status: Some("in_progress".into()),
            priority: Some("high".into()),
            due_date: Some("2026-12-01".into()),
        })
        .unwrap();
        assert_eq!(task.description.unwrap().as_str(), "Final review");
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.priority, TaskPriority::High);
        assert_eq!(task.due_date.unwrap().value().to_string(), "2026-12-01");
    }

    #[test]
    fn missing_and_null_title_report_required() {
        let err = NewTask::parse(RawNewTask::default()).unwrap_err();
        assert_eq!(fields(err), vec![("title".into(), "is required".into())]);
    }

    #[test]
    fn all_invalid_fields_are_reported_together() {
        let err = NewTask::parse(RawNewTask {
            title: Some("   ".into()),
            priority: Some("urgent".into()),
            due_date: Some("2026-02-30".into()),
            ..RawNewTask::default()
        })
        .unwrap_err();
        let names: Vec<_> = fields(err).into_iter().map(|f| f.0).collect();
        assert_eq!(names, ["title", "priority", "due_date"]);
    }

    #[test]
    fn errors_follow_field_order() {
        let err = NewTask::parse(RawNewTask {
            title: Some(String::new()),
            description: Some("a".repeat(5001)),
            status: Some("DONE".into()),
            priority: Some("urgent".into()),
            due_date: Some("nope".into()),
        })
        .unwrap_err();
        let names: Vec<_> = fields(err).into_iter().map(|f| f.0).collect();
        assert_eq!(
            names,
            ["title", "description", "status", "priority", "due_date"]
        );
    }
}
