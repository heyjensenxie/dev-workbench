CREATE TABLE IF NOT EXISTS database_connections (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    type TEXT NOT NULL CHECK(type IN ('mysql', 'sqlite', 'postgresql')),
    host TEXT,
    port INTEGER,
    username TEXT,
    database_name TEXT,
    sqlite_path TEXT,
    secret_ref TEXT,
    options_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_database_connections_project_id
    ON database_connections(project_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS database_query_history (
    id TEXT PRIMARY KEY NOT NULL,
    connection_id TEXT NOT NULL REFERENCES database_connections(id) ON DELETE CASCADE,
    database_name TEXT,
    sql_text TEXT NOT NULL,
    success INTEGER NOT NULL DEFAULT 0,
    execution_time INTEGER NOT NULL DEFAULT 0,
    executed_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_database_query_history_executed_at
    ON database_query_history(connection_id, executed_at DESC);
