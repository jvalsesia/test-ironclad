//! `GET /openapi.json` and the Swagger UI at `/docs` (F10): document content
//! and documentation routes. No query runs, so the router is built over a
//! lazily connected pool and these tests do not need a database.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeSet;

use axum::Router;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;

use common::{
    Reply, assert_uniform_error, get, request_with, resolve, send, test_router,
    validate_with_schema,
};

const METHODS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

const TASK_OPERATIONS: [(&str, &str); 5] = [
    ("post", "/tasks"),
    ("get", "/tasks"),
    ("get", "/tasks/{id}"),
    ("patch", "/tasks/{id}"),
    ("delete", "/tasks/{id}"),
];

fn router(test_routes: bool) -> Router {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .unwrap();
    test_router(pool, test_routes)
}

async fn spec_reply(test_routes: bool) -> Reply {
    send(&router(test_routes), get("/openapi.json")).await
}

async fn spec() -> Value {
    spec_reply(false).await.json()
}

fn header<'a>(reply: &'a Reply, name: &str) -> Option<&'a str> {
    reply.headers.get(name).map(|v| v.to_str().unwrap())
}

fn assert_request_id(reply: &Reply) {
    let id = header(reply, "x-request-id").expect("X-Request-Id header");
    let parsed = uuid::Uuid::parse_str(id).unwrap();
    assert_eq!(parsed.get_version_num(), 4, "{id}");
    assert_eq!(id, parsed.hyphenated().to_string(), "{id}");
}

fn op<'a>(spec: &'a Value, method: &str, path: &str) -> &'a Value {
    let operation = &spec["paths"][path][method];
    assert!(operation.is_object(), "missing operation {method} {path}");
    operation
}

fn response_codes(spec: &Value, method: &str, path: &str) -> Vec<String> {
    op(spec, method, path)["responses"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

fn schema<'a>(spec: &'a Value, name: &str) -> &'a Value {
    let schema = &spec["components"]["schemas"][name];
    assert!(schema.is_object(), "missing schema {name}");
    schema
}

/// Whether `value` refers to `#/components/schemas/<name>`, directly or as
/// the single non-null member of `allOf`/`oneOf`/`anyOf`.
fn refers_to(value: &Value, name: &str) -> bool {
    let target = format!("#/components/schemas/{name}");
    if value["$ref"] == target.as_str() {
        return true;
    }
    ["allOf", "oneOf", "anyOf"].iter().any(|key| {
        value[*key].as_array().is_some_and(|members| {
            let non_null: Vec<&Value> = members
                .iter()
                .filter(|m| **m != json!({"type": "null"}))
                .collect();
            non_null.len() == 1 && non_null[0]["$ref"] == target.as_str()
        })
    })
}

fn admits_null(value: &Value) -> bool {
    let typed = match &value["type"] {
        Value::String(t) => t == "null",
        Value::Array(types) => types.iter().any(|t| t == "null"),
        _ => false,
    };
    typed
        || ["allOf", "oneOf", "anyOf"].iter().any(|key| {
            value[*key]
                .as_array()
                .is_some_and(|members| members.iter().any(|m| *m == json!({"type": "null"})))
        })
}

fn has_type(value: &Value, expected: &str) -> bool {
    match &value["type"] {
        Value::String(t) => t == expected,
        Value::Array(types) => types.iter().any(|t| t == expected),
        _ => false,
    }
}

fn sorted_strings(value: &Value) -> Vec<String> {
    let mut items: Vec<String> = value
        .as_array()
        .unwrap_or_else(|| panic!("not an array: {value}"))
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    items.sort();
    items
}

fn strings(list: &[&str]) -> Vec<String> {
    let mut items: Vec<String> = list.iter().map(|s| (*s).to_owned()).collect();
    items.sort();
    items
}

fn property<'a>(spec: &'a Value, schema_name: &str, name: &str) -> &'a Value {
    let property = &schema(spec, schema_name)["properties"][name];
    assert!(property.is_object(), "missing {schema_name}.{name}");
    property
}

fn parameter<'a>(spec: &'a Value, method: &str, path: &str, name: &str) -> &'a Value {
    op(spec, method, path)["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("missing parameter {name} on {method} {path}"))
}

/// Every `$ref` string anywhere under `value`.
fn collect_refs(value: &Value, refs: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == "$ref" {
                    refs.push(child.as_str().unwrap().to_owned());
                } else {
                    collect_refs(child, refs);
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_refs(item, refs)),
        _ => {}
    }
}

/// The example carried by a media type or parameter (its own `example`, a
/// non-empty `examples`, or its schema's `example`).
fn carried_example(holder: &Value) -> Option<Value> {
    if let Some(example) = holder.get("example") {
        return Some(example.clone());
    }
    if let Some(first) = holder["examples"]
        .as_object()
        .and_then(|examples| examples.values().next())
    {
        return Some(first["value"].clone());
    }
    holder["schema"].get("example").cloned()
}

fn json_media(response: &Value) -> &Value {
    let media = &response["content"]["application/json"];
    assert!(media.is_object(), "no application/json content: {response}");
    media
}

// ---------------------------------------------------------------- Serve spec

#[tokio::test]
async fn openapi_json_is_openapi_3_1_json() {
    let reply = spec_reply(false).await;
    assert_eq!(reply.status.as_u16(), 200, "body: {}", reply.body);
    assert_eq!(header(&reply, "content-type"), Some("application/json"));
    assert_eq!(reply.json()["openapi"], "3.1.0");
    assert_request_id(&reply);
}

#[tokio::test]
async fn info_title_and_version() {
    let spec = spec().await;
    assert_eq!(spec["info"]["title"], "Task API");
    assert_eq!(spec["info"]["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn document_validates_against_oas_3_1_schema() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/openapi-3.1-schema.json")).unwrap();
    assert_eq!(
        fixture["$id"],
        "https://spec.openapis.org/oas/3.1/schema/2022-10-07"
    );
    let validator = jsonschema::validator_for(&fixture).unwrap();
    let spec = spec().await;
    let errors: Vec<String> = validator
        .iter_errors(&spec)
        .map(|err| format!("{} at {}", err, err.instance_path))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");

    let mut broken = spec.clone();
    broken["paths"]["/tasks"]["get"]["responses"] = json!("nope");
    assert!(
        !validator.is_valid(&broken),
        "the validator must reject a broken document"
    );
}

#[tokio::test]
async fn every_ref_resolves() {
    let spec = spec().await;
    let mut refs = Vec::new();
    collect_refs(&spec, &mut refs);
    assert!(!refs.is_empty());
    for reference in refs {
        let name = reference
            .strip_prefix("#/components/schemas/")
            .unwrap_or_else(|| panic!("unexpected $ref form: {reference}"));
        assert!(
            spec["components"]["schemas"].get(name).is_some(),
            "{reference} does not resolve"
        );
    }
}

#[tokio::test]
async fn docs_and_test_routes_are_excluded() {
    let plain = spec_reply(false).await;
    let with_test_routes = spec_reply(true).await;
    let spec = with_test_routes.json();
    let paths: BTreeSet<String> = spec["paths"].as_object().unwrap().keys().cloned().collect();
    assert_eq!(
        paths,
        BTreeSet::from([
            "/health".to_owned(),
            "/tasks".to_owned(),
            "/tasks/{id}".to_owned()
        ])
    );
    for path in &paths {
        assert_ne!(path, "/openapi.json");
        assert!(!path.starts_with("/docs"), "{path}");
        assert!(!path.starts_with("/__test"), "{path}");
    }
    assert_eq!(
        with_test_routes.body, plain.body,
        "document depends on test routes"
    );
}

#[tokio::test]
async fn no_fixed_server_is_declared() {
    let spec = spec().await;
    if let Some(servers) = spec.get("servers") {
        for server in servers.as_array().unwrap() {
            assert_eq!(server["url"], "/", "{server}");
        }
    }
}

#[tokio::test]
async fn openapi_json_rejects_post() {
    let reply = send(
        &router(false),
        request_with("POST", "/openapi.json", Some("application/json"), "{}"),
    )
    .await;
    assert_uniform_error(&reply, 405, "method_not_allowed", "Method not allowed");
}

// ---------------------------------------------------------------- Operations

#[tokio::test]
async fn exactly_six_operations() {
    let spec = spec().await;
    let mut operations = BTreeSet::new();
    for (path, item) in spec["paths"].as_object().unwrap() {
        for method in METHODS {
            if item.get(method).is_some() {
                operations.insert(format!("{method} {path}"));
            }
        }
    }
    let expected: BTreeSet<String> = [
        "get /health",
        "post /tasks",
        "get /tasks",
        "get /tasks/{id}",
        "patch /tasks/{id}",
        "delete /tasks/{id}",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    assert_eq!(
        operations,
        expected,
        "documented operation set mismatch ({} documented, 6 expected)",
        operations.len()
    );
}

#[tokio::test]
async fn tags_and_operation_ids() {
    let spec = spec().await;
    let tags: Vec<&str> = spec["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(tags, ["Tasks", "Health"]);
    for (method, path) in TASK_OPERATIONS {
        assert_eq!(
            op(&spec, method, path)["tags"],
            json!(["Tasks"]),
            "{method} {path}"
        );
    }
    assert_eq!(op(&spec, "get", "/health")["tags"], json!(["Health"]));

    let expected = [
        ("post", "/tasks", "createTask"),
        ("get", "/tasks", "listTasks"),
        ("get", "/tasks/{id}", "getTask"),
        ("patch", "/tasks/{id}", "updateTask"),
        ("delete", "/tasks/{id}", "deleteTask"),
        ("get", "/health", "getHealth"),
    ];
    for (method, path, id) in expected {
        assert_eq!(
            op(&spec, method, path)["operationId"],
            id,
            "{method} {path}"
        );
    }
    let mut ids = Vec::new();
    for item in spec["paths"].as_object().unwrap().values() {
        for method in METHODS {
            if let Some(id) = item.get(method).and_then(|o| o["operationId"].as_str()) {
                ids.push(id.to_owned());
            }
        }
    }
    ids.sort();
    assert_eq!(
        ids,
        strings(&[
            "createTask",
            "listTasks",
            "getTask",
            "updateTask",
            "deleteTask",
            "getHealth"
        ])
    );
}

#[tokio::test]
async fn documented_statuses_per_operation() {
    let spec = spec().await;
    let table: [(&str, &str, &[&str]); 6] = [
        ("get", "/health", &["200", "503"]),
        (
            "post",
            "/tasks",
            &["201", "400", "413", "415", "422", "500", "503"],
        ),
        ("get", "/tasks", &["200", "400", "500", "503"]),
        ("get", "/tasks/{id}", &["200", "400", "404", "500", "503"]),
        (
            "patch",
            "/tasks/{id}",
            &["200", "400", "404", "413", "415", "422", "500", "503"],
        ),
        (
            "delete",
            "/tasks/{id}",
            &["204", "400", "404", "500", "503"],
        ),
    ];
    for (method, path, codes) in table {
        let mut documented = response_codes(&spec, method, path);
        documented.sort();
        assert_eq!(documented, strings(codes), "{method} {path}");
    }
}

#[tokio::test]
async fn error_responses_reference_error_response() {
    let spec = spec().await;
    for (method, path) in TASK_OPERATIONS {
        for (code, response) in op(&spec, method, path)["responses"].as_object().unwrap() {
            if code.starts_with('4') || code.starts_with('5') {
                assert!(
                    refers_to(&json_media(response)["schema"], "ErrorResponse"),
                    "{method} {path} {code}"
                );
            }
        }
    }
    for code in ["200", "503"] {
        let response = &op(&spec, "get", "/health")["responses"][code];
        assert!(
            refers_to(&json_media(response)["schema"], "HealthResponse"),
            "{code}"
        );
    }
    let no_content = &op(&spec, "delete", "/tasks/{id}")["responses"]["204"];
    assert!(no_content.get("content").is_none(), "{no_content}");
    let created = &op(&spec, "post", "/tasks")["responses"]["201"];
    assert!(created["headers"].get("Location").is_some(), "{created}");
}

#[tokio::test]
async fn success_responses_and_request_bodies_reference_their_schemas() {
    let spec = spec().await;
    for (method, path, code) in [
        ("post", "/tasks", "201"),
        ("get", "/tasks/{id}", "200"),
        ("patch", "/tasks/{id}", "200"),
    ] {
        let response = &op(&spec, method, path)["responses"][code];
        assert!(
            refers_to(&json_media(response)["schema"], "Task"),
            "{method} {path}"
        );
    }
    let page = &op(&spec, "get", "/tasks")["responses"]["200"];
    assert!(refers_to(&json_media(page)["schema"], "TaskPage"));
    for (method, path, name) in [
        ("post", "/tasks", "CreateTaskRequest"),
        ("patch", "/tasks/{id}", "UpdateTaskRequest"),
    ] {
        let body = &op(&spec, method, path)["requestBody"];
        assert_eq!(body["required"], true, "{method} {path}");
        assert!(
            refers_to(&json_media(body)["schema"], name),
            "{method} {path}"
        );
    }
}

// ------------------------------------------------------------------- Schemas

#[tokio::test]
async fn exactly_nine_component_schemas() {
    let spec = spec().await;
    let names: Vec<String> = spec["components"]["schemas"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let mut names = names;
    names.sort();
    assert_eq!(
        names,
        strings(&[
            "Task",
            "CreateTaskRequest",
            "UpdateTaskRequest",
            "TaskPage",
            "TaskStatus",
            "TaskPriority",
            "ErrorResponse",
            "ErrorDetail",
            "HealthResponse",
        ])
    );
}

#[tokio::test]
async fn enum_schemas() {
    let spec = spec().await;
    assert_eq!(schema(&spec, "TaskStatus")["type"], "string");
    assert_eq!(
        schema(&spec, "TaskStatus")["enum"],
        json!(["todo", "in_progress", "done"])
    );
    assert_eq!(schema(&spec, "TaskPriority")["type"], "string");
    assert_eq!(
        schema(&spec, "TaskPriority")["enum"],
        json!(["low", "medium", "high"])
    );
    assert!(refers_to(property(&spec, "Task", "status"), "TaskStatus"));
    assert!(refers_to(
        property(&spec, "Task", "priority"),
        "TaskPriority"
    ));
}

#[tokio::test]
async fn task_schema_constraints() {
    let spec = spec().await;
    assert_eq!(
        sorted_strings(&schema(&spec, "Task")["required"]),
        strings(&[
            "id",
            "title",
            "description",
            "status",
            "priority",
            "due_date",
            "created_at",
            "updated_at"
        ])
    );
    let id = property(&spec, "Task", "id");
    assert!(has_type(id, "string"));
    assert_eq!(id["format"], "uuid");
    for name in ["created_at", "updated_at"] {
        let timestamp = property(&spec, "Task", name);
        assert!(has_type(timestamp, "string"), "{name}");
        assert_eq!(timestamp["format"], "date-time", "{name}");
    }
    for name in [
        "id",
        "title",
        "status",
        "priority",
        "created_at",
        "updated_at",
    ] {
        assert!(!admits_null(property(&spec, "Task", name)), "{name}");
    }
    let title = property(&spec, "Task", "title");
    assert!(has_type(title, "string"));
    assert_eq!(title["minLength"], 1);
    assert_eq!(title["maxLength"], 200);
    let description = property(&spec, "Task", "description");
    assert!(admits_null(description));
    assert_eq!(description["maxLength"], 5000);
    let due_date = property(&spec, "Task", "due_date");
    assert!(admits_null(due_date));
    assert_eq!(due_date["format"], "date");
}

#[tokio::test]
async fn create_request_schema() {
    let spec = spec().await;
    assert_eq!(
        schema(&spec, "CreateTaskRequest")["required"],
        json!(["title"])
    );
    let title = property(&spec, "CreateTaskRequest", "title");
    assert!(has_type(title, "string"));
    assert!(!admits_null(title));
    assert_eq!(title["minLength"], 1);
    assert_eq!(title["maxLength"], 200);
    let description = property(&spec, "CreateTaskRequest", "description");
    assert!(admits_null(description));
    assert_eq!(description["maxLength"], 5000);
    let due_date = property(&spec, "CreateTaskRequest", "due_date");
    assert!(admits_null(due_date));
    assert_eq!(due_date["format"], "date");

    let status = property(&spec, "CreateTaskRequest", "status");
    assert!(refers_to(status, "TaskStatus"));
    assert_eq!(status["default"], "todo");
    assert!(!admits_null(status));
    let priority = property(&spec, "CreateTaskRequest", "priority");
    assert!(refers_to(priority, "TaskPriority"));
    assert_eq!(priority["default"], "medium");
    assert!(!admits_null(priority));
}

#[tokio::test]
async fn update_request_schema_nullability() {
    let spec = spec().await;
    let update = schema(&spec, "UpdateTaskRequest");
    assert!(
        update
            .get("required")
            .is_none_or(|r| r.as_array().is_some_and(Vec::is_empty)),
        "{update}"
    );
    let mut keys: Vec<String> = update["properties"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        strings(&["title", "description", "status", "priority", "due_date"])
    );
    let description = property(&spec, "UpdateTaskRequest", "description");
    assert!(admits_null(description));
    assert_eq!(description["maxLength"], 5000);
    let due_date = property(&spec, "UpdateTaskRequest", "due_date");
    assert!(admits_null(due_date));
    assert_eq!(due_date["format"], "date");
    let title = property(&spec, "UpdateTaskRequest", "title");
    assert!(!admits_null(title));
    assert_eq!(title["minLength"], 1);
    assert_eq!(title["maxLength"], 200);
    let status = property(&spec, "UpdateTaskRequest", "status");
    assert!(refers_to(status, "TaskStatus"));
    assert!(!admits_null(status));
    let priority = property(&spec, "UpdateTaskRequest", "priority");
    assert!(refers_to(priority, "TaskPriority"));
    assert!(!admits_null(priority));
}

#[tokio::test]
async fn task_page_schema() {
    let spec = spec().await;
    assert_eq!(
        sorted_strings(&schema(&spec, "TaskPage")["required"]),
        strings(&["items", "total", "limit", "offset"])
    );
    let items = property(&spec, "TaskPage", "items");
    assert!(has_type(items, "array"));
    assert!(refers_to(&items["items"], "Task"));
    let total = property(&spec, "TaskPage", "total");
    assert!(has_type(total, "integer"));
    assert_eq!(total["minimum"], 0);
    let limit = property(&spec, "TaskPage", "limit");
    assert!(has_type(limit, "integer"));
    assert_eq!(
        (limit["minimum"].clone(), limit["maximum"].clone()),
        (json!(1), json!(100))
    );
    let offset = property(&spec, "TaskPage", "offset");
    assert!(has_type(offset, "integer"));
    assert_eq!(
        (offset["minimum"].clone(), offset["maximum"].clone()),
        (json!(0), json!(10000))
    );
}

#[tokio::test]
async fn error_and_health_schemas() {
    let spec = spec().await;
    assert_eq!(schema(&spec, "ErrorResponse")["required"], json!(["error"]));
    let error = property(&spec, "ErrorResponse", "error");
    assert!(has_type(error, "object"));
    assert_eq!(
        sorted_strings(&error["required"]),
        strings(&["code", "message", "details"])
    );
    let code = &error["properties"]["code"];
    assert!(has_type(code, "string"));
    assert_eq!(
        sorted_strings(&code["enum"]),
        strings(&[
            "invalid_json",
            "invalid_id",
            "invalid_query",
            "not_found",
            "method_not_allowed",
            "unsupported_media_type",
            "payload_too_large",
            "validation_error",
            "service_unavailable",
            "internal_error",
        ])
    );
    let details = &error["properties"]["details"];
    assert!(has_type(details, "array"));
    assert!(refers_to(&details["items"], "ErrorDetail"));

    assert_eq!(
        sorted_strings(&schema(&spec, "ErrorDetail")["required"]),
        strings(&["field", "message"])
    );
    assert_eq!(property(&spec, "ErrorDetail", "field")["type"], "string");
    assert_eq!(property(&spec, "ErrorDetail", "message")["type"], "string");

    assert_eq!(
        sorted_strings(&schema(&spec, "HealthResponse")["required"]),
        strings(&["status", "database", "version"])
    );
    let status = property(&spec, "HealthResponse", "status");
    assert_eq!(status["type"], "string");
    assert_eq!(status["enum"], json!(["ok", "degraded"]));
    let database = property(&spec, "HealthResponse", "database");
    assert_eq!(database["type"], "string");
    assert_eq!(database["enum"], json!(["up", "down"]));
    assert_eq!(
        property(&spec, "HealthResponse", "version")["type"],
        "string"
    );
}

// ---------------------------------------------------------------- Parameters

#[tokio::test]
async fn list_query_parameters() {
    let spec = spec().await;
    let params = op(&spec, "get", "/tasks")["parameters"].as_array().unwrap();
    let names: Vec<&str> = params.iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        ["status", "priority", "sort", "order", "limit", "offset"]
    );
    for param in params {
        assert_eq!(param["in"], "query", "{param}");
        assert_ne!(param["required"], true, "{param}");
    }

    for (name, item) in [("status", "TaskStatus"), ("priority", "TaskPriority")] {
        let param = parameter(&spec, "get", "/tasks", name);
        assert!(has_type(&param["schema"], "array"), "{param}");
        assert!(refers_to(&param["schema"]["items"], item), "{param}");
        assert_eq!(param["style"], "form", "{param}");
        assert_eq!(param["explode"], false, "{param}");
    }

    let sort = &parameter(&spec, "get", "/tasks", "sort")["schema"];
    assert_eq!(sort["type"], "string");
    assert_eq!(
        sorted_strings(&sort["enum"]),
        strings(&["created_at", "updated_at", "due_date", "priority"])
    );
    assert_eq!(sort["default"], "created_at");
    let order = &parameter(&spec, "get", "/tasks", "order")["schema"];
    assert_eq!(order["type"], "string");
    assert_eq!(sorted_strings(&order["enum"]), strings(&["asc", "desc"]));
    assert_eq!(order["default"], "desc");

    let limit = &parameter(&spec, "get", "/tasks", "limit")["schema"];
    assert_eq!(limit["type"], "integer");
    assert_eq!(
        [&limit["minimum"], &limit["maximum"], &limit["default"]],
        [&json!(1), &json!(100), &json!(20)]
    );
    let offset = &parameter(&spec, "get", "/tasks", "offset")["schema"];
    assert_eq!(offset["type"], "integer");
    assert_eq!(
        [&offset["minimum"], &offset["maximum"], &offset["default"]],
        [&json!(0), &json!(10000), &json!(0)]
    );
}

#[tokio::test]
async fn id_path_parameter() {
    let spec = spec().await;
    for method in ["get", "patch", "delete"] {
        let params = op(&spec, method, "/tasks/{id}")["parameters"]
            .as_array()
            .unwrap();
        assert_eq!(params.len(), 1, "{method}");
        let id = &params[0];
        assert_eq!(id["name"], "id", "{method}");
        assert_eq!(id["in"], "path", "{method}");
        assert_eq!(id["required"], true, "{method}");
        assert_eq!(id["schema"]["type"], "string", "{method}");
        assert_eq!(id["schema"]["format"], "uuid", "{method}");
    }
}

// ------------------------------------------------------------------ Examples

#[tokio::test]
async fn every_operation_has_an_example() {
    let spec = spec().await;
    for (method, path, code) in [
        ("get", "/health", "200"),
        ("post", "/tasks", "201"),
        ("get", "/tasks", "200"),
        ("get", "/tasks/{id}", "200"),
        ("patch", "/tasks/{id}", "200"),
        ("delete", "/tasks/{id}", "404"),
    ] {
        let media = json_media(&op(&spec, method, path)["responses"][code]);
        assert!(carried_example(media).is_some(), "{method} {path} {code}");
    }
}

#[tokio::test]
async fn request_examples() {
    let spec = spec().await;
    let post = carried_example(json_media(&op(&spec, "post", "/tasks")["requestBody"])).unwrap();
    assert_eq!(
        post,
        json!({"title": "Write PRD", "priority": "high", "due_date": "2026-12-01"})
    );
    let patch = carried_example(json_media(
        &op(&spec, "patch", "/tasks/{id}")["requestBody"],
    ))
    .unwrap();
    let fields = ["title", "description", "status", "priority", "due_date"];
    assert!(
        patch
            .as_object()
            .unwrap()
            .keys()
            .any(|k| fields.contains(&k.as_str())),
        "{patch}"
    );
}

#[tokio::test]
async fn examples_validate_against_schemas() {
    let spec = spec().await;
    let mut checked = 0;
    for (path, item) in spec["paths"].as_object().unwrap() {
        for method in METHODS {
            let Some(operation) = item.get(method) else {
                continue;
            };
            let mut media_types: Vec<(String, &Value)> = Vec::new();
            if let Some(body) = operation.get("requestBody") {
                for (kind, media) in body["content"].as_object().unwrap() {
                    media_types.push((format!("request {kind}"), media));
                }
            }
            for (code, response) in operation["responses"].as_object().unwrap() {
                if let Some(content) = response.get("content") {
                    for (kind, media) in content.as_object().unwrap() {
                        media_types.push((format!("{code} {kind}"), media));
                    }
                }
            }
            for (label, media) in media_types {
                if let Some(example) = carried_example(media) {
                    if let Err(errors) = validate_with_schema(&spec, &media["schema"], &example) {
                        panic!("{method} {path} {label}: {errors:#?}");
                    }
                    checked += 1;
                }
            }
            for param in operation["parameters"].as_array().into_iter().flatten() {
                if let Some(example) = carried_example(param) {
                    if let Err(errors) = validate_with_schema(&spec, &param["schema"], &example) {
                        panic!("{method} {path} parameter {}: {errors:#?}", param["name"]);
                    }
                    checked += 1;
                }
            }
        }
    }
    assert!(checked >= 10, "only {checked} examples found");
}

#[tokio::test]
async fn validator_helper_rejects_an_invalid_instance() {
    let spec = spec().await;
    let schema = json!({"$ref": "#/components/schemas/Task"});
    assert!(validate_with_schema(&spec, &schema, &json!({"id": 5})).is_err());
    assert!(resolve(&spec, "#/components/schemas/Task").is_object());
}

// ---------------------------------------------------------------- Swagger UI

#[tokio::test]
async fn docs_returns_html_without_redirect() {
    let reply = send(&router(false), get("/docs")).await;
    assert_eq!(reply.status.as_u16(), 200, "body: {}", reply.body);
    assert!(reply.headers.get("location").is_none());
    assert!(
        header(&reply, "content-type")
            .unwrap()
            .starts_with("text/html")
    );
    assert!(reply.body.contains(r#"id="swagger-ui""#), "{}", reply.body);
    assert_request_id(&reply);
}

#[tokio::test]
async fn docs_slash_returns_html() {
    let router = router(false);
    let docs = send(&router, get("/docs")).await;
    let slash = send(&router, get("/docs/")).await;
    assert_eq!(slash.status.as_u16(), 200);
    assert!(
        header(&slash, "content-type")
            .unwrap()
            .starts_with("text/html")
    );
    assert_eq!(slash.body, docs.body);
}

fn is_javascript(content_type: &str) -> bool {
    let media = content_type.split(';').next().unwrap().trim();
    media == "application/javascript" || media == "text/javascript"
}

#[tokio::test]
async fn initializer_points_to_openapi_json_with_try_it_out() {
    let reply = send(&router(false), get("/docs/swagger-initializer.js")).await;
    assert_eq!(reply.status.as_u16(), 200);
    assert!(is_javascript(header(&reply, "content-type").unwrap()));
    assert!(
        reply.body.contains(r#""url": "/openapi.json""#),
        "{}",
        reply.body
    );
    assert!(
        reply.body.contains(r#""tryItOutEnabled": true"#),
        "{}",
        reply.body
    );
}

/// The values of `attribute` on every `<tag …>` element whose text matches
/// `filter`.
fn attribute_values(html: &str, tag: &str, attribute: &str, filter: &str) -> Vec<String> {
    let mut values = Vec::new();
    for chunk in html.split(&format!("<{tag}")).skip(1) {
        let element = chunk.split('>').next().unwrap();
        if !element.contains(filter) {
            continue;
        }
        let marker = format!(r#"{attribute}=""#);
        if let Some(start) = element.find(&marker) {
            let rest = &element[start + marker.len()..];
            values.push(rest[..rest.find('"').unwrap()].to_owned());
        }
    }
    values
}

/// Resolves a relative URL against the page's `<base href>` (or `/docs`).
fn resolve_url(base: &str, url: &str) -> String {
    if url.starts_with('/') {
        return url.to_owned();
    }
    let directory = &base[..=base.rfind('/').unwrap()];
    format!("{directory}{}", url.trim_start_matches("./"))
}

#[tokio::test]
async fn referenced_assets_are_served() {
    let router = router(false);
    let html = send(&router, get("/docs")).await.body;
    let base = attribute_values(&html, "base", "href", "")
        .into_iter()
        .next()
        .unwrap_or_else(|| "/docs".to_owned());
    let stylesheets = attribute_values(&html, "link", "href", r#"rel="stylesheet""#);
    let scripts = attribute_values(&html, "script", "src", "");
    assert!(!stylesheets.is_empty() && !scripts.is_empty(), "{html}");

    let mut script_paths = Vec::new();
    for (urls, is_style) in [(&stylesheets, true), (&scripts, false)] {
        for url in urls {
            let path = resolve_url(&base, url);
            assert!(path.starts_with("/docs/"), "{url} resolves to {path}");
            let reply = send(&router, get(&path)).await;
            assert_eq!(reply.status.as_u16(), 200, "{path}");
            let content_type = header(&reply, "content-type").unwrap();
            if is_style {
                assert!(
                    content_type.starts_with("text/css"),
                    "{path}: {content_type}"
                );
            } else {
                assert!(is_javascript(content_type), "{path}: {content_type}");
                script_paths.push(path);
            }
        }
    }
    assert!(script_paths.contains(&"/docs/swagger-initializer.js".to_owned()));
}

#[tokio::test]
async fn unknown_docs_asset_is_404() {
    let reply = send(&router(false), get("/docs/does-not-exist.js")).await;
    assert_uniform_error(&reply, 404, "not_found", "Route not found");
    assert_request_id(&reply);
}

#[tokio::test]
async fn docs_routes_reject_post() {
    let router = router(false);
    for path in ["/docs", "/docs/", "/docs/swagger-ui.css"] {
        let reply = send(
            &router,
            request_with("POST", path, Some("application/json"), "{}"),
        )
        .await;
        assert_uniform_error(&reply, 405, "method_not_allowed", "Method not allowed");
    }
}
