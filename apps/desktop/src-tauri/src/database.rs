use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{
    Column, Row, TypeInfo,
    mysql::MySqlRow,
    mysql::{MySqlConnectOptions, MySqlPoolOptions},
    sqlite::SqliteRow,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT_MS: u64 = 5_000;

/// Locale-neutral codes for wording this runtime produces itself. The interface
/// translates them into the active language, so the native layer stays
/// language-agnostic; engine and driver errors keep their original text.
mod message {
    pub const CONNECTED_MYSQL: &str = "database.message.connectedMysql";
    pub const CONNECTED_SQLITE: &str = "database.message.connectedSqlite";
    pub const STATEMENT_EXECUTED: &str = "database.message.statementExecuted";
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseConnectionConfig {
    #[serde(rename = "type")]
    pub database_type: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub database: Option<String>,
    pub sqlite_path: Option<String>,
    pub connection_timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub success: bool,
    pub latency_ms: u64,
    pub server_version: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseInfo {
    pub name: String,
    pub database_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub table_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryColumn {
    pub name: String,
    #[serde(rename = "type")]
    pub column_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub columns: Vec<QueryColumn>,
    pub rows: Vec<BTreeMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affected_rows: Option<u64>,
    pub execution_time: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

fn timeout(config: &DatabaseConnectionConfig) -> Duration {
    Duration::from_millis(
        config
            .connection_timeout_ms
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .clamp(1_000, 120_000),
    )
}

fn mysql_options(config: &DatabaseConnectionConfig) -> Result<MySqlConnectOptions, String> {
    let host = config.host.as_deref().unwrap_or("localhost").trim();
    if host.is_empty() {
        return Err("Host is required".into());
    }
    let mut options = MySqlConnectOptions::new()
        .host(host)
        .port(config.port.unwrap_or(3306))
        .username(config.username.as_deref().unwrap_or("root"))
        .password(config.password.as_deref().unwrap_or_default());
    if let Some(database) = config
        .database
        .as_deref()
        .filter(|database| !database.trim().is_empty())
    {
        options = options.database(database);
    }
    Ok(options)
}

fn sqlite_options(config: &DatabaseConnectionConfig) -> Result<SqliteConnectOptions, String> {
    let path = config
        .sqlite_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .ok_or("SQLite file path is required")?;
    Ok(SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false))
}

fn quote_mysql_identifier(identifier: &str) -> String {
    format!("`{}`", identifier.replace('`', "``"))
}

pub async fn test_connection(
    config: DatabaseConnectionConfig,
) -> Result<ConnectionTestResult, crate::error::AppError> {
    let started = Instant::now();
    let timeout = timeout(&config);
    match config.database_type.as_str() {
        "mysql" => {
            let options = mysql_options(&config).map_err(crate::error::AppError::Validation)?;
            let pool = MySqlPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(timeout)
                .connect_with(options)
                .await?;
            let version: String = sqlx::query_scalar("SELECT VERSION()")
                .fetch_one(&pool)
                .await?;
            pool.close().await;
            Ok(ConnectionTestResult {
                success: true,
                latency_ms: started.elapsed().as_millis() as u64,
                server_version: Some(version),
                message: message::CONNECTED_MYSQL.into(),
            })
        }
        "sqlite" => {
            let options = sqlite_options(&config).map_err(crate::error::AppError::Validation)?;
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .acquire_timeout(timeout)
                .connect_with(options)
                .await?;
            let version: String = sqlx::query_scalar("SELECT sqlite_version()")
                .fetch_one(&pool)
                .await?;
            pool.close().await;
            Ok(ConnectionTestResult {
                success: true,
                latency_ms: started.elapsed().as_millis() as u64,
                server_version: Some(format!("SQLite {version}")),
                message: message::CONNECTED_SQLITE.into(),
            })
        }
        other => Err(crate::error::AppError::Validation(format!(
            "Database type is not supported by the native runtime: {other}"
        ))),
    }
}

pub async fn list_databases(
    config: DatabaseConnectionConfig,
) -> Result<Vec<DatabaseInfo>, crate::error::AppError> {
    match config.database_type.as_str() {
        "mysql" => {
            let options = mysql_options(&config).map_err(crate::error::AppError::Validation)?;
            let pool = MySqlPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(timeout(&config))
                .connect_with(options)
                .await?;
            let rows = sqlx::query(
                "SELECT SCHEMA_NAME FROM INFORMATION_SCHEMA.SCHEMATA ORDER BY SCHEMA_NAME",
            )
            .fetch_all(&pool)
            .await?;
            pool.close().await;
            Ok(rows
                .into_iter()
                .map(|row| DatabaseInfo {
                    name: row.get::<String, _>("SCHEMA_NAME"),
                    database_type: "MySQL".into(),
                })
                .collect())
        }
        "sqlite" => {
            let options = sqlite_options(&config).map_err(crate::error::AppError::Validation)?;
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .acquire_timeout(timeout(&config))
                .connect_with(options)
                .await?;
            let rows = sqlx::query("PRAGMA database_list").fetch_all(&pool).await?;
            pool.close().await;
            Ok(rows
                .into_iter()
                .map(|row| DatabaseInfo {
                    name: row.get::<String, _>("name"),
                    database_type: "SQLite".into(),
                })
                .collect())
        }
        other => Err(crate::error::AppError::Validation(format!(
            "Database type is not supported by the native runtime: {other}"
        ))),
    }
}

pub async fn list_tables(
    config: DatabaseConnectionConfig,
    database: String,
) -> Result<Vec<TableInfo>, crate::error::AppError> {
    match config.database_type.as_str() {
        "mysql" => {
            let options = mysql_options(&config).map_err(crate::error::AppError::Validation)?;
            let pool = MySqlPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(timeout(&config))
                .connect_with(options)
                .await?;
            let statement = format!(
                "SHOW FULL TABLES FROM {}",
                quote_mysql_identifier(&database)
            );
            let rows = sqlx::query(&statement).fetch_all(&pool).await?;
            pool.close().await;
            Ok(rows
                .into_iter()
                .map(|row| TableInfo {
                    name: row.get::<String, _>(0),
                    table_type: if row.get::<String, _>(1) == "VIEW" {
                        "view".into()
                    } else {
                        "table".into()
                    },
                    rows: None,
                })
                .collect())
        }
        "sqlite" => {
            let options = sqlite_options(&config).map_err(crate::error::AppError::Validation)?;
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .acquire_timeout(timeout(&config))
                .connect_with(options)
                .await?;
            let rows = sqlx::query("SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name").fetch_all(&pool).await?;
            pool.close().await;
            Ok(rows
                .into_iter()
                .map(|row| TableInfo {
                    name: row.get::<String, _>("name"),
                    table_type: row.get::<String, _>("type"),
                    rows: None,
                })
                .collect())
        }
        other => Err(crate::error::AppError::Validation(format!(
            "Database type is not supported by the native runtime: {other}"
        ))),
    }
}

fn is_read_query(sql: &str) -> bool {
    let normalized = sql.trim_start().to_ascii_lowercase();
    [
        "select ",
        "with ",
        "show ",
        "describe ",
        "desc ",
        "pragma ",
        "explain ",
    ]
    .iter()
    .any(|prefix| normalized.starts_with(prefix))
}

fn binary_value(bytes: Option<Vec<u8>>) -> Value {
    bytes.map_or(Value::Null, |bytes| {
        Value::String(format!(
            "0x{}",
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ))
    })
}

fn mysql_value(row: &MySqlRow, index: usize, type_name: &str) -> Value {
    match type_name {
        "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "INTEGER" | "BIGINT" => row
            .try_get::<Option<i64>, _>(index)
            .map(|value| value.map_or(Value::Null, |value| json!(value)))
            .unwrap_or_else(|_| Value::String("<unreadable>".into())),
        "FLOAT" | "DOUBLE" | "DECIMAL" => row
            .try_get::<Option<f64>, _>(index)
            .map(|value| value.map_or(Value::Null, |value| json!(value)))
            .unwrap_or_else(|_| Value::String("<unreadable>".into())),
        "DATE" => row
            .try_get::<Option<NaiveDate>, _>(index)
            .map(|value| value.map_or(Value::Null, |value| Value::String(value.to_string())))
            .unwrap_or_else(|_| Value::String("<unreadable>".into())),
        "DATETIME" => row
            .try_get::<Option<NaiveDateTime>, _>(index)
            .map(|value| value.map_or(Value::Null, |value| Value::String(value.to_string())))
            .unwrap_or_else(|_| Value::String("<unreadable>".into())),
        "TIMESTAMP" => row
            .try_get::<Option<DateTime<Utc>>, _>(index)
            .map(|value| {
                value.map_or(Value::Null, |value| {
                    Value::String(value.naive_utc().to_string())
                })
            })
            .unwrap_or_else(|_| Value::String("<unreadable>".into())),
        "TIME" => row
            .try_get::<Option<NaiveTime>, _>(index)
            .map(|value| value.map_or(Value::Null, |value| Value::String(value.to_string())))
            .unwrap_or_else(|_| Value::String("<unreadable>".into())),
        _ => match row.try_get::<Option<String>, _>(index) {
            Ok(value) => value.map_or(Value::Null, Value::String),
            Err(_) => row
                .try_get::<Option<Vec<u8>>, _>(index)
                .map(binary_value)
                .unwrap_or_else(|_| Value::String("<unreadable>".into())),
        },
    }
}

fn sqlite_value(row: &SqliteRow, index: usize, type_name: &str) -> Value {
    match type_name {
        "INTEGER" | "INT" => row
            .try_get::<Option<i64>, _>(index)
            .map(|value| value.map_or(Value::Null, |value| json!(value)))
            .unwrap_or_else(|_| Value::String("<unreadable>".into())),
        "REAL" | "FLOAT" | "DOUBLE" => row
            .try_get::<Option<f64>, _>(index)
            .map(|value| value.map_or(Value::Null, |value| json!(value)))
            .unwrap_or_else(|_| Value::String("<unreadable>".into())),
        "DATE" => row
            .try_get::<Option<NaiveDate>, _>(index)
            .map(|value| value.map_or(Value::Null, |value| Value::String(value.to_string())))
            .unwrap_or_else(|_| Value::String("<unreadable>".into())),
        "DATETIME" | "TIMESTAMP" => row
            .try_get::<Option<NaiveDateTime>, _>(index)
            .map(|value| value.map_or(Value::Null, |value| Value::String(value.to_string())))
            .unwrap_or_else(|_| Value::String("<unreadable>".into())),
        "TIME" => row
            .try_get::<Option<NaiveTime>, _>(index)
            .map(|value| value.map_or(Value::Null, |value| Value::String(value.to_string())))
            .unwrap_or_else(|_| Value::String("<unreadable>".into())),
        _ => match row.try_get::<Option<String>, _>(index) {
            Ok(value) => value.map_or(Value::Null, Value::String),
            Err(_) => row
                .try_get::<Option<Vec<u8>>, _>(index)
                .map(binary_value)
                .unwrap_or_else(|_| Value::String("<unreadable>".into())),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::binary_value;
    use serde_json::Value;

    #[test]
    fn preserves_binary_bytes_as_hex() {
        assert_eq!(
            binary_value(Some(vec![0x00, 0x2a, 0xff])),
            Value::String("0x002aff".into())
        );
        assert_eq!(binary_value(None), Value::Null);
    }
}

pub async fn query(
    config: DatabaseConnectionConfig,
    sql: String,
) -> Result<QueryResult, crate::error::AppError> {
    let started = Instant::now();
    let read = is_read_query(&sql);
    match config.database_type.as_str() {
        "mysql" => {
            let options = mysql_options(&config).map_err(crate::error::AppError::Validation)?;
            let pool = MySqlPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(timeout(&config))
                .connect_with(options)
                .await?;
            if read {
                let rows = sqlx::query(&sql).fetch_all(&pool).await?;
                let columns = rows.first().map_or_else(Vec::new, |row| {
                    row.columns()
                        .iter()
                        .map(|column| QueryColumn {
                            name: column.name().to_owned(),
                            column_type: column.type_info().name().to_owned(),
                        })
                        .collect()
                });
                let result_rows = rows
                    .iter()
                    .map(|row| {
                        columns
                            .iter()
                            .enumerate()
                            .map(|(index, column)| {
                                (
                                    column.name.clone(),
                                    mysql_value(row, index, &column.column_type),
                                )
                            })
                            .collect()
                    })
                    .collect();
                pool.close().await;
                Ok(QueryResult {
                    columns,
                    rows: result_rows,
                    affected_rows: None,
                    execution_time: started.elapsed().as_millis() as u64,
                    message: None,
                })
            } else {
                let affected_rows = sqlx::query(&sql).execute(&pool).await?.rows_affected();
                pool.close().await;
                Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    affected_rows: Some(affected_rows),
                    execution_time: started.elapsed().as_millis() as u64,
                    message: Some(message::STATEMENT_EXECUTED.into()),
                })
            }
        }
        "sqlite" => {
            let options = sqlite_options(&config).map_err(crate::error::AppError::Validation)?;
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .acquire_timeout(timeout(&config))
                .connect_with(options)
                .await?;
            if read {
                let rows = sqlx::query(&sql).fetch_all(&pool).await?;
                let columns = rows.first().map_or_else(Vec::new, |row| {
                    row.columns()
                        .iter()
                        .map(|column| QueryColumn {
                            name: column.name().to_owned(),
                            column_type: column.type_info().name().to_owned(),
                        })
                        .collect()
                });
                let result_rows = rows
                    .iter()
                    .map(|row| {
                        columns
                            .iter()
                            .enumerate()
                            .map(|(index, column)| {
                                (
                                    column.name.clone(),
                                    sqlite_value(row, index, &column.column_type),
                                )
                            })
                            .collect()
                    })
                    .collect();
                pool.close().await;
                Ok(QueryResult {
                    columns,
                    rows: result_rows,
                    affected_rows: None,
                    execution_time: started.elapsed().as_millis() as u64,
                    message: None,
                })
            } else {
                let affected_rows = sqlx::query(&sql).execute(&pool).await?.rows_affected();
                pool.close().await;
                Ok(QueryResult {
                    columns: Vec::new(),
                    rows: Vec::new(),
                    affected_rows: Some(affected_rows),
                    execution_time: started.elapsed().as_millis() as u64,
                    message: Some(message::STATEMENT_EXECUTED.into()),
                })
            }
        }
        other => Err(crate::error::AppError::Validation(format!(
            "Database type is not supported by the native runtime: {other}"
        ))),
    }
}
