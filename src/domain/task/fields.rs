//! Task value objects. Each one validates and normalizes its own input and
//! produces the exact `FieldError` message shared by every endpoint.

use chrono::NaiveDate;

use crate::domain::shared::FieldError;

pub const TITLE_MAX_CHARS: usize = 200;
pub const DESCRIPTION_MAX_CHARS: usize = 5000;

const TITLE_MESSAGE: &str = "must be between 1 and 200 characters";
const DESCRIPTION_MESSAGE: &str = "must be at most 5000 characters";
const STATUS_MESSAGE: &str = "must be one of: todo, in_progress, done";
const PRIORITY_MESSAGE: &str = "must be one of: low, medium, high";
const DUE_DATE_MESSAGE: &str = "must be a valid date in YYYY-MM-DD format";

/// Message for a missing or `null` title.
pub const TITLE_REQUIRED_MESSAGE: &str = "is required";

/// Trimmed title of 1 to 200 characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTitle(String);

impl TaskTitle {
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        let trimmed = raw.trim();
        let length = trimmed.chars().count();
        if length == 0 || length > TITLE_MAX_CHARS {
            return Err(FieldError::new("title", TITLE_MESSAGE));
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// Rehydrates a value read from storage without re-running the rules.
    pub fn restore(stored: String) -> Self {
        Self(stored)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Trimmed, non-blank description of at most 5,000 characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDescription(String);

impl TaskDescription {
    /// A blank description is represented as `None`.
    pub fn parse(raw: &str) -> Result<Option<Self>, FieldError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        if trimmed.chars().count() > DESCRIPTION_MAX_CHARS {
            return Err(FieldError::new("description", DESCRIPTION_MESSAGE));
        }
        Ok(Some(Self(trimmed.to_owned())))
    }

    pub fn restore(stored: String) -> Self {
        Self(stored)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Todo,
    InProgress,
    Done,
}

impl TaskStatus {
    /// Exact, case-sensitive match.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        Self::restore(raw).ok_or_else(|| FieldError::new("status", STATUS_MESSAGE))
    }

    /// Returns `None` for an unknown stored value.
    pub fn restore(stored: &str) -> Option<Self> {
        match stored {
            "todo" => Some(Self::Todo),
            "in_progress" => Some(Self::InProgress),
            "done" => Some(Self::Done),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Done => "done",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPriority {
    Low,
    Medium,
    High,
}

impl TaskPriority {
    /// Exact, case-sensitive match.
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        Self::restore(raw).ok_or_else(|| FieldError::new("priority", PRIORITY_MESSAGE))
    }

    /// Returns `None` for an unknown stored value.
    pub fn restore(stored: &str) -> Option<Self> {
        match stored {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// A real calendar date written exactly as `YYYY-MM-DD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DueDate(NaiveDate);

impl DueDate {
    pub fn parse(raw: &str) -> Result<Self, FieldError> {
        let bytes = raw.as_bytes();
        let shaped = bytes.len() == 10
            && bytes.iter().enumerate().all(|(index, byte)| match index {
                4 | 7 => *byte == b'-',
                _ => byte.is_ascii_digit(),
            });
        if !shaped {
            return Err(FieldError::new("due_date", DUE_DATE_MESSAGE));
        }
        NaiveDate::parse_from_str(raw, "%Y-%m-%d")
            .map(Self)
            .map_err(|_| FieldError::new("due_date", DUE_DATE_MESSAGE))
    }

    pub fn restore(stored: NaiveDate) -> Self {
        Self(stored)
    }

    pub fn value(self) -> NaiveDate {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_is_trimmed() {
        assert_eq!(TaskTitle::parse("  Trim me  ").unwrap().as_str(), "Trim me");
    }

    #[test]
    fn title_length_bounds() {
        assert!(TaskTitle::parse("a").is_ok());
        assert!(TaskTitle::parse(&"a".repeat(200)).is_ok());
        for bad in ["a".repeat(201), String::new()] {
            let err = TaskTitle::parse(&bad).unwrap_err();
            assert_eq!(err.field, "title");
            assert_eq!(err.message, "must be between 1 and 200 characters");
        }
    }

    #[test]
    fn title_blank_is_rejected() {
        for bad in ["", "   ", "\t\n", " \u{a0} "] {
            assert!(TaskTitle::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn title_counts_characters_not_bytes() {
        assert!(TaskTitle::parse(&"é".repeat(200)).is_ok());
    }

    #[test]
    fn description_blank_becomes_none() {
        assert_eq!(TaskDescription::parse("   "), Ok(None));
    }

    #[test]
    fn description_length_bounds() {
        assert!(TaskDescription::parse(&"a".repeat(5000)).unwrap().is_some());
        let err = TaskDescription::parse(&"a".repeat(5001)).unwrap_err();
        assert_eq!(err.message, "must be at most 5000 characters");
    }

    #[test]
    fn status_values_are_case_sensitive() {
        for ok in ["todo", "in_progress", "done"] {
            assert_eq!(TaskStatus::parse(ok).unwrap().as_str(), ok);
        }
        for bad in ["DONE", "Todo", ""] {
            let err = TaskStatus::parse(bad).unwrap_err();
            assert_eq!(err.message, "must be one of: todo, in_progress, done");
        }
    }

    #[test]
    fn priority_values_are_case_sensitive() {
        for ok in ["low", "medium", "high"] {
            assert_eq!(TaskPriority::parse(ok).unwrap().as_str(), ok);
        }
        for bad in ["urgent", "HIGH"] {
            let err = TaskPriority::parse(bad).unwrap_err();
            assert_eq!(err.message, "must be one of: low, medium, high");
        }
    }

    #[test]
    fn due_date_accepts_valid_dates() {
        for ok in ["2026-12-01", "2024-02-29", "2000-01-01"] {
            assert!(DueDate::parse(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn due_date_rejects_invalid_dates() {
        for bad in [
            "2026-02-30",
            "2026-02-29",
            "12/01/2026",
            "2026-1-5",
            "2026-13-01",
            "",
            "+026-12-01",
        ] {
            let err = DueDate::parse(bad).unwrap_err();
            assert_eq!(err.message, "must be a valid date in YYYY-MM-DD format");
        }
    }
}
