-- Adds task engines columns (Vision Sidecar & Subagent configuration) to cursor_auto_router
ALTER TABLE cursor_auto_router ADD COLUMN vision_auto INTEGER NOT NULL DEFAULT 1;
ALTER TABLE cursor_auto_router ADD COLUMN vision_slots_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE cursor_auto_router ADD COLUMN subagent_auto INTEGER NOT NULL DEFAULT 1;
ALTER TABLE cursor_auto_router ADD COLUMN subagent_slots_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE cursor_auto_router ADD COLUMN subagent_write_access INTEGER NOT NULL DEFAULT 1;
