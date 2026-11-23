//! Use case: delete a task.

use std::sync::Arc;

use crate::application::AppError;
use crate::domain::task::TaskId;

use super::not_found::task_not_found;
use super::repository::TaskRepository;

pub struct DeleteTask {
    tasks: Arc<dyn TaskRepository>,
}

impl DeleteTask {
    pub fn new(tasks: Arc<dyn TaskRepository>) -> Self {
        Self { tasks }
    }

    /// Permanently removes the task. A single statement decides whether the
    /// task existed, so a repeated delete reports not found.
    pub async fn execute(&self, id: TaskId) -> Result<(), AppError> {
        if self.tasks.delete(id).await? {
            Ok(())
        } else {
            Err(task_not_found(id))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use uuid::Uuid;

    use crate::application::tasks::repository::{TaskPage, TaskPatch, TaskQuery};
    use crate::domain::task::{NewTask, Task};

    use super::*;

    enum Outcome {
        Removed,
        Missing,
        Unavailable,
        Internal,
    }

    struct FakeRepository {
        outcome: Outcome,
        deleted: Mutex<Vec<TaskId>>,
    }

    impl FakeRepository {
        fn new(outcome: Outcome) -> Arc<Self> {
            Arc::new(Self {
                outcome,
                deleted: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl TaskRepository for FakeRepository {
        async fn create(&self, _: &NewTask) -> Result<Task, AppError> {
            Err(AppError::Internal("unused".into()))
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

        async fn update(&self, _: TaskId, _: &TaskPatch) -> Result<Option<Task>, AppError> {
            Ok(None)
        }

        async fn delete(&self, id: TaskId) -> Result<bool, AppError> {
            self.deleted.lock().unwrap().push(id);
            match self.outcome {
                Outcome::Removed => Ok(true),
                Outcome::Missing => Ok(false),
                Outcome::Unavailable => Err(AppError::Unavailable("down".into())),
                Outcome::Internal => Err(AppError::Internal("boom".into())),
            }
        }
    }

    fn id() -> TaskId {
        TaskId::new(Uuid::new_v4())
    }

    #[tokio::test]
    async fn removed_task_returns_ok() {
        let repo = FakeRepository::new(Outcome::Removed);
        let id = id();
        DeleteTask::new(repo.clone()).execute(id).await.unwrap();
        assert_eq!(*repo.deleted.lock().unwrap(), vec![id]);
    }

    #[tokio::test]
    async fn missing_task_is_not_found_with_message() {
        let repo = FakeRepository::new(Outcome::Missing);
        let id = id();
        match DeleteTask::new(repo).execute(id).await {
            Err(AppError::NotFound(message)) => {
                assert_eq!(message, format!("Task {} not found", id.value()));
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[tokio::test]
    async fn unavailable_error_is_propagated() {
        let repo = FakeRepository::new(Outcome::Unavailable);
        let result = DeleteTask::new(repo).execute(id()).await;
        assert!(matches!(result, Err(AppError::Unavailable(_))));
    }

    #[tokio::test]
    async fn internal_error_is_propagated() {
        let repo = FakeRepository::new(Outcome::Internal);
        let result = DeleteTask::new(repo).execute(id()).await;
        assert!(matches!(result, Err(AppError::Internal(_))));
    }
}
