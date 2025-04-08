CREATE TABLE
  offer_snapshots (
    -- > If a table contains a column of type INTEGER PRIMARY KEY,
    -- > then that column becomes an alias for the ROWID.
    -- > https://www.sqlite.org/autoinc.html
    --
    rowid INTEGER PRIMARY KEY,
    --
    provider_peer_id TEXT NOT NULL REFERENCES peers (peer_id),
    protocol_id TEXT NOT NULL,
    --
    -- `protocol_id`-unique offer ID (w.r.t. `active`).
    offer_id TEXT NOT NULL,
    --
    -- Protocol-specific payload.
    protocol_payload TEXT NOT NULL,
    --
    -- A snapshot becomes inactive when superseded by another snapshot
    -- with the same `protocol_id` and `offer_id`,
    -- but different `protocol_payload`.
    -- Or when absent in the recent heartbeat.
    -- A snapshot may become active later again.
    active INTEGER NOT NULL CHECK (active IN (0, 1))
  );
--
CREATE INDEX idx_offer_snapshots_provider_peer_id --
ON offer_snapshots (provider_peer_id);
--
CREATE INDEX idx_offer_snapshots_protocol_id --
ON offer_snapshots (protocol_id);
--
CREATE INDEX idx_offer_snapshots_active --
ON offer_snapshots (active);
--
-- Payload must be unique per triple.
CREATE UNIQUE INDEX idx_offer_snapshots_protocol_payload --
ON offer_snapshots (provider_peer_id, protocol_id, offer_id, protocol_payload);
--
-- There may be many inactive snapshots with the same triple...
CREATE INDEX idx_offer_snapshots_main_inactive --
ON offer_snapshots (provider_peer_id, protocol_id, offer_id)
WHERE
  active = 0;
--
-- ...but only one active snapshot per triple.
CREATE UNIQUE INDEX idx_offer_snapshots_main_active --
ON offer_snapshots (provider_peer_id, protocol_id, offer_id)
WHERE
  active = 1;
