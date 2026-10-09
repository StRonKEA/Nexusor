-- Adds tables for model presentation/customization and smart combo router configuration.
CREATE TABLE IF NOT EXISTS model_customizations (
    model_id TEXT PRIMARY KEY,
    custom_name TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS router_combos (
    combo_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    strategy TEXT NOT NULL DEFAULT 'fallback',
    models_json TEXT NOT NULL DEFAULT '[]',
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
