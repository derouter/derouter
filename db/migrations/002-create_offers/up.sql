CREATE TABLE
  offers (
    provider_peer_id TEXT NOT NULL REFERENCES peers (peer_id),
    protocol_id TEXT NOT NULL,
    --
    -- Protocol-unique offer ID.
    offer_id TEXT NOT NULL,
    --
    -- Whether had the provider included this offer in the recent heartbeat.
    enabled INTEGER DEFAULT 1,
    --
    -- Protocol-specific JSON payload.
    protocol_payload TEXT NOT NULL,
    --
    -- In our clock.
    discovered_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    --
    -- In our clock.
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    --
    PRIMARY KEY (provider_peer_id, protocol_id, offer_id)
  );
--
CREATE INDEX idx_offers_enabled ON offers (enabled);
