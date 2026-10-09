-- Adds cursor_auto_router table for storing task-based auto routing configuration
CREATE TABLE IF NOT EXISTS cursor_auto_router (
    id TEXT PRIMARY KEY DEFAULT 'default',
    enabled INTEGER NOT NULL DEFAULT 1,
    coding_slots_json TEXT NOT NULL DEFAULT '[]',
    reasoning_slots_json TEXT NOT NULL DEFAULT '[]',
    fast_slots_json TEXT NOT NULL DEFAULT '[]',
    updated_at_ms INTEGER NOT NULL
);
