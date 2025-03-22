CREATE TABLE
  providers (
    peer_id TEXT PRIMARY KEY,
    name TEXT,
    teaser TEXT,
    description TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    latest_heartbeat_at TEXT NOT NULL
  );
--
CREATE INDEX idx_providers_name ON providers (name);
