//! The `tasks` table as created by the embedded migration, checked with
//! direct SQL against the per-test database.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use sqlx::PgPool;

async fn task_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM tasks"#)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Asserts the error is a database error with the given SQLSTATE and, when
/// given, constraint name.
fn assert_db_error(err: &sqlx::Error, code: &str, constraint: Option<&str>) {
    let db_err = err.as_database_error().expect("a database error");
    assert_eq!(db_err.code().as_deref(), Some(code), "{db_err}");
    assert_eq!(db_err.constraint(), constraint, "{db_err}");
}

#[sqlx::test]
async fn tasks_table_has_expected_columns(pool: PgPool) {
    let columns = sqlx::query!(
        r#"SELECT column_name::text AS "name!",
                  data_type::text AS "data_type!",
                  character_maximum_length AS "max_len",
                  is_nullable::text AS "nullable!"
           FROM information_schema.columns
           WHERE table_schema = 'public' AND table_name = 'tasks'
           ORDER BY ordinal_position"#
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let actual: Vec<(String, String, Option<i32>, bool)> = columns
        .into_iter()
        .map(|c| (c.name, c.data_type, c.max_len, c.nullable == "YES"))
        .collect();
    let expected: Vec<(String, String, Option<i32>, bool)> = [
        ("id", "uuid", None, false),
        ("title", "character varying", Some(200), false),
        ("description", "text", None, true),
        ("status", "text", None, false),
        ("priority", "text", None, false),
        ("due_date", "date", None, true),
        ("created_at", "timestamp with time zone", None, false),
        ("updated_at", "timestamp with time zone", None, false),
    ]
    .into_iter()
    .map(|(n, t, l, nullable)| (n.to_owned(), t.to_owned(), l, nullable))
    .collect();
    assert_eq!(actual, expected);

    let primary_key = sqlx::query_scalar!(
        r#"SELECT a.attname::text AS "name!"
           FROM pg_index i
           JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY (i.indkey)
           WHERE i.indrelid = 'public.tasks'::regclass AND i.indisprimary"#
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(primary_key, vec!["id".to_owned()]);
}

#[sqlx::test]
async fn tasks_table_has_expected_defaults(pool: PgPool) {
    let rows = sqlx::query!(
        r#"SELECT column_name::text AS "name!", column_default::text AS "default"
           FROM information_schema.columns
           WHERE table_schema = 'public' AND table_name = 'tasks'
           ORDER BY ordinal_position"#
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let default_of = |column: &str| -> Option<String> {
        rows.iter()
            .find(|row| row.name == column)
            .unwrap_or_else(|| panic!("column {column} missing"))
            .default
            .clone()
    };
    assert_eq!(default_of("id").as_deref(), Some("gen_random_uuid()"));
    assert_eq!(default_of("status").as_deref(), Some("'todo'::text"));
    assert_eq!(default_of("priority").as_deref(), Some("'medium'::text"));
    assert_eq!(default_of("created_at").as_deref(), Some("now()"));
    assert_eq!(default_of("updated_at").as_deref(), Some("now()"));
    assert_eq!(default_of("title"), None);
    assert_eq!(default_of("description"), None);
    assert_eq!(default_of("due_date"), None);
}

#[sqlx::test]
async fn tasks_table_has_expected_indexes(pool: PgPool) {
    let indexes = sqlx::query!(
        r#"SELECT indexname::text AS "name!", indexdef::text AS "definition!"
           FROM pg_indexes
           WHERE schemaname = 'public' AND tablename = 'tasks'"#
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let definition_of = |name: &str| -> String {
        indexes
            .iter()
            .find(|index| index.name == name)
            .unwrap_or_else(|| panic!("index {name} missing"))
            .definition
            .clone()
    };
    for column in ["status", "priority", "due_date", "created_at"] {
        let definition = definition_of(&format!("idx_tasks_{column}"));
        assert!(
            definition.contains(&format!("USING btree ({column})")),
            "{definition}"
        );
        assert!(!definition.contains("UNIQUE"), "{definition}");
    }
    let primary = definition_of("tasks_pkey");
    assert!(primary.contains("UNIQUE INDEX"), "{primary}");
    assert!(primary.contains("USING btree (id)"), "{primary}");
    assert_eq!(indexes.len(), 5);
}

#[sqlx::test]
async fn insert_with_title_only_applies_defaults(pool: PgPool) {
    assert_eq!(task_count(&pool).await, 0);
    let row = sqlx::query!(
        "INSERT INTO tasks (title) VALUES ('Write PRD')
         RETURNING id, title, description, status, priority, due_date, created_at, updated_at"
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(!row.id.is_nil());
    assert_eq!(row.title, "Write PRD");
    assert_eq!(row.status, "todo");
    assert_eq!(row.priority, "medium");
    assert_eq!(row.description, None);
    assert_eq!(row.due_date, None);
    assert_eq!(row.created_at, row.updated_at);
}

#[sqlx::test]
async fn rejects_archived_status(pool: PgPool) {
    let err = sqlx::query!("INSERT INTO tasks (title, status) VALUES ('t', 'archived')")
        .execute(&pool)
        .await
        .unwrap_err();
    assert_db_error(&err, "23514", Some("tasks_status_check"));
    assert_eq!(task_count(&pool).await, 0);
}

#[sqlx::test]
async fn rejects_urgent_priority(pool: PgPool) {
    let err = sqlx::query!("INSERT INTO tasks (title, priority) VALUES ('t', 'urgent')")
        .execute(&pool)
        .await
        .unwrap_err();
    assert_db_error(&err, "23514", Some("tasks_priority_check"));
    assert_eq!(task_count(&pool).await, 0);
}

#[sqlx::test]
async fn rejects_201_char_title(pool: PgPool) {
    // The PRD fixes title as VARCHAR(200), so PostgreSQL rejects the value
    // with SQLSTATE 22001 before tasks_title_check is evaluated (spec
    // Assumption 19).
    let title = "a".repeat(201);
    let err = sqlx::query!("INSERT INTO tasks (title) VALUES ($1)", title)
        .execute(&pool)
        .await
        .unwrap_err();
    assert_db_error(&err, "22001", None);
    assert_eq!(task_count(&pool).await, 0);
}

#[sqlx::test]
async fn accepts_200_char_title(pool: PgPool) {
    let title = "a".repeat(200);
    sqlx::query!("INSERT INTO tasks (title) VALUES ($1)", title)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(task_count(&pool).await, 1);
}

#[sqlx::test]
async fn rejects_blank_title(pool: PgPool) {
    let err = sqlx::query!("INSERT INTO tasks (title) VALUES ('   ')")
        .execute(&pool)
        .await
        .unwrap_err();
    assert_db_error(&err, "23514", Some("tasks_title_check"));
    assert_eq!(task_count(&pool).await, 0);
}

#[sqlx::test]
async fn rejects_5001_char_description(pool: PgPool) {
    let description = "a".repeat(5_001);
    let err = sqlx::query!(
        "INSERT INTO tasks (title, description) VALUES ('t', $1)",
        description
    )
    .execute(&pool)
    .await
    .unwrap_err();
    assert_db_error(&err, "23514", Some("tasks_description_check"));
    assert_eq!(task_count(&pool).await, 0);
}

#[sqlx::test]
async fn accepts_5000_char_description(pool: PgPool) {
    let description = "a".repeat(5_000);
    sqlx::query!(
        "INSERT INTO tasks (title, description) VALUES ('t', $1)",
        description
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(task_count(&pool).await, 1);
}
