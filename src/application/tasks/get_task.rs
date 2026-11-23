//! Use case: get one task by id.

use std::sync::Arc;

use crate::application::AppError;
use crate::domain::task::{Task, TaskId};

use super::not_found::task_not_found;
use super::repository::TaskRepository;

pub struct GetTask {
    tasks: Arc<dyn TaskRepository>,
}

impl GetTask {
    pub fn new(tasks: Arc<dyn TaskRepository>) -> Self {
        Self { tasks }
    }

    /// Loads the task; a missing task is `AppError::NotFound` with the
    /// shared message. Repository errors pass through unchanged.
    pub async fn execute(&self, id: TaskId) -> Result<Task, AppError> {
        self.tasks
            .find_by_id(id)
            .await?
            .ok_or_else(|| task_not_found(id))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use chrono::Utc;
    use uuid::Uuid;

    use crate::application::tasks::repository::{TaskPage, TaskPatch, TaskQuery};
    use crate::domain::task::{NewTask, TaskParts, TaskPriority, TaskStatus, TaskTitle};

    use super::*;

    #[derive(Default)]
    struct FakeRepository {
        calls: AtomicUsize,
        stored: Option<Task>,
        unavailable: bool,
    }

    #[async_trait]
    impl TaskRepository for FakeRepository {
        async fn create(&self, _: &NewTask) -> Result<Task, AppError> {
            Err(AppError::Internal("not used".into()))
        }

        async fn find_by_id(&self, id: TaskId) -> Result<Option<Task>, AppError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.unavailable {
                return Err(AppError::Unavailable("down".into()));
            }
            Ok(self.stored.clone().filter(|task| task.id() == id))
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

        async fn delete(&self, _: TaskId) -> Result<bool, AppError> {
            Ok(false)
        }
    }

    fn task(id: TaskId) -> Task {
        let now = Utc::now();
        Task::restore(TaskParts {
            id,
            title: TaskTitle::parse("Write PRD").unwrap(),
            description: None,
            status: TaskStatus::Todo,
            priority: TaskPriority::High,
            due_date: None,
            created_at: now,
            updated_at: now,
        })
    }

    #[tokio::test]
    async fn returns_the_stored_task() {
        let id = TaskId::new(Uuid::new_v4());
        let stored = task(id);
        let repo = Arc::new(FakeRepository {
            stored: Some(stored.clone()),
            ..FakeRepository::default()
        });
        let found = GetTask::new(repo.clone()).execute(id).await.unwrap();
        assert_eq!(found, stored);
        assert_eq!(repo.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unknown_id_is_not_found_with_message() {
        let id = TaskId::new(Uuid::new_v4());
        let repo = Arc::new(FakeRepository::default());
        match GetTask::new(repo).execute(id).await {
            Err(AppError::NotFound(message)) => {
                assert_eq!(message, format!("Task {} not found", id.value()));
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[tokio::test]
    async fn repository_errors_are_propagated() {
        let repo = Arc::new(FakeRepository {
            unavailable: true,
            ..FakeRepository::default()
        });
        let result = GetTask::new(repo)
            .execute(TaskId::new(Uuid::new_v4()))
            .await;
        assert!(matches!(result, Err(AppError::Unavailable(_))));
    }
}
