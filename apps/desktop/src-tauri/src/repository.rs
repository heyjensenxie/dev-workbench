//! SQLite-backed persistence for non-secret workbench data.
//!
//! Migrations are applied once each and tracked in `_migrations`, so a release
//! can add schema without dropping user data.

use crate::{
    error::AppError,
    models::{ApiModule, DevService, Project, SavedApiRequest, now_millis},
};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{path::Path, time::Duration};

/// Ordered schema migrations. Each entry runs at most once per database.
const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_initial",
        include_str!("../migrations/0001_initial.sql"),
    ),
    (
        "0002_service_metadata",
        include_str!("../migrations/0002_service_metadata.sql"),
    ),
    (
        "0003_api_requests",
        include_str!("../migrations/0003_api_requests.sql"),
    ),
    (
        "0004_api_modules",
        include_str!("../migrations/0004_api_modules.sql"),
    ),
    (
        "0005_database_workbench",
        include_str!("../migrations/0005_database_workbench.sql"),
    ),
];

pub async fn connect(path: &Path) -> Result<SqlitePool, AppError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        // Cascading deletes and the service/telemetry joins rely on this.
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    migrate(&pool).await?;
    Ok(pool)
}

async fn migrate(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS _migrations (name TEXT PRIMARY KEY NOT NULL, applied_at INTEGER NOT NULL);",
    )
    .execute(pool)
    .await?;
    let applied: Vec<String> = sqlx::query_scalar("SELECT name FROM _migrations")
        .fetch_all(pool)
        .await?;
    for (name, sql) in MIGRATIONS {
        if applied.iter().any(|entry| entry == name) {
            continue;
        }
        let mut transaction = pool.begin().await?;
        sqlx::raw_sql(sql).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO _migrations(name, applied_at) VALUES(?,?)")
            .bind(name)
            .bind(now_millis())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
    }
    Ok(())
}

pub async fn list_projects(pool: &SqlitePool) -> Result<Vec<Project>, AppError> {
    Ok(sqlx::query_as::<_, Project>("SELECT id,name,path,created_at,updated_at,last_opened_at FROM projects ORDER BY COALESCE(last_opened_at, updated_at) DESC").fetch_all(pool).await?)
}

pub async fn get_project(pool: &SqlitePool, project_id: &str) -> Result<Option<Project>, AppError> {
    Ok(sqlx::query_as::<_, Project>(
        "SELECT id,name,path,created_at,updated_at,last_opened_at FROM projects WHERE id=?",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?)
}

pub async fn upsert_project(pool: &SqlitePool, project: &Project) -> Result<(), AppError> {
    sqlx::query("INSERT INTO projects(id,name,path,created_at,updated_at,last_opened_at) VALUES(?,?,?,?,?,?) ON CONFLICT(path) DO UPDATE SET name=excluded.name,updated_at=excluded.updated_at,last_opened_at=excluded.last_opened_at")
        .bind(&project.id).bind(&project.name).bind(&project.path).bind(project.created_at).bind(project.updated_at).bind(project.last_opened_at).execute(pool).await?;
    Ok(())
}

/// Deletes only the local workbench record; the real project directory is never touched.
pub async fn delete_project(pool: &SqlitePool, project_id: &str) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM projects WHERE id=?")
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_services(
    pool: &SqlitePool,
    project_id: &str,
) -> Result<Vec<DevService>, AppError> {
    let rows = sqlx::query("SELECT id,project_id,name,cwd,command,args_json,env_json,port,auto_open,dependencies_json,updated_at FROM project_services WHERE project_id=? ORDER BY name COLLATE NOCASE")
        .bind(project_id)
        .fetch_all(pool)
        .await?;
    rows.iter().map(service_from_row).collect()
}

pub async fn get_service(
    pool: &SqlitePool,
    service_id: &str,
) -> Result<Option<DevService>, AppError> {
    let row = sqlx::query("SELECT id,project_id,name,cwd,command,args_json,env_json,port,auto_open,dependencies_json,updated_at FROM project_services WHERE id=?")
        .bind(service_id)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(service_from_row).transpose()
}

/// Inserts or updates a service after validating it.
pub async fn save_service(pool: &SqlitePool, service: &DevService) -> Result<(), AppError> {
    service.validate().map_err(AppError::Validation)?;
    ensure_unique_name(pool, service).await?;
    sqlx::query("INSERT INTO project_services(id,project_id,name,cwd,command,args_json,env_json,port,auto_open,dependencies_json,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,cwd=excluded.cwd,command=excluded.command,args_json=excluded.args_json,env_json=excluded.env_json,port=excluded.port,auto_open=excluded.auto_open,dependencies_json=excluded.dependencies_json,updated_at=excluded.updated_at")
      .bind(&service.id)
      .bind(&service.project_id)
      .bind(service.name.trim())
      .bind(service.cwd.as_deref().map(str::trim).filter(|cwd| !cwd.is_empty()))
      .bind(service.command.trim())
      .bind(encode(&service.args, "[]"))
      .bind(encode(&service.env, "{}"))
      .bind(service.port.map(i64::from))
      .bind(i64::from(service.auto_open))
      .bind(encode(&service.dependencies, "[]"))
      .bind(if service.updated_at > 0 { service.updated_at } else { now_millis() })
      .execute(pool)
      .await?;
    Ok(())
}

/// Deletes a service and reports whether a row was removed.
pub async fn delete_service(pool: &SqlitePool, service_id: &str) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM project_services WHERE id=?")
        .bind(service_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Lists API modules, newest first.
pub async fn list_api_modules(pool: &SqlitePool) -> Result<Vec<ApiModule>, AppError> {
    Ok(sqlx::query_as::<_, ApiModule>(
        "SELECT id,name,description,created_at,updated_at FROM api_modules \
         ORDER BY updated_at DESC, name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?)
}

/// Inserts or updates an API module.
pub async fn save_api_module(pool: &SqlitePool, module: &ApiModule) -> Result<ApiModule, AppError> {
    let id = module.id.trim();
    let name = module.name.trim();
    if id.is_empty() || id.chars().count() > 100 {
        return Err(AppError::Validation(
            "API module id must be 1-100 characters".into(),
        ));
    }
    if name.is_empty() || name.chars().count() > 100 {
        return Err(AppError::Validation(
            "API module name must be 1-100 characters".into(),
        ));
    }
    let now = now_millis();
    let stored = ApiModule {
        id: id.to_owned(),
        name: name.to_owned(),
        description: module
            .description
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        created_at: if module.created_at > 0 {
            module.created_at
        } else {
            now
        },
        updated_at: if module.updated_at > 0 {
            module.updated_at
        } else {
            now
        },
    };
    sqlx::query(
        "INSERT INTO api_modules(id,name,description,created_at,updated_at) VALUES(?,?,?,?,?) \
         ON CONFLICT(id) DO UPDATE SET name=excluded.name,description=excluded.description,updated_at=excluded.updated_at",
    )
    .bind(&stored.id)
    .bind(&stored.name)
    .bind(&stored.description)
    .bind(stored.created_at)
    .bind(stored.updated_at)
    .execute(pool)
    .await?;
    Ok(stored)
}

/// Deletes a module; its requests become ungrouped through ON DELETE SET NULL.
pub async fn delete_api_module(pool: &SqlitePool, module_id: &str) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM api_modules WHERE id=?")
        .bind(module_id.trim())
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Lists saved API request templates, newest first, optionally scoped to a module.
pub async fn list_api_requests(
    pool: &SqlitePool,
    module_id: Option<&str>,
) -> Result<Vec<SavedApiRequest>, AppError> {
    let rows = sqlx::query(
        "SELECT id,name,method,url,module_id,headers_json,body,created_at,updated_at \
         FROM api_requests WHERE (? IS NULL OR module_id=?) \
         ORDER BY updated_at DESC, name COLLATE NOCASE",
    )
    .bind(module_id)
    .bind(module_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(api_request_from_row).collect()
}

/// Inserts or updates a request template after normalizing and scrubbing it.
pub async fn save_api_request(
    pool: &SqlitePool,
    request: &SavedApiRequest,
) -> Result<SavedApiRequest, AppError> {
    let sanitized = sanitize_api_request(request)?;
    sqlx::query(
        "INSERT INTO api_requests(id,name,method,url,module_id,headers_json,body,created_at,updated_at) \
         VALUES(?,?,?,?,?,?,?,?,?) \
         ON CONFLICT(id) DO UPDATE SET name=excluded.name,method=excluded.method,url=excluded.url,module_id=excluded.module_id,\
         headers_json=excluded.headers_json,body=excluded.body,updated_at=excluded.updated_at",
    )
    .bind(&sanitized.id)
    .bind(&sanitized.name)
    .bind(&sanitized.method)
    .bind(&sanitized.url)
    .bind(&sanitized.module_id)
    .bind(encode(&sanitized.headers, "{}"))
    .bind(&sanitized.body)
    .bind(sanitized.created_at)
    .bind(sanitized.updated_at)
    .execute(pool)
    .await?;
    Ok(sanitized)
}

/// Deletes one saved API request and reports whether a row was removed.
pub async fn delete_api_request(pool: &SqlitePool, request_id: &str) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM api_requests WHERE id=?")
        .bind(request_id.trim())
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Removes every dependency edge pointing at `service_id` from its siblings.
pub async fn forget_service_dependencies(
    pool: &SqlitePool,
    project_id: &str,
    service_id: &str,
) -> Result<u32, AppError> {
    let services = list_services(pool, project_id).await?;
    let mut updated = 0;
    for service in services {
        if !service.dependencies.iter().any(|id| id == service_id) {
            continue;
        }
        let mut pruned = service.clone();
        pruned.dependencies.retain(|id| id != service_id);
        save_service(pool, &pruned).await?;
        updated += 1;
    }
    Ok(updated)
}

async fn ensure_unique_name(pool: &SqlitePool, service: &DevService) -> Result<(), AppError> {
    let conflict: Option<String> = sqlx::query_scalar(
        "SELECT id FROM project_services WHERE project_id=? AND id<>? AND name=? COLLATE NOCASE",
    )
    .bind(&service.project_id)
    .bind(&service.id)
    .bind(service.name.trim())
    .fetch_optional(pool)
    .await?;
    if conflict.is_some() {
        return Err(AppError::Validation(format!(
            "a service named \"{}\" already exists in this project",
            service.name.trim()
        )));
    }
    Ok(())
}

/// Reads every persisted setting as a JSON value.
pub async fn load_settings(
    pool: &SqlitePool,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>, AppError> {
    let rows = sqlx::query("SELECT key, value_json FROM settings")
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let key: String = row.get("key");
            let raw: String = row.get("value_json");
            serde_json::from_str(&raw).ok().map(|value| (key, value))
        })
        .collect())
}

/// Writes one setting, keyed by name and stored as JSON so values can grow.
pub async fn save_setting(
    pool: &SqlitePool,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), AppError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(AppError::Validation("setting key must not be empty".into()));
    }
    if key.chars().count() > 64 {
        return Err(AppError::Validation(
            "setting key must be 64 characters or fewer".into(),
        ));
    }
    if key.chars().any(|character| character.is_whitespace()) {
        return Err(AppError::Validation(
            "setting key must not contain whitespace".into(),
        ));
    }
    sqlx::query(
        "INSERT INTO settings(key,value_json,updated_at) VALUES(?,?,?) \
         ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at",
    )
    .bind(key)
    .bind(encode(value, "null"))
    .bind(now_millis())
    .execute(pool)
    .await?;
    Ok(())
}

fn service_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<DevService, AppError> {
    Ok(DevService {
        id: row.get("id"),
        project_id: row.get("project_id"),
        name: row.get("name"),
        cwd: row.get("cwd"),
        command: row.get("command"),
        args: decode(row.get("args_json"), "[]"),
        env: decode(row.get("env_json"), "{}"),
        port: row.get::<Option<i64>, _>("port").map(|value| value as u16),
        auto_open: row.get::<i64, _>("auto_open") != 0,
        dependencies: decode(row.get("dependencies_json"), "[]"),
        updated_at: row.get("updated_at"),
    })
}

fn api_request_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<SavedApiRequest, AppError> {
    Ok(SavedApiRequest {
        id: row.get("id"),
        name: row.get("name"),
        method: row.get("method"),
        url: row.get("url"),
        module_id: row.get("module_id"),
        headers: decode(row.get("headers_json"), "{}"),
        body: row.get("body"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn sanitize_api_request(request: &SavedApiRequest) -> Result<SavedApiRequest, AppError> {
    let id = request.id.trim();
    let name = request.name.trim();
    let method = request.method.trim().to_uppercase();
    let url = request.url.trim();
    if id.is_empty() || id.chars().count() > 100 {
        return Err(AppError::Validation(
            "API request id must be 1-100 characters".into(),
        ));
    }
    if name.is_empty() || name.chars().count() > 120 {
        return Err(AppError::Validation(
            "API request name must be 1-120 characters".into(),
        ));
    }
    if !matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
    ) {
        return Err(AppError::Validation("unsupported HTTP method".into()));
    }
    if url.is_empty() || url.chars().count() > 4096 {
        return Err(AppError::Validation(
            "API request URL must be 1-4096 characters".into(),
        ));
    }
    let headers = request
        .headers
        .iter()
        .filter_map(|(key, value)| {
            let key = key.trim();
            if key.is_empty() {
                return None;
            }
            let stored = if is_sensitive_name(key) && !is_template(value) {
                "{{SECRET}}".to_owned()
            } else {
                value.trim().to_owned()
            };
            Some((key.to_owned(), stored))
        })
        .collect();
    let body = request
        .body
        .as_deref()
        .map(sanitize_body)
        .filter(|body| !body.is_empty());
    if body
        .as_deref()
        .map_or(false, |value| value.len() > 5 * 1024 * 1024)
    {
        return Err(AppError::Validation(
            "API request body must be 5 MB or smaller".into(),
        ));
    }
    let now = now_millis();
    Ok(SavedApiRequest {
        id: id.to_owned(),
        name: name.to_owned(),
        method,
        url: sanitize_url(url),
        module_id: request
            .module_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        headers,
        body,
        created_at: if request.created_at > 0 {
            request.created_at
        } else {
            now
        },
        updated_at: if request.updated_at > 0 {
            request.updated_at
        } else {
            now
        },
    })
}

fn is_sensitive_name(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase().replace(['-', '_'], "");
    normalized.contains("token")
        || normalized.contains("secret")
        || normalized.contains("password")
        || normalized.contains("passwd")
        || normalized.contains("apikey")
        || normalized == "authorization"
        || normalized == "cookie"
        || normalized == "setcookie"
}

fn sanitize_url(value: &str) -> String {
    if let Ok(mut parsed) = reqwest::Url::parse(value) {
        parsed.set_username("").ok();
        parsed.set_password(None).ok();
        let pairs: Vec<(String, String)> = parsed
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        if !pairs.is_empty() {
            parsed.set_query(None);
            let mut query = parsed.query_pairs_mut();
            for (key, value) in pairs {
                query.append_pair(
                    &key,
                    if is_sensitive_name(&key) && !is_template(&value) {
                        "{{SECRET}}"
                    } else {
                        &value
                    },
                );
            }
        }
        return parsed.to_string();
    }
    let Some((base, query)) = value.split_once('?') else {
        return value.to_owned();
    };
    let sanitized = query
        .split('&')
        .map(|part| {
            let Some((key, value)) = part.split_once('=') else {
                return part.to_owned();
            };
            format!(
                "{key}={}",
                if is_sensitive_name(key) {
                    "{{SECRET}}"
                } else {
                    value
                }
            )
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{sanitized}")
}

fn sanitize_body(value: &str) -> String {
    if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(value) {
        scrub_json(&mut json);
        return serde_json::to_string_pretty(&json).unwrap_or_else(|_| value.to_owned());
    }
    value
        .split('&')
        .map(|part| {
            let Some((key, item_value)) = part.split_once('=') else {
                return part.to_owned();
            };
            format!(
                "{key}={}",
                if is_sensitive_name(key) && !is_template(item_value) {
                    "{{SECRET}}"
                } else {
                    item_value
                }
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn is_template(value: &str) -> bool {
    value.contains("{{") && value.contains("}}")
}

fn scrub_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                if is_sensitive_name(key) && !value.as_str().is_some_and(is_template) {
                    *value = serde_json::Value::String("{{SECRET}}".into());
                } else {
                    scrub_json(value);
                }
            }
        }
        serde_json::Value::Array(values) => values.iter_mut().for_each(scrub_json),
        _ => {}
    }
}

fn encode<T: serde::Serialize>(value: &T, fallback: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| fallback.to_owned())
}

fn decode<T: serde::de::DeserializeOwned + Default>(value: String, fallback: &str) -> T {
    serde_json::from_str(&value)
        .or_else(|_| serde_json::from_str(fallback))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct TempDatabase {
        path: std::path::PathBuf,
    }

    impl TempDatabase {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "dev-workbench-repository-{name}-{}.sqlite3",
                std::process::id()
            ));
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
            }
            Self { path }
        }
    }

    impl Drop for TempDatabase {
        fn drop(&mut self) {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", self.path.display()));
            }
        }
    }

    fn service(project_id: &str, id: &str, name: &str) -> DevService {
        DevService {
            id: id.into(),
            project_id: project_id.into(),
            name: name.into(),
            cwd: None,
            command: "pnpm".into(),
            args: vec!["dev".into()],
            env: HashMap::new(),
            port: Some(5173),
            auto_open: true,
            dependencies: Vec::new(),
            updated_at: 0,
        }
    }

    async fn project(pool: &SqlitePool) -> Project {
        let project = Project {
            id: "project-1".into(),
            name: "demo".into(),
            path: std::env::temp_dir().to_string_lossy().into_owned(),
            created_at: 1,
            updated_at: 1,
            last_opened_at: Some(1),
        };
        upsert_project(pool, &project).await.unwrap();
        project
    }

    #[tokio::test]
    async fn migrates_once_and_stays_idempotent() {
        let database = TempDatabase::new("migrate");
        let pool = connect(&database.path).await.unwrap();
        drop(pool);
        // A second connection must not re-apply migrations or fail on
        // non-idempotent statements such as ALTER TABLE.
        let pool = connect(&database.path).await.unwrap();
        let applied: Vec<String> = sqlx::query_scalar("SELECT name FROM _migrations ORDER BY name")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(
            applied,
            vec![
                "0001_initial",
                "0002_service_metadata",
                "0003_api_requests",
                "0004_api_modules"
            ]
        );
    }

    #[tokio::test]
    async fn saves_lists_and_deletes_a_service() {
        let database = TempDatabase::new("crud");
        let pool = connect(&database.path).await.unwrap();
        let project = project(&pool).await;

        let mut api = service(&project.id, "service-1", "api");
        api.env.insert("NODE_ENV".into(), "development".into());
        api.dependencies = vec!["service-2".into()];
        save_service(&pool, &api).await.unwrap();

        let stored = list_services(&pool, &project.id).await.unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0].env.get("NODE_ENV").map(String::as_str),
            Some("development")
        );
        assert_eq!(stored[0].dependencies, vec!["service-2"]);
        assert!(stored[0].updated_at > 0);

        assert!(get_service(&pool, "service-1").await.unwrap().is_some());
        assert!(delete_service(&pool, "service-1").await.unwrap());
        assert!(!delete_service(&pool, "service-1").await.unwrap());
        assert!(get_service(&pool, "service-1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn saves_api_requests_without_persisting_obvious_secrets() {
        let database = TempDatabase::new("api-request");
        let pool = connect(&database.path).await.unwrap();
        let request = SavedApiRequest {
            id: "request-1".into(),
            name: "Create user".into(),
            method: "post".into(),
            url: "https://user:password@example.com/users?token=real-token&active=true".into(),
            module_id: None,
            headers: [
                ("Authorization".into(), "Bearer real-token".into()),
                ("Accept".into(), "application/json".into()),
            ]
            .into_iter()
            .collect(),
            body: Some(
                r#"{"username":"demo","password":"real-password","profile":{"name":"Jensen"}}"#
                    .into(),
            ),
            created_at: 0,
            updated_at: 0,
        };

        let stored = save_api_request(&pool, &request).await.unwrap();
        assert_eq!(stored.method, "POST");
        assert!(stored.url.contains("token=%7B%7BSECRET%7D%7D"));
        assert!(!stored.url.contains("real-token"));
        assert_eq!(stored.headers["Authorization"], "{{SECRET}}");
        assert!(!stored.body.as_deref().unwrap().contains("real-password"));
        assert!(stored.body.as_deref().unwrap().contains("{{SECRET}}"));

        let module = ApiModule {
            id: "module-users".into(),
            name: "Users".into(),
            description: None,
            created_at: 0,
            updated_at: 0,
        };
        save_api_module(&pool, &module).await.unwrap();
        let mut grouped = request.clone();
        grouped.module_id = Some(module.id.clone());
        save_api_request(&pool, &grouped).await.unwrap();
        let listed = list_api_requests(&pool, Some(&module.id)).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(delete_api_module(&pool, &module.id).await.unwrap());
        assert!(
            list_api_requests(&pool, Some(&module.id))
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            list_api_requests(&pool, None)
                .await
                .unwrap()
                .iter()
                .all(|item| item.module_id.is_none())
        );
        assert!(delete_api_request(&pool, "request-1").await.unwrap());
        assert!(list_api_requests(&pool, None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn validates_invalid_services_and_accepts_relative_directories() {
        let database = TempDatabase::new("invalid");
        let pool = connect(&database.path).await.unwrap();
        let project = project(&pool).await;

        let mut blank_name = service(&project.id, "service-1", "   ");
        assert!(matches!(
            save_service(&pool, &blank_name).await,
            Err(AppError::Validation(_))
        ));

        blank_name.name = "api".into();
        blank_name.command = "  ".into();
        assert!(matches!(
            save_service(&pool, &blank_name).await,
            Err(AppError::Validation(_))
        ));

        blank_name.command = "pnpm".into();
        blank_name.cwd = Some("relative/path".into());
        assert!(matches!(save_service(&pool, &blank_name).await, Ok(())));
        assert_eq!(
            get_service(&pool, "service-1")
                .await
                .unwrap()
                .and_then(|stored| stored.cwd),
            Some("relative/path".into())
        );

        blank_name.cwd = None;
        blank_name.dependencies = vec!["service-1".into()];
        assert!(matches!(
            save_service(&pool, &blank_name).await,
            Err(AppError::Validation(_))
        ));
    }

    #[tokio::test]
    async fn rejects_duplicate_service_names_per_project() {
        let database = TempDatabase::new("duplicate");
        let pool = connect(&database.path).await.unwrap();
        let project = project(&pool).await;
        save_service(&pool, &service(&project.id, "service-1", "api"))
            .await
            .unwrap();
        assert!(matches!(
            save_service(&pool, &service(&project.id, "service-2", "API")).await,
            Err(AppError::Validation(_))
        ));
        // Updating the same service keeps its own name.
        save_service(&pool, &service(&project.id, "service-1", "api"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn deleting_a_service_prunes_sibling_dependencies() {
        let database = TempDatabase::new("prune");
        let pool = connect(&database.path).await.unwrap();
        let project = project(&pool).await;
        let mut web = service(&project.id, "service-1", "web");
        web.dependencies = vec!["service-2".into(), "service-3".into()];
        save_service(&pool, &web).await.unwrap();

        assert_eq!(
            forget_service_dependencies(&pool, &project.id, "service-2")
                .await
                .unwrap(),
            1
        );
        let stored = get_service(&pool, "service-1").await.unwrap().unwrap();
        assert_eq!(stored.dependencies, vec!["service-3"]);
    }

    #[tokio::test]
    async fn deletes_services_with_their_project() {
        let database = TempDatabase::new("cascade");
        let pool = connect(&database.path).await.unwrap();
        let project = project(&pool).await;
        save_service(&pool, &service(&project.id, "service-1", "api"))
            .await
            .unwrap();

        sqlx::query("DELETE FROM projects WHERE id=?")
            .bind(&project.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(list_services(&pool, &project.id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn deletes_only_the_project_record() {
        let database = TempDatabase::new("project-delete");
        let pool = connect(&database.path).await.unwrap();
        let project = project(&pool).await;
        save_service(&pool, &service(&project.id, "service-1", "api"))
            .await
            .unwrap();

        assert!(delete_project(&pool, &project.id).await.unwrap());
        assert!(!delete_project(&pool, &project.id).await.unwrap());
        assert!(list_projects(&pool).await.unwrap().is_empty());
        assert!(list_services(&pool, &project.id).await.unwrap().is_empty());
        assert!(database.path.exists());
    }

    #[tokio::test]
    async fn stores_settings_as_json_values() {
        let database = TempDatabase::new("settings");
        let pool = connect(&database.path).await.unwrap();
        assert!(load_settings(&pool).await.unwrap().is_empty());

        save_setting(&pool, "theme", &serde_json::json!("dark"))
            .await
            .unwrap();
        save_setting(&pool, "logLimit", &serde_json::json!(500))
            .await
            .unwrap();
        save_setting(&pool, "theme", &serde_json::json!("light"))
            .await
            .unwrap();

        let settings = load_settings(&pool).await.unwrap();
        assert_eq!(settings.get("theme"), Some(&serde_json::json!("light")));
        assert_eq!(settings.get("logLimit"), Some(&serde_json::json!(500)));
    }

    #[tokio::test]
    async fn rejects_unusable_setting_keys() {
        let database = TempDatabase::new("settings-invalid");
        let pool = connect(&database.path).await.unwrap();
        for key in ["", "   ", "log limit", &"k".repeat(65)] {
            assert!(
                matches!(
                    save_setting(&pool, key, &serde_json::json!(true)).await,
                    Err(AppError::Validation(_))
                ),
                "expected {key:?} to be rejected"
            );
        }
    }

    #[tokio::test]
    async fn settings_survive_a_reconnect() {
        let database = TempDatabase::new("settings-persist");
        let pool = connect(&database.path).await.unwrap();
        save_setting(&pool, "locale", &serde_json::json!("zh-CN"))
            .await
            .unwrap();
        drop(pool);

        let pool = connect(&database.path).await.unwrap();
        assert_eq!(
            load_settings(&pool).await.unwrap().get("locale"),
            Some(&serde_json::json!("zh-CN"))
        );
    }
}
