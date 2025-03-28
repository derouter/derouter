CREATE TABLE
  peers (
    peer_id TEXT PRIMARY KEY,
    --
    provider_name TEXT,
    provider_teaser TEXT,
    provider_description TEXT,
    --
    -- When the provider details were last updated, in their clock.
    their_provider_updated_at TIMESTAMP,
    --
    -- When the provider details were last updated, in our clock.
    our_provider_updated_at TIMESTAMP,
    --
    -- Latest heartbeat timestamp, in their clock.
    their_latest_heartbeat_timestamp TIMESTAMP,
    --
    -- Latest heartbeat timestamp, in our clock.
    our_latest_heartbeat_timestamp TIMESTAMP
  );
--
CREATE INDEX idx_peers_provider_name ON peers (provider_name);
CREATE INDEX idx_peers_our_latest_heartbeat_timestamp ON peers (our_latest_heartbeat_timestamp);
