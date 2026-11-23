//! The shared "task not found" error, reused by every `/tasks/{id}` use case.

use crate::application::AppError;
use crate::domain::task::TaskId;

/// `Task <id> not found`, with the id in lowercase hyphenated form.
pub fn task_not_found(id: TaskId) -> AppError {
    AppError::NotFound(format!("Task {id} not found"))
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn message_uses_lowercase_canonical_id() {
        let uuid = Uuid::try_parse("3F2B6F0E-6D4A-4C8E-9A0F-1B2C3D4E5F60").unwrap();
        match task_not_found(TaskId::new(uuid)) {
            AppError::NotFound(message) => assert_eq!(
                message,
                "Task 3f2b6f0e-6d4a-4c8e-9a0f-1b2c3d4e5f60 not found"
            ),
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
