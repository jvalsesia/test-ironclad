//! Use case: create a task.

use std::sync::Arc;

use crate::application::AppError;
use crate::domain::task::{NewTask, RawNewTask, Task};

use super::repository::TaskRepository;

pub struct CreateTask {
    tasks: Arc<dyn TaskRepository>,
}

impl CreateTask {
    pub fn new(tasks: Arc<dyn TaskRepository>) -> Self {
        Self { tasks }
    }

    /// Validates through the creation aggregate, then persists. The
    /// repository is never called with invalid input.
    pub async fn execute(&self, raw: RawNewTask) -> Result<Task, AppError> {
        let new_task = NewTask::parse(raw)?;
        self.tasks.create(&new_task).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use chrono::Utc;
    use uuid::Uuid;

    use crate::application::tasks::repository::{TaskPage, TaskPatch, TaskQuery};
    use crate::domain::task::{TaskId, TaskParts};

    use super::*;

    #[derive(Default)]
    struct FakeRepository {
        calls: AtomicUsize,
        seen: Mutex<Vec<NewTask>>,
        unavailable: bool,
    }

    #[async_trait]
    impl TaskRepository for FakeRepository {
        async fn create(&self, task: &NewTask) -> Result<Task, AppError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.unavailable {
                return Err(AppError::Unavailable("down".into()));
            }
            if let Ok(mut seen) = self.seen.lock() {
                seen.push(task.clone());
            }
            let now = Utc::now();
            Ok(Task::restore(TaskParts {
                id: TaskId::new(Uuid::nil()),
                title: task.title.clone(),
                description: task.description.clone(),
                status: task.status,
                priority: task.priority,
                due_date: task.due_date,
                created_at: now,
                updated_at: now,
            }))
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

        async fn delete(&self, _: TaskId) -> Result<bool, AppError> {
            Ok(false)
        }
    }

    fn raw(title: &str) -> RawNewTask {
        RawNewTask {
            title: Some(title.to_owned()),
            ..RawNewTask::default()
        }
    }

    #[tokio::test]
    async fn invalid_input_never_reaches_the_repository() {
        let repo = Arc::new(FakeRepository::default());
        let result = CreateTask::new(repo.clone()).execute(raw("   ")).await;
        assert!(matches!(result, Err(AppError::Validation(_))));
        assert_eq!(repo.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn valid_input_is_persisted_once() {
        let repo = Arc::new(FakeRepository::default());
        let task = CreateTask::new(repo.clone())
            .execute(raw("  Write PRD  "))
            .await
            .unwrap();
        assert_eq!(task.title().as_str(), "Write PRD");
        assert_eq!(repo.calls.load(Ordering::SeqCst), 1);
        let seen = repo.seen.lock().unwrap();
        assert_eq!(seen[0].title.as_str(), "Write PRD");
    }

    #[tokio::test]
    async fn repository_errors_are_propagated() {
        let repo = Arc::new(FakeRepository {
            unavailable: true,
            ..FakeRepository::default()
        });
        let result = CreateTask::new(repo).execute(raw("t")).await;
        assert!(matches!(result, Err(AppError::Unavailable(_))));
    }
}
