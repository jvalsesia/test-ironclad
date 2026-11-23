//! Shared not-found error for task endpoints addressed by id.

use crate::application::AppError;
use crate::domain::task::TaskId;

/// `AppError::NotFound("Task <id> not found")`, with the id in its
/// canonical lowercase hyphenated form. Shared by get, update and delete.
pub fn task_not_found(id: TaskId) -> AppError {
    AppError::NotFound(format!("Task {id} not found"))
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn message_uses_lowercase_canonical_id() {
        let uuid = Uuid::parse_str("0B7D2C1E-5F3A-4E8B-9C6D-7A1F2E3D4C5B").unwrap();
        match task_not_found(TaskId::new(uuid)) {
            AppError::NotFound(message) => assert_eq!(
                message,
                "Task 0b7d2c1e-5f3a-4e8b-9c6d-7a1f2e3d4c5b not found"
            ),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
