-- One schema for SQLite and Postgres: only TEXT and BIGINT columns.
-- Booleans are 0/1, timestamps are unix milliseconds, ids are UUIDv7 text, JSON is text.

CREATE TABLE workspaces (id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at BIGINT NOT NULL);
CREATE TABLE settings (workspace_id TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (workspace_id, key));
CREATE TABLE users (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, email TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL, created_at BIGINT NOT NULL);
CREATE TABLE sessions (token_hash TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, user_id TEXT NOT NULL, created_at BIGINT NOT NULL, expires_at BIGINT NOT NULL);
CREATE TABLE api_tokens (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, name TEXT NOT NULL, token_hash TEXT NOT NULL UNIQUE, token_prefix TEXT NOT NULL, created_by TEXT NOT NULL, created_at BIGINT NOT NULL, last_used_at BIGINT);
CREATE TABLE sources (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL, key_hash TEXT NOT NULL UNIQUE, key_prefix TEXT NOT NULL, mapping TEXT, created_at BIGINT NOT NULL);
CREATE TABLE source_requests (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, source_id TEXT NOT NULL, received_at BIGINT NOT NULL, body TEXT NOT NULL, error TEXT);
CREATE INDEX source_requests_source ON source_requests (source_id, id);
CREATE TABLE events (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, source_id TEXT NOT NULL, topic TEXT NOT NULL, severity BIGINT NOT NULL, status TEXT NOT NULL, title TEXT NOT NULL, body TEXT, payload TEXT NOT NULL, dedup_key TEXT, idempotency_key TEXT, occurred_at BIGINT NOT NULL, received_at BIGINT NOT NULL, routed_at BIGINT, claimed_until BIGINT, route_count BIGINT NOT NULL DEFAULT 0);
CREATE UNIQUE INDEX events_idempotency ON events (source_id, idempotency_key);
CREATE INDEX events_unrouted ON events (id) WHERE routed_at IS NULL;
CREATE INDEX events_topic ON events (workspace_id, topic, id);
CREATE INDEX events_source ON events (workspace_id, source_id, id);
CREATE INDEX events_received ON events (received_at);
CREATE TABLE event_labels (event_id TEXT NOT NULL, workspace_id TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (event_id, key));
CREATE INDEX event_labels_kv ON event_labels (key, value, event_id);
CREATE TABLE connections (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, kind TEXT NOT NULL, name TEXT NOT NULL, settings TEXT NOT NULL, credentials TEXT NOT NULL, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL);
CREATE TABLE connector_definitions (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, name TEXT NOT NULL, definition TEXT NOT NULL, created_at BIGINT NOT NULL);
CREATE TABLE templates (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, connector_kind TEXT NOT NULL, name TEXT NOT NULL, body TEXT NOT NULL, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL);
CREATE TABLE routes (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, name TEXT NOT NULL, position BIGINT NOT NULL, enabled BIGINT NOT NULL, stop BIGINT NOT NULL, match_json TEXT NOT NULL, targets_json TEXT NOT NULL, shaping_json TEXT NOT NULL, escalation_json TEXT NOT NULL, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL);
CREATE TABLE silences (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, match_json TEXT NOT NULL, starts_at BIGINT NOT NULL, ends_at BIGINT NOT NULL, author TEXT NOT NULL, comment TEXT, created_at BIGINT NOT NULL);
CREATE TABLE notifications (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, route_id TEXT NOT NULL, route_name TEXT NOT NULL, connection_id TEXT NOT NULL, destination TEXT, group_key TEXT, dedup_key TEXT, state TEXT NOT NULL, reason TEXT, event_count BIGINT NOT NULL, replay BIGINT NOT NULL, acked_by TEXT, acked_at BIGINT, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL);
CREATE TABLE notification_events (notification_id TEXT NOT NULL, event_id TEXT NOT NULL, workspace_id TEXT NOT NULL, PRIMARY KEY (notification_id, event_id));
CREATE INDEX notification_events_event ON notification_events (event_id);
CREATE TABLE deliveries (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, notification_id TEXT NOT NULL, connection_id TEXT NOT NULL, destination TEXT, state TEXT NOT NULL, attempts BIGINT NOT NULL, next_attempt_at BIGINT NOT NULL, claimed_until BIGINT, last_error TEXT, external_ref TEXT, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL);
CREATE INDEX deliveries_due ON deliveries (next_attempt_at) WHERE state IN ('pending', 'in_progress');
CREATE INDEX deliveries_notification ON deliveries (notification_id);
CREATE INDEX deliveries_state ON deliveries (workspace_id, state, id);
CREATE TABLE delivery_attempts (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, delivery_id TEXT NOT NULL, started_at BIGINT NOT NULL, duration_ms BIGINT NOT NULL, outcome TEXT NOT NULL, status BIGINT, detail TEXT);
CREATE INDEX delivery_attempts_delivery ON delivery_attempts (delivery_id, id);
CREATE TABLE timers (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, kind TEXT NOT NULL, due_at BIGINT NOT NULL, claimed_until BIGINT, notification_id TEXT NOT NULL);
CREATE TABLE config_revisions (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, author TEXT NOT NULL, created_at BIGINT NOT NULL, snapshot TEXT NOT NULL);
