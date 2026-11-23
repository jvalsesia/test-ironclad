//! `GET /tasks` query-string extractor.
//!
//! Turns the raw query string into a [`TaskQuery`], reporting every invalid
//! known parameter at once as `400 invalid_query`. Rules:
//!
//! * unknown names (including different-case variants) are ignored;
//! * a known name given more than once is rejected (`must not be repeated`);
//! * values are case-sensitive and never trimmed; empty values and empty
//!   list items are invalid;
//! * details always follow the order `status`, `priority`, `sort`, `order`,
//!   `limit`, `offset`, regardless of the order in the URL.

use axum::extract::{FromRequestParts, Query};
use axum::http::request::Parts;

use crate::application::tasks::{SortField, SortOrder, TaskQuery};
use crate::domain::task::{TaskPriority, TaskStatus};
use crate::http::error::{ApiError, ErrorDetail};

pub const MSG_INVALID_QUERY: &str = "Query parameters are invalid";
pub const MSG_SORT: &str = "must be one of: created_at, updated_at, due_date, priority";
pub const MSG_ORDER: &str = "must be one of: asc, desc";
pub const MSG_LIMIT: &str = "must be an integer between 1 and 100";
pub const MSG_OFFSET: &str = "must be an integer between 0 and 10000";
pub const MSG_REPEATED: &str = "must not be repeated";

pub const LIMIT_MIN: i64 = 1;
pub const LIMIT_MAX: i64 = 100;
pub const OFFSET_MIN: i64 = 0;
pub const OFFSET_MAX: i64 = 10_000;

/// The validated list query, with defaults applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListTasksParams(pub TaskQuery);

impl<S: Send + Sync> FromRequestParts<S> for ListTasksParams {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Query(pairs) = Query::<Vec<(String, String)>>::try_from_uri(&parts.uri)
            .map_err(|_| ApiError::invalid_query(MSG_INVALID_QUERY, Vec::new()))?;
        parse_pairs(&pairs).map(Self)
    }
}

/// The single value of `name`: `None` when absent, `Err` when repeated.
fn single<'a>(pairs: &'a [(String, String)], name: &str) -> Result<Option<&'a str>, ()> {
    let mut values = pairs
        .iter()
        .filter(|(key, _)| key == name)
        .map(|(_, value)| value.as_str());
    let first = values.next();
    if values.next().is_some() {
        return Err(());
    }
    Ok(first)
}

/// Parses one known parameter, recording a detail on failure.
fn field<T>(
    pairs: &[(String, String)],
    name: &str,
    details: &mut Vec<ErrorDetail>,
    parse: impl FnOnce(&str) -> Result<T, String>,
) -> Option<T> {
    match single(pairs, name) {
        Err(()) => {
            details.push(ErrorDetail::new(name, MSG_REPEATED));
            None
        }
        Ok(None) => None,
        Ok(Some(raw)) => match parse(raw) {
            Ok(value) => Some(value),
            Err(message) => {
                details.push(ErrorDetail::new(name, message));
                None
            }
        },
    }
}

fn parse_list<T>(
    raw: &str,
    parse_item: impl Fn(&str) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    raw.split(',').map(parse_item).collect()
}

fn parse_status(item: &str) -> Result<TaskStatus, String> {
    TaskStatus::parse(item).map_err(|err| err.message)
}

fn parse_priority(item: &str) -> Result<TaskPriority, String> {
    TaskPriority::parse(item).map_err(|err| err.message)
}

fn parse_sort(raw: &str) -> Result<SortField, String> {
    match raw {
        "created_at" => Ok(SortField::CreatedAt),
        "updated_at" => Ok(SortField::UpdatedAt),
        "due_date" => Ok(SortField::DueDate),
        "priority" => Ok(SortField::Priority),
        _ => Err(MSG_SORT.to_owned()),
    }
}

fn parse_order(raw: &str) -> Result<SortOrder, String> {
    match raw {
        "asc" => Ok(SortOrder::Asc),
        "desc" => Ok(SortOrder::Desc),
        _ => Err(MSG_ORDER.to_owned()),
    }
}

fn parse_bounded(raw: &str, min: i64, max: i64, message: &str) -> Result<i64, String> {
    match raw.parse::<i64>() {
        Ok(value) if (min..=max).contains(&value) => Ok(value),
        _ => Err(message.to_owned()),
    }
}

/// Validates the decoded key/value pairs in URL order.
pub fn parse_pairs(pairs: &[(String, String)]) -> Result<TaskQuery, ApiError> {
    let mut details = Vec::new();
    let mut query = TaskQuery::default();

    let statuses = field(pairs, "status", &mut details, |raw| {
        parse_list(raw, parse_status)
    });
    let priorities = field(pairs, "priority", &mut details, |raw| {
        parse_list(raw, parse_priority)
    });
    let sort = field(pairs, "sort", &mut details, parse_sort);
    let order = field(pairs, "order", &mut details, parse_order);
    let limit = field(pairs, "limit", &mut details, |raw| {
        parse_bounded(raw, LIMIT_MIN, LIMIT_MAX, MSG_LIMIT)
    });
    let offset = field(pairs, "offset", &mut details, |raw| {
        parse_bounded(raw, OFFSET_MIN, OFFSET_MAX, MSG_OFFSET)
    });

    if !details.is_empty() {
        return Err(ApiError::invalid_query(MSG_INVALID_QUERY, details));
    }

    if let Some(statuses) = statuses {
        query.filters.statuses = statuses;
    }
    if let Some(priorities) = priorities {
        query.filters.priorities = priorities;
    }
    if let Some(sort) = sort {
        query.sort = sort;
    }
    if let Some(order) = order {
        query.order = order;
    }
    if let Some(limit) = limit {
        query.limit = limit;
    }
    if let Some(offset) = offset {
        query.offset = offset;
    }
    Ok(query)
}

#[cfg(test)]
mod tests {
    use axum::http::Request;

    use crate::application::tasks::TaskFilters;
    use crate::http::error::ErrorCode;

    use super::*;

    const STATUS_MSG: &str = "must be one of: todo, in_progress, done";
    const PRIORITY_MSG: &str = "must be one of: low, medium, high";

    async fn extract(uri: &str) -> Result<TaskQuery, ApiError> {
        let (mut parts, ()) = Request::builder().uri(uri).body(()).unwrap().into_parts();
        ListTasksParams::from_request_parts(&mut parts, &())
            .await
            .map(|ListTasksParams(query)| query)
    }

    async fn details_of(uri: &str) -> Vec<(String, String)> {
        let err = extract(uri).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidQuery);
        assert_eq!(err.message, MSG_INVALID_QUERY);
        err.details
            .into_iter()
            .map(|d| (d.field, d.message))
            .collect()
    }

    fn one(field: &str, message: &str) -> Vec<(String, String)> {
        vec![(field.to_owned(), message.to_owned())]
    }

    #[tokio::test]
    async fn no_query_yields_defaults() {
        assert_eq!(extract("/tasks").await.unwrap(), TaskQuery::default());
        assert_eq!(extract("/tasks?").await.unwrap(), TaskQuery::default());
    }

    #[tokio::test]
    async fn all_parameters_are_parsed() {
        let query = extract(
            "/tasks?status=in_progress&priority=medium&sort=due_date&order=asc&limit=50&offset=10",
        )
        .await
        .unwrap();
        assert_eq!(
            query,
            TaskQuery {
                filters: TaskFilters {
                    statuses: vec![TaskStatus::InProgress],
                    priorities: vec![TaskPriority::Medium],
                },
                sort: SortField::DueDate,
                order: SortOrder::Asc,
                limit: 50,
                offset: 10,
            }
        );
        for (raw, expected) in [
            ("created_at", SortField::CreatedAt),
            ("updated_at", SortField::UpdatedAt),
            ("priority", SortField::Priority),
        ] {
            let query = extract(&format!("/tasks?sort={raw}&order=desc"))
                .await
                .unwrap();
            assert_eq!(query.sort, expected);
            assert_eq!(query.order, SortOrder::Desc);
        }
    }

    #[tokio::test]
    async fn lists_are_split_on_commas() {
        let query = extract("/tasks?status=todo,in_progress&priority=low,high")
            .await
            .unwrap();
        assert_eq!(
            query.filters.statuses,
            vec![TaskStatus::Todo, TaskStatus::InProgress]
        );
        assert_eq!(
            query.filters.priorities,
            vec![TaskPriority::Low, TaskPriority::High]
        );
        // A percent-encoded comma decodes to a separator too.
        let query = extract("/tasks?status=todo%2Cdone").await.unwrap();
        assert_eq!(
            query.filters.statuses,
            vec![TaskStatus::Todo, TaskStatus::Done]
        );
    }

    #[tokio::test]
    async fn duplicate_list_items_are_accepted() {
        let query = extract("/tasks?status=todo,todo").await.unwrap();
        assert!(
            query
                .filters
                .statuses
                .iter()
                .all(|s| *s == TaskStatus::Todo)
        );
    }

    #[tokio::test]
    async fn limit_and_offset_bounds() {
        assert_eq!(extract("/tasks?limit=1").await.unwrap().limit, 1);
        assert_eq!(extract("/tasks?limit=100").await.unwrap().limit, 100);
        assert_eq!(extract("/tasks?limit=%2B5").await.unwrap().limit, 5);
        assert_eq!(extract("/tasks?offset=0").await.unwrap().offset, 0);
        assert_eq!(extract("/tasks?offset=10000").await.unwrap().offset, 10_000);
        for value in ["0", "101", "-1"] {
            assert_eq!(
                details_of(&format!("/tasks?limit={value}")).await,
                one("limit", MSG_LIMIT)
            );
        }
        for value in ["-1", "10001"] {
            assert_eq!(
                details_of(&format!("/tasks?offset={value}")).await,
                one("offset", MSG_OFFSET)
            );
        }
    }

    #[tokio::test]
    async fn non_integer_numbers_are_rejected() {
        for value in ["abc", "1.0", "", "99999999999999999999", "1e2", "%201"] {
            assert_eq!(
                details_of(&format!("/tasks?limit={value}")).await,
                one("limit", MSG_LIMIT),
                "limit={value}"
            );
            assert_eq!(
                details_of(&format!("/tasks?offset={value}")).await,
                one("offset", MSG_OFFSET),
                "offset={value}"
            );
        }
    }

    #[tokio::test]
    async fn enum_values_are_case_sensitive() {
        assert_eq!(
            details_of("/tasks?status=TODO").await,
            one("status", STATUS_MSG)
        );
        assert_eq!(
            details_of("/tasks?priority=High").await,
            one("priority", PRIORITY_MSG)
        );
        assert_eq!(
            details_of("/tasks?order=DESC").await,
            one("order", MSG_ORDER)
        );
        assert_eq!(
            details_of("/tasks?sort=Created_at").await,
            one("sort", MSG_SORT)
        );
        assert_eq!(details_of("/tasks?sort=title").await, one("sort", MSG_SORT));
        assert_eq!(details_of("/tasks?order=up").await, one("order", MSG_ORDER));
    }

    #[tokio::test]
    async fn empty_values_and_items_are_rejected() {
        assert_eq!(
            details_of("/tasks?status=").await,
            one("status", STATUS_MSG)
        );
        assert_eq!(
            details_of("/tasks?status=todo,,done").await,
            one("status", STATUS_MSG)
        );
        assert_eq!(
            details_of("/tasks?status=todo,").await,
            one("status", STATUS_MSG)
        );
        assert_eq!(
            details_of("/tasks?priority=").await,
            one("priority", PRIORITY_MSG)
        );
        assert_eq!(details_of("/tasks?sort=").await, one("sort", MSG_SORT));
        assert_eq!(details_of("/tasks?order").await, one("order", MSG_ORDER));
        // `+` decodes to a space, which is never trimmed.
        assert_eq!(
            details_of("/tasks?status=todo+done").await,
            one("status", STATUS_MSG)
        );
        assert_eq!(
            details_of("/tasks?status=+todo").await,
            one("status", STATUS_MSG)
        );
    }

    #[tokio::test]
    async fn repeated_parameter_is_rejected() {
        assert_eq!(
            details_of("/tasks?status=todo&status=done").await,
            one("status", MSG_REPEATED)
        );
        assert_eq!(
            details_of("/tasks?limit=5&limit=5").await,
            one("limit", MSG_REPEATED)
        );
        // Only the repetition is reported, even when a value is also invalid.
        assert_eq!(
            details_of("/tasks?sort=bad&sort=worse").await,
            one("sort", MSG_REPEATED)
        );
    }

    #[tokio::test]
    async fn unknown_names_are_ignored() {
        for uri in [
            "/tasks?foo=bar",
            "/tasks?Status=closed",
            "/tasks?foo=1&foo=2",
            "/tasks?LIMIT=0&Offset=-1",
        ] {
            assert_eq!(extract(uri).await.unwrap(), TaskQuery::default(), "{uri}");
        }
    }

    #[tokio::test]
    async fn details_follow_fixed_order() {
        let fields: Vec<String> =
            details_of("/tasks?offset=x&limit=x&order=x&sort=x&priority=x&status=x")
                .await
                .into_iter()
                .map(|(field, _)| field)
                .collect();
        assert_eq!(
            fields,
            ["status", "priority", "sort", "order", "limit", "offset"]
        );
        assert_eq!(
            details_of("/tasks?limit=500&status=closed").await,
            vec![
                ("status".to_owned(), STATUS_MSG.to_owned()),
                ("limit".to_owned(), MSG_LIMIT.to_owned()),
            ]
        );
    }

    #[tokio::test]
    async fn invalid_utf8_value_is_rejected_for_its_parameter() {
        assert_eq!(
            details_of("/tasks?status=%FF").await,
            one("status", STATUS_MSG)
        );
        assert_eq!(
            details_of("/tasks?limit=%FF").await,
            one("limit", MSG_LIMIT)
        );
    }
}
