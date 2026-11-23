//! Use case: list one page of tasks.

use std::sync::Arc;

use crate::application::AppError;

use super::repository::{TaskPage, TaskQuery, TaskRepository};

pub struct ListTasks {
    tasks: Arc<dyn TaskRepository>,
}

impl ListTasks {
    pub fn new(tasks: Arc<dyn TaskRepository>) -> Self {
        Self { tasks }
    }

    /// The query is already validated by the transport layer; repository
    /// errors are propagated unchanged.
    pub async fn execute(&self, query: &TaskQuery) -> Result<TaskPage, AppError> {
        self.tasks.list(query).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use chrono::Utc;
    use uuid::Uuid;

    use crate::application::tasks::repository::{SortField, SortOrder, TaskFilters, TaskPatch};
    use crate::domain::task::{
        NewTask, Task, TaskId, TaskParts, TaskPriority, TaskStatus, TaskTitle,
    };

    use super::*;

    #[derive(Default)]
    struct FakeRepository {
        seen: Mutex<Vec<TaskQuery>>,
        unavailable: bool,
    }

    fn sample_task() -> Task {
        let now = Utc::now();
        Task::restore(TaskParts {
            id: TaskId::new(Uuid::nil()),
            title: TaskTitle::restore("Write PRD".into()),
            description: None,
            status: TaskStatus::Todo,
            priority: TaskPriority::High,
            due_date: None,
            created_at: now,
            updated_at: now,
        })
    }

    #[async_trait]
    impl TaskRepository for FakeRepository {
        async fn create(&self, _: &NewTask) -> Result<Task, AppError> {
            Ok(sample_task())
        }

        async fn find_by_id(&self, _: TaskId) -> Result<Option<Task>, AppError> {
            Ok(None)
        }

        async fn list(&self, query: &TaskQuery) -> Result<TaskPage, AppError> {
            if self.unavailable {
                return Err(AppError::Unavailable("down".into()));
            }
            if let Ok(mut seen) = self.seen.lock() {
                seen.push(query.clone());
            }
            Ok(TaskPage {
                items: vec![sample_task()],
                total: 42,
            })
        }

        async fn update(&self, _: TaskId, _: &TaskPatch) -> Result<Option<Task>, AppError> {
            Ok(None)
        }

        async fn delete(&self, _: TaskId) -> Result<bool, AppError> {
            Ok(false)
        }
    }

    #[tokio::test]
    async fn query_is_passed_to_the_repository() {
        let repo = Arc::new(FakeRepository::default());
        let query = TaskQuery {
            filters: TaskFilters {
                statuses: vec![TaskStatus::Todo, TaskStatus::Done],
                priorities: vec![TaskPriority::High],
            },
            sort: SortField::DueDate,
            order: SortOrder::Asc,
            limit: 7,
            offset: 14,
        };
        let page = ListTasks::new(repo.clone()).execute(&query).await.unwrap();
        assert_eq!(page.total, 42);
        assert_eq!(page.items.len(), 1);
        let seen = repo.seen.lock().unwrap();
        assert_eq!(seen.as_slice(), &[query]);
    }

    #[tokio::test]
    async fn repository_errors_are_propagated() {
        let repo = Arc::new(FakeRepository {
            unavailable: true,
            ..FakeRepository::default()
        });
        let result = ListTasks::new(repo).execute(&TaskQuery::default()).await;
        assert!(matches!(result, Err(AppError::Unavailable(_))));
    }
}
