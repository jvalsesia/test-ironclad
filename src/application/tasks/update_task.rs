//! Use case: partially update a task.

use std::sync::Arc;

use crate::application::AppError;
use crate::domain::task::{FieldChange, RawTaskChanges, Task, TaskChanges, TaskId};

use super::not_found::task_not_found;
use super::repository::{FieldPatch, TaskPatch, TaskRepository};

pub struct UpdateTask {
    tasks: Arc<dyn TaskRepository>,
}

impl UpdateTask {
    pub fn new(tasks: Arc<dyn TaskRepository>) -> Self {
        Self { tasks }
    }

    /// Validates through the update aggregate, then applies the patch in a
    /// single repository call. Invalid or empty input never reaches the
    /// repository; an unknown id is reported as `task_not_found`.
    pub async fn execute(&self, id: TaskId, raw: RawTaskChanges) -> Result<Task, AppError> {
        let changes = TaskChanges::parse(raw)?;
        let patch = to_patch(changes);
        self.tasks
            .update(id, &patch)
            .await?
            .ok_or_else(|| task_not_found(id))
    }
}

fn to_patch(changes: TaskChanges) -> TaskPatch {
    TaskPatch {
        title: changes.title,
        description: to_field_patch(changes.description),
        status: changes.status,
        priority: changes.priority,
        due_date: to_field_patch(changes.due_date),
    }
}

fn to_field_patch<T>(change: FieldChange<T>) -> FieldPatch<T> {
    match change {
        FieldChange::Keep => FieldPatch::Keep,
        FieldChange::Set(value) => FieldPatch::Set(value),
        FieldChange::Clear => FieldPatch::Clear,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use chrono::Utc;
    use uuid::Uuid;

    use crate::application::tasks::repository::{TaskPage, TaskQuery};
    use crate::domain::task::{
        NewTask, RawField, TaskDescription, TaskParts, TaskPriority, TaskStatus, TaskTitle,
    };

    use super::*;

    #[derive(Default)]
    struct FakeRepository {
        calls: AtomicUsize,
        seen: Mutex<Vec<TaskPatch>>,
        missing: bool,
        unavailable: bool,
    }

    #[async_trait]
    impl TaskRepository for FakeRepository {
        async fn create(&self, _: &NewTask) -> Result<Task, AppError> {
            Err(AppError::Internal("not used".into()))
        }

        async fn find_by_id(&self, _: TaskId) -> Result<Option<Task>, AppError> {
            Ok(None)
        }

        async fn list(&self, _: &TaskQuery) -> Result<TaskPage, AppError> {
            Ok(TaskPage {
                items: Vec::new(),
                total: 0,
            })
        }

        async fn update(&self, id: TaskId, patch: &TaskPatch) -> Result<Option<Task>, AppError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.unavailable {
                return Err(AppError::Unavailable("down".into()));
            }
            if let Ok(mut seen) = self.seen.lock() {
                seen.push(patch.clone());
            }
            if self.missing {
                return Ok(None);
            }
            let now = Utc::now();
            Ok(Some(Task::restore(TaskParts {
                id,
                title: patch
                    .title
                    .clone()
                    .unwrap_or_else(|| TaskTitle::restore("Write PRD".into())),
                description: None,
                status: patch.status.unwrap_or(TaskStatus::Todo),
                priority: patch.priority.unwrap_or(TaskPriority::High),
                due_date: None,
                created_at: now,
                updated_at: now,
            })))
        }

        async fn delete(&self, _: TaskId) -> Result<bool, AppError> {
            Ok(false)
        }
    }

    fn id() -> TaskId {
        TaskId::new(Uuid::from_u128(0x3f2b_6f0e_6d4a_4c8e_9a0f_1b2c_3d4e_5f60))
    }

    fn status(value: &str) -> RawTaskChanges {
        RawTaskChanges {
            status: RawField::Value(value.to_owned()),
            ..RawTaskChanges::default()
        }
    }

    #[tokio::test]
    async fn invalid_input_never_reaches_the_repository() {
        let repo = Arc::new(FakeRepository::default());
        let result = UpdateTask::new(repo.clone())
            .execute(id(), status("DONE"))
            .await;
        assert!(matches!(result, Err(AppError::Validation(_))));
        assert_eq!(repo.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn no_changes_never_reaches_the_repository() {
        let repo = Arc::new(FakeRepository::default());
        let result = UpdateTask::new(repo.clone())
            .execute(id(), RawTaskChanges::default())
            .await;
        assert!(matches!(result, Err(AppError::NoChanges)));
        assert_eq!(repo.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn changes_map_to_patch() {
        let repo = Arc::new(FakeRepository::default());
        let task = UpdateTask::new(repo.clone())
            .execute(
                id(),
                RawTaskChanges {
                    title: RawField::Value("  Ship it  ".into()),
                    description: RawField::Value("Final review".into()),
                    status: RawField::Absent,
                    priority: RawField::Value("low".into()),
                    due_date: RawField::Null,
                },
            )
            .await
            .unwrap();
        assert_eq!(task.id(), id());
        assert_eq!(repo.calls.load(Ordering::SeqCst), 1);
        let seen = repo.seen.lock().unwrap();
        assert_eq!(
            seen[0],
            TaskPatch {
                title: Some(TaskTitle::restore("Ship it".into())),
                description: FieldPatch::Set(TaskDescription::restore("Final review".into())),
                status: None,
                priority: Some(TaskPriority::Low),
                due_date: FieldPatch::Clear,
            }
        );
    }

    #[tokio::test]
    async fn unknown_id_is_not_found() {
        let repo = Arc::new(FakeRepository {
            missing: true,
            ..FakeRepository::default()
        });
        let result = UpdateTask::new(repo).execute(id(), status("done")).await;
        match result {
            Err(AppError::NotFound(message)) => assert_eq!(
                message,
                "Task 3f2b6f0e-6d4a-4c8e-9a0f-1b2c3d4e5f60 not found"
            ),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[tokio::test]
    async fn repository_errors_are_propagated() {
        let repo = Arc::new(FakeRepository {
            unavailable: true,
            ..FakeRepository::default()
        });
        let result = UpdateTask::new(repo).execute(id(), status("done")).await;
        assert!(matches!(result, Err(AppError::Unavailable(_))));
    }
}
