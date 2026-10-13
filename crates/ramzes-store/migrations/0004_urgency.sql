-- Milestone 4: escalation steps send their own deliveries.

ALTER TABLE deliveries ADD COLUMN template_id TEXT;
-- The escalation step that created the delivery, counted from 0. NULL for the route's own targets.
ALTER TABLE deliveries ADD COLUMN step BIGINT;
ALTER TABLE notification_events ADD COLUMN dedup_key TEXT;
