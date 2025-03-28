CREATE TABLE
  service_connections (
    -- > If a table contains a column of type INTEGER PRIMARY KEY,
    -- > then that column becomes an alias for the ROWID.
    -- > https://www.sqlite.org/autoinc.html
    --
    rowid INTEGER PRIMARY KEY,
    --
    offer_snapshot_rowid INTEGER NOT NULL REFERENCES offer_snapshots (rowid),
    consumer_peer_id TEXT NOT NULL REFERENCES peers (peer_id),
    --
    -- When the connection was opened.
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    --
    -- When the connection was closed.
    -- Set to zero when unknown (e.g. after an abrupt restart).
    destroyed_at TIMESTAMP
  );
--
CREATE INDEX idx_service_connections_offer_snapshot_rowid --
ON service_connections (offer_snapshot_rowid);
--
CREATE INDEX idx_service_connections_consumer_peer_id --
ON service_connections (consumer_peer_id);
--
CREATE INDEX idx_service_connections_created_at --
ON service_connections (created_at);
--
CREATE INDEX idx_service_connections_destroyed_at --
ON service_connections (destroyed_at);
