//! PostgreSQL implementation of the task repository.

use async_trait::async_trait;
use chrono::NaiveDate;
use sqlx::PgPool;

use crate::application::AppError;
use crate::application::tasks::{
    FieldPatch, SortField, SortOrder, TaskFilters, TaskPage, TaskPatch, TaskQuery, TaskRepository,
};
use crate::domain::task::{NewTask, Task, TaskId};

use super::errors::map_sqlx_error;
use super::task_row::TaskRow;

pub struct PgTaskRepository {
    pool: PgPool,
}

impl PgTaskRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// One `query_as!` per sort field and direction: the project forbids runtime
/// SQL, so every ordering is a static, compile-time-checked statement.
/// `$1`/`$2` are the optional status / priority filters, `$3` the limit and
/// `$4` the offset.
macro_rules! list_query {
    ($pool:expr, $filters:expr, $limit:expr, $offset:expr, $order:literal) => {
        sqlx::query_as!(
            TaskRow,
            "SELECT id, title, description, status, priority, due_date, created_at, updated_at \
             FROM tasks \
             WHERE ($1::text[] IS NULL OR status = ANY($1)) \
               AND ($2::text[] IS NULL OR priority = ANY($2)) \
             ORDER BY "
                + $order
                + " \
             LIMIT $3 OFFSET $4",
            $filters.0.as_deref(),
            $filters.1.as_deref(),
            $limit,
            $offset
        )
        .fetch_all($pool)
        .await
    };
}

fn filter_values(filters: &TaskFilters) -> (Option<Vec<String>>, Option<Vec<String>>) {
    let statuses = (!filters.statuses.is_empty()).then(|| {
        filters
            .statuses
            .iter()
            .map(|s| s.as_str().to_owned())
            .collect()
    });
    let priorities = (!filters.priorities.is_empty()).then(|| {
        filters
            .priorities
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect()
    });
    (statuses, priorities)
}

#[async_trait]
impl TaskRepository for PgTaskRepository {
    async fn create(&self, task: &NewTask) -> Result<Task, AppError> {
        let row = sqlx::query_as!(
            TaskRow,
            "INSERT INTO tasks (title, description, status, priority, due_date) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING id, title, description, status, priority, due_date, created_at, updated_at",
            task.title.as_str(),
            task.description.as_ref().map(|d| d.as_str()),
            task.status.as_str(),
            task.priority.as_str(),
            task.due_date.map(|d| d.value()),
        )
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;
        Task::try_from(row)
    }

    async fn find_by_id(&self, id: TaskId) -> Result<Option<Task>, AppError> {
        sqlx::query_as!(
            TaskRow,
            "SELECT id, title, description, status, priority, due_date, created_at, updated_at \
             FROM tasks WHERE id = $1",
            id.value()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(Task::try_from)
        .transpose()
    }

    async fn list(&self, query: &TaskQuery) -> Result<TaskPage, AppError> {
        let filters = filter_values(&query.filters);
        let (limit, offset) = (query.limit, query.offset);
        let pool = &self.pool;
        let rows = match (query.sort, query.order) {
            (SortField::CreatedAt, SortOrder::Asc) => {
                list_query!(pool, filters, limit, offset, "created_at ASC, id ASC")
            }
            (SortField::CreatedAt, SortOrder::Desc) => {
                list_query!(pool, filters, limit, offset, "created_at DESC, id ASC")
            }
            (SortField::UpdatedAt, SortOrder::Asc) => {
                list_query!(pool, filters, limit, offset, "updated_at ASC, id ASC")
            }
            (SortField::UpdatedAt, SortOrder::Desc) => {
                list_query!(pool, filters, limit, offset, "updated_at DESC, id ASC")
            }
            (SortField::DueDate, SortOrder::Asc) => {
                list_query!(
                    pool,
                    filters,
                    limit,
                    offset,
                    "due_date ASC NULLS LAST, id ASC"
                )
            }
            (SortField::DueDate, SortOrder::Desc) => {
                list_query!(
                    pool,
                    filters,
                    limit,
                    offset,
                    "due_date DESC NULLS LAST, id ASC"
                )
            }
            (SortField::Priority, SortOrder::Asc) => list_query!(
                pool,
                filters,
                limit,
                offset,
                "CASE priority WHEN 'low' THEN 1 WHEN 'medium' THEN 2 ELSE 3 END ASC, id ASC"
            ),
            (SortField::Priority, SortOrder::Desc) => list_query!(
                pool,
                filters,
                limit,
                offset,
                "CASE priority WHEN 'low' THEN 1 WHEN 'medium' THEN 2 ELSE 3 END DESC, id ASC"
            ),
        }
        .map_err(map_sqlx_error)?;

        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) AS \"count!\" FROM tasks \
             WHERE ($1::text[] IS NULL OR status = ANY($1)) \
               AND ($2::text[] IS NULL OR priority = ANY($2))",
            filters.0.as_deref(),
            filters.1.as_deref()
        )
        .fetch_one(pool)
        .await
        .map_err(map_sqlx_error)?;

        let items = rows
            .into_iter()
            .map(Task::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(TaskPage { items, total })
    }

    async fn update(&self, id: TaskId, patch: &TaskPatch) -> Result<Option<Task>, AppError> {
        let (set_description, description): (bool, Option<&str>) = match &patch.description {
            FieldPatch::Keep => (false, None),
            FieldPatch::Set(value) => (true, Some(value.as_str())),
            FieldPatch::Clear => (true, None),
        };
        let (set_due_date, due_date): (bool, Option<NaiveDate>) = match &patch.due_date {
            FieldPatch::Keep => (false, None),
            FieldPatch::Set(value) => (true, Some(value.value())),
            FieldPatch::Clear => (true, None),
        };
        sqlx::query_as!(
            TaskRow,
            "UPDATE tasks SET \
                title = COALESCE($2, title), \
                description = CASE WHEN $3 THEN $4 ELSE description END, \
                status = COALESCE($5, status), \
                priority = COALESCE($6, priority), \
                due_date = CASE WHEN $7 THEN $8 ELSE due_date END, \
                updated_at = now() \
             WHERE id = $1 \
             RETURNING id, title, description, status, priority, due_date, created_at, updated_at",
            id.value(),
            patch.title.as_ref().map(|t| t.as_str()),
            set_description,
            description,
            patch.status.map(|s| s.as_str()),
            patch.priority.map(|p| p.as_str()),
            set_due_date,
            due_date,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?
        .map(Task::try_from)
        .transpose()
    }

    async fn delete(&self, id: TaskId) -> Result<bool, AppError> {
        let result = sqlx::query!("DELETE FROM tasks WHERE id = $1", id.value())
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;
        Ok(result.rows_affected() > 0)
    }
}
