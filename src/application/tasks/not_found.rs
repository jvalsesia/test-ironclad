//! The shared "task not found" error, used by every `/tasks/{id}` use case.

use crate::application::AppError;
use crate::domain::task::TaskId;

/// `AppError::NotFound("Task <id> not found")`, with the id in lowercase
/// hyphenated form.
pub fn task_not_found(id: TaskId) -> AppError {
    AppError::NotFound(format!("Task {id} not found"))
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn message_names_the_lowercase_id() {
        let id = TaskId::new(Uuid::from_u128(0xA1B2C3D4_0000_4000_8000_00000000000F));
        match task_not_found(id) {
            AppError::NotFound(message) => assert_eq!(
                message,
                "Task a1b2c3d4-0000-4000-8000-00000000000f not found"
            ),
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
