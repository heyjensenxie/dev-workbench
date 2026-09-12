CREATE TABLE IF NOT EXISTS api_modules (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

ALTER TABLE api_requests ADD COLUMN module_id TEXT REFERENCES api_modules(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_api_modules_updated_at
    ON api_modules(updated_at DESC, name COLLATE NOCASE);

CREATE INDEX IF NOT EXISTS idx_api_requests_module_id
    ON api_requests(module_id, updated_at DESC, name COLLATE NOCASE);
