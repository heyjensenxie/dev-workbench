CREATE TABLE IF NOT EXISTS api_requests (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    method TEXT NOT NULL,
    url TEXT NOT NULL,
    headers_json TEXT NOT NULL DEFAULT '{}',
    body TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_api_requests_updated_at
    ON api_requests(updated_at DESC, name COLLATE NOCASE);
