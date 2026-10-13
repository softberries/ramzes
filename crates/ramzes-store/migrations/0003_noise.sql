-- Milestone 3: shaping state on notifications, timers, connection pausing.

ALTER TABLE notifications ADD COLUMN target_key TEXT NOT NULL DEFAULT '';
ALTER TABLE notifications ADD COLUMN sent_at BIGINT;
ALTER TABLE notifications ADD COLUMN sent_count BIGINT NOT NULL DEFAULT 0;
ALTER TABLE notifications ADD COLUMN held_until BIGINT;
ALTER TABLE notifications ADD COLUMN hold_kind TEXT;
ALTER TABLE notifications ADD COLUMN silence_id TEXT;
ALTER TABLE notifications ADD COLUMN resolved_at BIGINT;
ALTER TABLE notifications ADD COLUMN title TEXT NOT NULL DEFAULT '';
ALTER TABLE notifications ADD COLUMN severity BIGINT NOT NULL DEFAULT 1;
-- Notifications from before this migration were all sent when they were created.
UPDATE notifications SET sent_at = created_at, sent_count = 1 WHERE state = 'sent';
CREATE INDEX notifications_target ON notifications (workspace_id, target_key, state);
CREATE INDEX notifications_state ON notifications (workspace_id, state, id);
CREATE INDEX notifications_sent ON notifications (workspace_id, target_key, sent_at);
ALTER TABLE notification_events ADD COLUMN role TEXT NOT NULL DEFAULT 'first';
ALTER TABLE timers ADD COLUMN data TEXT;
CREATE INDEX timers_due ON timers (due_at);
CREATE INDEX timers_notification ON timers (notification_id);
ALTER TABLE connections ADD COLUMN paused_until BIGINT;
ALTER TABLE connections ADD COLUMN failure_streak BIGINT NOT NULL DEFAULT 0;
CREATE INDEX silences_workspace ON silences (workspace_id, ends_at);
