//! `PgTaskRepository` against a per-test database.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::HashSet;

use common::stored_task;
use sqlx::PgPool;
use task_api::application::tasks::{
    FieldPatch, SortField, SortOrder, TaskFilters, TaskPatch, TaskQuery, TaskRepository,
};
use task_api::domain::task::{
    NewTask, RawNewTask, Task, TaskId, TaskPriority, TaskStatus, TaskTitle,
};
use task_api::infrastructure::db::PgTaskRepository;
use uuid::Uuid;

fn new_task(title: &str, status: &str, priority: &str, due: Option<&str>) -> NewTask {
    NewTask::parse(RawNewTask {
        title: Some(title.to_owned()),
        description: None,
        status: Some(status.to_owned()),
        priority: Some(priority.to_owned()),
        due_date: due.map(str::to_owned),
    })
    .unwrap()
}

fn minimal(title: &str) -> NewTask {
    NewTask::parse(RawNewTask {
        title: Some(title.to_owned()),
        ..RawNewTask::default()
    })
    .unwrap()
}

async fn seed(repo: &PgTaskRepository, task: &NewTask) -> Task {
    repo.create(task).await.unwrap()
}

fn query(sort: SortField, order: SortOrder) -> TaskQuery {
    TaskQuery {
        sort,
        order,
        limit: 100,
        ..TaskQuery::default()
    }
}

#[sqlx::test]
async fn create_applies_defaults_and_equal_timestamps(pool: PgPool) {
    let repo = PgTaskRepository::new(pool);
    let task = seed(&repo, &minimal("t")).await;
    assert_eq!(task.status(), TaskStatus::Todo);
    assert_eq!(task.priority(), TaskPriority::Medium);
    assert!(task.description().is_none());
    assert!(task.due_date().is_none());
    assert_eq!(task.created_at(), task.updated_at());
    assert_ne!(task.id().value(), Uuid::nil());
}

#[sqlx::test]
async fn create_persists_given_values(pool: PgPool) {
    let repo = PgTaskRepository::new(pool.clone());
    let input = NewTask::parse(RawNewTask {
        title: Some("Ship it".into()),
        description: Some("Final review".into()),
        status: Some("in_progress".into()),
        priority: Some("high".into()),
        due_date: Some("2026-12-01".into()),
    })
    .unwrap();
    let task = seed(&repo, &input).await;
    let row = stored_task(&pool, task.id().value()).await.unwrap();
    assert_eq!(row.title, "Ship it");
    assert_eq!(row.description.as_deref(), Some("Final review"));
    assert_eq!(row.status, "in_progress");
    assert_eq!(row.priority, "high");
    assert_eq!(row.due_date.unwrap().to_string(), "2026-12-01");
}

#[sqlx::test]
async fn find_by_id_returns_task_or_none(pool: PgPool) {
    let repo = PgTaskRepository::new(pool);
    let task = seed(&repo, &minimal("t")).await;
    assert_eq!(repo.find_by_id(task.id()).await.unwrap(), Some(task));
    assert!(
        repo.find_by_id(TaskId::new(Uuid::from_u128(0xdead_beef)))
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn list_returns_all_with_total(pool: PgPool) {
    let repo = PgTaskRepository::new(pool);
    for i in 0..25 {
        seed(&repo, &minimal(&format!("task {i}"))).await;
    }
    let page = repo
        .list(&TaskQuery {
            limit: 10,
            ..TaskQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(page.items.len(), 10);
    assert_eq!(page.total, 25);
}

#[sqlx::test]
async fn list_filters_by_status_and_priority(pool: PgPool) {
    let repo = PgTaskRepository::new(pool);
    seed(&repo, &new_task("a", "todo", "low", None)).await;
    seed(&repo, &new_task("b", "todo", "high", None)).await;
    seed(&repo, &new_task("c", "done", "high", None)).await;
    seed(&repo, &new_task("d", "in_progress", "medium", None)).await;

    let or_within = repo
        .list(&TaskQuery {
            filters: TaskFilters {
                statuses: vec![TaskStatus::Todo, TaskStatus::Done],
                priorities: vec![],
            },
            ..query(SortField::CreatedAt, SortOrder::Asc)
        })
        .await
        .unwrap();
    assert_eq!(or_within.total, 3);

    let and_across = repo
        .list(&TaskQuery {
            filters: TaskFilters {
                statuses: vec![TaskStatus::Todo, TaskStatus::Done],
                priorities: vec![TaskPriority::High],
            },
            ..query(SortField::CreatedAt, SortOrder::Asc)
        })
        .await
        .unwrap();
    assert_eq!(and_across.total, 2);
    assert_eq!(and_across.items.len(), 2);
}

#[sqlx::test]
async fn list_sorts_priority_semantically(pool: PgPool) {
    let repo = PgTaskRepository::new(pool);
    for (title, priority) in [("m", "medium"), ("h", "high"), ("l", "low")] {
        seed(&repo, &new_task(title, "todo", priority, None)).await;
    }
    let titles = |page: task_api::application::tasks::TaskPage| -> Vec<String> {
        page.items
            .iter()
            .map(|t| t.title().as_str().to_owned())
            .collect()
    };
    let asc = repo
        .list(&query(SortField::Priority, SortOrder::Asc))
        .await
        .unwrap();
    assert_eq!(titles(asc), ["l", "m", "h"]);
    let desc = repo
        .list(&query(SortField::Priority, SortOrder::Desc))
        .await
        .unwrap();
    assert_eq!(titles(desc), ["h", "m", "l"]);
}

#[sqlx::test]
async fn list_places_null_due_dates_last(pool: PgPool) {
    let repo = PgTaskRepository::new(pool);
    seed(&repo, &new_task("none", "todo", "low", None)).await;
    seed(&repo, &new_task("early", "todo", "low", Some("2026-01-01"))).await;
    seed(&repo, &new_task("late", "todo", "low", Some("2026-06-01"))).await;
    for (order, expected) in [
        (SortOrder::Asc, ["early", "late", "none"]),
        (SortOrder::Desc, ["late", "early", "none"]),
    ] {
        let page = repo.list(&query(SortField::DueDate, order)).await.unwrap();
        let titles: Vec<_> = page.items.iter().map(|t| t.title().as_str()).collect();
        assert_eq!(titles, expected);
    }
}

#[sqlx::test]
async fn list_breaks_ties_by_id(pool: PgPool) {
    let repo = PgTaskRepository::new(pool);
    for i in 0..30 {
        seed(&repo, &new_task(&format!("t{i}"), "todo", "medium", None)).await;
    }
    let mut seen = HashSet::new();
    let mut offset = 0;
    loop {
        let page = repo
            .list(&TaskQuery {
                limit: 7,
                offset,
                ..query(SortField::Priority, SortOrder::Asc)
            })
            .await
            .unwrap();
        if page.items.is_empty() {
            break;
        }
        for task in &page.items {
            assert!(seen.insert(task.id()), "duplicate across pages");
        }
        offset += 7;
    }
    assert_eq!(seen.len(), 30);
}

#[sqlx::test]
async fn list_offset_beyond_total(pool: PgPool) {
    let repo = PgTaskRepository::new(pool);
    seed(&repo, &minimal("t")).await;
    let page = repo
        .list(&TaskQuery {
            offset: 1000,
            ..TaskQuery::default()
        })
        .await
        .unwrap();
    assert!(page.items.is_empty());
    assert_eq!(page.total, 1);
}

#[sqlx::test]
async fn update_changes_only_given_fields(pool: PgPool) {
    let repo = PgTaskRepository::new(pool);
    let original = seed(&repo, &new_task("keep", "todo", "low", Some("2026-01-01"))).await;
    let updated = repo
        .update(
            original.id(),
            &TaskPatch {
                status: Some(TaskStatus::Done),
                ..TaskPatch::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status(), TaskStatus::Done);
    assert_eq!(updated.title(), original.title());
    assert_eq!(updated.priority(), original.priority());
    assert_eq!(updated.due_date(), original.due_date());
    assert_eq!(updated.description(), original.description());
}

#[sqlx::test]
async fn update_can_set_and_clear_nullable_fields(pool: PgPool) {
    let repo = PgTaskRepository::new(pool.clone());
    let input = NewTask::parse(RawNewTask {
        title: Some("t".into()),
        description: Some("desc".into()),
        due_date: Some("2026-01-01".into()),
        ..RawNewTask::default()
    })
    .unwrap();
    let task = seed(&repo, &input).await;
    let cleared = repo
        .update(
            task.id(),
            &TaskPatch {
                description: FieldPatch::Clear,
                due_date: FieldPatch::Clear,
                title: Some(TaskTitle::parse("renamed").unwrap()),
                ..TaskPatch::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert!(cleared.description().is_none());
    assert!(cleared.due_date().is_none());
    assert_eq!(cleared.title().as_str(), "renamed");
    let row = stored_task(&pool, task.id().value()).await.unwrap();
    assert!(row.description.is_none() && row.due_date.is_none());
}

#[sqlx::test]
async fn update_refreshes_updated_at_only(pool: PgPool) {
    let repo = PgTaskRepository::new(pool);
    let original = seed(&repo, &minimal("t")).await;
    let updated = repo
        .update(
            original.id(),
            &TaskPatch {
                status: Some(TaskStatus::InProgress),
                ..TaskPatch::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert!(updated.updated_at() > original.updated_at());
    assert_eq!(updated.created_at(), original.created_at());
    assert_eq!(updated.id(), original.id());
}

#[sqlx::test]
async fn update_unknown_id_returns_none(pool: PgPool) {
    let repo = PgTaskRepository::new(pool);
    let result = repo
        .update(
            TaskId::new(Uuid::from_u128(0xdead_beef)),
            &TaskPatch {
                status: Some(TaskStatus::Done),
                ..TaskPatch::default()
            },
        )
        .await
        .unwrap();
    assert!(result.is_none());
}

#[sqlx::test]
async fn delete_removes_the_row(pool: PgPool) {
    let repo = PgTaskRepository::new(pool);
    let task = seed(&repo, &minimal("t")).await;
    assert!(repo.delete(task.id()).await.unwrap());
    assert!(repo.find_by_id(task.id()).await.unwrap().is_none());
}

#[sqlx::test]
async fn delete_unknown_id_returns_false(pool: PgPool) {
    let repo = PgTaskRepository::new(pool);
    assert!(
        !repo
            .delete(TaskId::new(Uuid::from_u128(0xdead_beef)))
            .await
            .unwrap()
    );
}
