PRAGMA foreign_keys = ON;

ALTER TABLE project_services ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_project_services_project_name ON project_services(project_id, name);
