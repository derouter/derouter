CREATE TABLE
  offers (
    provider_peer_id TEXT NOT NULL REFERENCES providers (peer_id),
    protocol_id TEXT NOT NULL,
    offer_id TEXT NOT NULL, -- Protocol-unique offer ID.
    protocol_payload TEXT NOT NULL, -- Protocol-specific JSON payload.
    description TEXT, -- Optional human-readable offer description.
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (provider_peer_id, protocol_id, offer_id)
  );
