-- Add slots_json column to router_combos for per-slot account selection.
-- Nullable: existing rows without this column fall back to models_json.
ALTER TABLE router_combos ADD COLUMN slots_json TEXT;
