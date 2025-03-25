CREATE TABLE
  offer_snapshots (
    provider_peer_id TEXT NOT NULL REFERENCES peers (peer_id),
    protocol_id TEXT NOT NULL,
    --
    -- Protocol-unique offer ID (may be superseded by newer payload).
    offer_id TEXT NOT NULL,
    --
    -- Becomes inactive when superseded by another offer
    -- with the same protocol and ID, but different payload.
    -- Or if absent in a recent heartbeat.
    active INTEGER NOT NULL CHECK (active IN (0, 1)),
    --
    -- Protocol-specific JSON payload.
    protocol_payload TEXT NOT NULL,
    --
    -- When we first discovered this snapshot, in our clock.
    discovered_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    --
    -- Last time it was updated, in our clock.
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
  );
--
CREATE INDEX idx_offer_snapshots_provider_peer_id ON offer_snapshots (provider_peer_id);
CREATE INDEX idx_offer_snapshots_protocol_id ON offer_snapshots (protocol_id);
CREATE INDEX idx_offer_snapshots_active ON offer_snapshots (active);
--
-- There may be many inactive snapshots with the same triple...
CREATE INDEX idx_offer_snapshots_main_inactive ON offer_snapshots (provider_peer_id, offer_id, protocol_id)
WHERE
  active = 0;
--
-- ..but only one active snapshot per triple.
CREATE UNIQUE INDEX idx_offer_snapshots_main_active ON offer_snapshots (provider_peer_id, offer_id, protocol_id)
WHERE
  active = 1;
