-- Milestone 2: expressions, templates, declarative connectors, source adapters, config history.

ALTER TABLE routes ADD COLUMN last_error TEXT;
ALTER TABLE routes ADD COLUMN last_error_at BIGINT;
ALTER TABLE deliveries ADD COLUMN warning TEXT;
ALTER TABLE notifications ADD COLUMN template_id TEXT;
ALTER TABLE sources ADD COLUMN key_sealed TEXT;
ALTER TABLE source_requests ADD COLUMN headers TEXT;
ALTER TABLE templates ADD COLUMN title TEXT NOT NULL DEFAULT '';
ALTER TABLE connector_definitions ADD COLUMN kind TEXT NOT NULL DEFAULT '';
CREATE UNIQUE INDEX connector_definitions_kind ON connector_definitions (workspace_id, kind);
CREATE INDEX config_revisions_workspace ON config_revisions (workspace_id, id);
ALTER TABLE config_revisions ADD COLUMN summary TEXT NOT NULL DEFAULT '';
