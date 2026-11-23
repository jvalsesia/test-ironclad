//! Update aggregate: applies the PATCH rules and reports every invalid field.
//!
//! A field may be absent (left unchanged), `null`, or a value. `null` clears
//! the nullable fields (`description`, `due_date`) and is rejected for the
//! others. Values go through the same value objects as creation.

use crate::domain::shared::{DomainError, FieldError};

use super::fields::{DueDate, TaskDescription, TaskPriority, TaskStatus, TaskTitle};

/// Message for `null` sent to a non-nullable field.
pub const CANNOT_BE_NULL_MESSAGE: &str = "cannot be null";

/// One unvalidated update field: absent, explicit `null`, or a text value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RawField {
    #[default]
    Absent,
    Null,
    Value(String),
}

impl RawField {
    pub fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }
}

/// Unvalidated update input, one entry per updatable field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawTaskChanges {
    pub title: RawField,
    pub description: RawField,
    pub status: RawField,
    pub priority: RawField,
    pub due_date: RawField,
}

/// What to do with a nullable field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FieldChange<T> {
    #[default]
    Keep,
    Set(T),
    Clear,
}

/// Validated update input. `None` / `Keep` leave a field unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskChanges {
    pub title: Option<TaskTitle>,
    pub description: FieldChange<TaskDescription>,
    pub status: Option<TaskStatus>,
    pub priority: Option<TaskPriority>,
    pub due_date: FieldChange<DueDate>,
}

impl TaskChanges {
    /// Validates every present field. Errors are reported in field order:
    /// `title`, `description`, `status`, `priority`, `due_date`. When every
    /// field is absent the result is `DomainError::NoChanges`.
    pub fn parse(raw: RawTaskChanges) -> Result<Self, DomainError> {
        let all_absent = raw.title.is_absent()
            && raw.description.is_absent()
            && raw.status.is_absent()
            && raw.priority.is_absent()
            && raw.due_date.is_absent();
        if all_absent {
            return Err(DomainError::NoChanges);
        }

        let mut errors: Vec<FieldError> = Vec::new();
        let title = required(&mut errors, "title", raw.title, TaskTitle::parse);
        let description = nullable(&mut errors, raw.description, |text| {
            TaskDescription::parse(text).map(|parsed| match parsed {
                Some(description) => FieldChange::Set(description),
                None => FieldChange::Clear,
            })
        });
        let status = required(&mut errors, "status", raw.status, TaskStatus::parse);
        let priority = required(&mut errors, "priority", raw.priority, TaskPriority::parse);
        let due_date = nullable(&mut errors, raw.due_date, |text| {
            DueDate::parse(text).map(FieldChange::Set)
        });

        if !errors.is_empty() {
            return Err(DomainError::Validation(errors));
        }
        Ok(Self {
            title,
            description,
            status,
            priority,
            due_date,
        })
    }
}

/// A non-nullable field: absent keeps it, `null` is an error.
fn required<T>(
    errors: &mut Vec<FieldError>,
    field: &str,
    raw: RawField,
    parse: impl FnOnce(&str) -> Result<T, FieldError>,
) -> Option<T> {
    match raw {
        RawField::Absent => None,
        RawField::Null => {
            errors.push(FieldError::new(field, CANNOT_BE_NULL_MESSAGE));
            None
        }
        RawField::Value(text) => match parse(&text) {
            Ok(value) => Some(value),
            Err(err) => {
                errors.push(err);
                None
            }
        },
    }
}

/// A nullable field: absent keeps it, `null` clears it.
fn nullable<T>(
    errors: &mut Vec<FieldError>,
    raw: RawField,
    parse: impl FnOnce(&str) -> Result<FieldChange<T>, FieldError>,
) -> FieldChange<T> {
    match raw {
        RawField::Absent => FieldChange::Keep,
        RawField::Null => FieldChange::Clear,
        RawField::Value(text) => parse(&text).unwrap_or_else(|err| {
            errors.push(err);
            FieldChange::Keep
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> RawField {
        RawField::Value(text.to_owned())
    }

    fn fields(err: DomainError) -> Vec<(String, String)> {
        match err {
            DomainError::Validation(list) => {
                list.into_iter().map(|e| (e.field, e.message)).collect()
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn all_absent_is_no_changes() {
        assert_eq!(
            TaskChanges::parse(RawTaskChanges::default()),
            Err(DomainError::NoChanges)
        );
    }

    #[test]
    fn single_field_is_set_others_kept() {
        let changes = TaskChanges::parse(RawTaskChanges {
            status: value("done"),
            ..RawTaskChanges::default()
        })
        .unwrap();
        assert_eq!(changes.status, Some(TaskStatus::Done));
        assert_eq!(changes.title, None);
        assert_eq!(changes.description, FieldChange::Keep);
        assert_eq!(changes.priority, None);
        assert_eq!(changes.due_date, FieldChange::Keep);
    }

    #[test]
    fn nullable_fields_can_be_cleared() {
        let changes = TaskChanges::parse(RawTaskChanges {
            description: RawField::Null,
            due_date: RawField::Null,
            ..RawTaskChanges::default()
        })
        .unwrap();
        assert_eq!(changes.description, FieldChange::Clear);
        assert_eq!(changes.due_date, FieldChange::Clear);
    }

    #[test]
    fn blank_description_clears() {
        let changes = TaskChanges::parse(RawTaskChanges {
            description: value("   "),
            ..RawTaskChanges::default()
        })
        .unwrap();
        assert_eq!(changes.description, FieldChange::Clear);
    }

    #[test]
    fn non_nullable_nulls_are_rejected() {
        let err = TaskChanges::parse(RawTaskChanges {
            title: RawField::Null,
            status: RawField::Null,
            priority: RawField::Null,
            ..RawTaskChanges::default()
        })
        .unwrap_err();
        assert_eq!(
            fields(err),
            vec![
                ("title".into(), "cannot be null".into()),
                ("status".into(), "cannot be null".into()),
                ("priority".into(), "cannot be null".into()),
            ]
        );
    }

    #[test]
    fn values_use_creation_rules() {
        let cases = [
            (
                RawTaskChanges {
                    title: value(&"a".repeat(201)),
                    ..RawTaskChanges::default()
                },
                ("title", "must be between 1 and 200 characters"),
            ),
            (
                RawTaskChanges {
                    description: value(&"a".repeat(5001)),
                    ..RawTaskChanges::default()
                },
                ("description", "must be at most 5000 characters"),
            ),
            (
                RawTaskChanges {
                    status: value("DONE"),
                    ..RawTaskChanges::default()
                },
                ("status", "must be one of: todo, in_progress, done"),
            ),
            (
                RawTaskChanges {
                    priority: value("urgent"),
                    ..RawTaskChanges::default()
                },
                ("priority", "must be one of: low, medium, high"),
            ),
            (
                RawTaskChanges {
                    due_date: value("2026-13-01"),
                    ..RawTaskChanges::default()
                },
                ("due_date", "must be a valid date in YYYY-MM-DD format"),
            ),
        ];
        for (raw, (field, message)) in cases {
            let err = TaskChanges::parse(raw).unwrap_err();
            assert_eq!(fields(err), vec![(field.to_owned(), message.to_owned())]);
        }
    }

    #[test]
    fn title_is_trimmed() {
        let changes = TaskChanges::parse(RawTaskChanges {
            title: value("  Ship it  "),
            ..RawTaskChanges::default()
        })
        .unwrap();
        assert_eq!(changes.title.unwrap().as_str(), "Ship it");
    }

    #[test]
    fn values_are_set() {
        let changes = TaskChanges::parse(RawTaskChanges {
            title: value("Ship it"),
            description: value(" Final review "),
            status: value("in_progress"),
            priority: value("low"),
            due_date: value("2027-01-15"),
        })
        .unwrap();
        match changes.description {
            FieldChange::Set(description) => assert_eq!(description.as_str(), "Final review"),
            other => panic!("unexpected description change: {other:?}"),
        }
        assert_eq!(changes.status, Some(TaskStatus::InProgress));
        assert_eq!(changes.priority, Some(TaskPriority::Low));
        match changes.due_date {
            FieldChange::Set(date) => assert_eq!(date.value().to_string(), "2027-01-15"),
            other => panic!("unexpected due date change: {other:?}"),
        }
    }

    #[test]
    fn errors_follow_field_order() {
        let err = TaskChanges::parse(RawTaskChanges {
            due_date: value("2026-02-30"),
            priority: RawField::Null,
            status: value("DONE"),
            description: value(&"a".repeat(5001)),
            title: value(""),
        })
        .unwrap_err();
        let names: Vec<_> = fields(err).into_iter().map(|f| f.0).collect();
        assert_eq!(
            names,
            ["title", "description", "status", "priority", "due_date"]
        );
    }

    #[test]
    fn nulls_only_report_field_errors_not_no_changes() {
        let err = TaskChanges::parse(RawTaskChanges {
            title: RawField::Null,
            ..RawTaskChanges::default()
        })
        .unwrap_err();
        assert_eq!(fields(err), vec![("title".into(), "cannot be null".into())]);
    }
}
