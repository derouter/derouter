CREATE TABLE
  service_jobs (
    --
    -- Local-only job row ID.
    rowid INTEGER PRIMARY KEY,
    --
    -- Local offer snapshot reference.
    offer_snapshot_rowid INTEGER NOT NULL REFERENCES offer_snapshots (rowid),
    --
    -- P2P peer ID of the consumer.
    consumer_peer_id TEXT NOT NULL,
    --
    -- The currency enum.
    currency INTEGER NOT NULL,
    --
    -- Initial job arguments (local).
    job_args TEXT,
    --
    -- Job ID unique per provider.
    provider_job_id TEXT NOT NULL,
    --
    -- Private job payload (local).
    private_payload TEXT,
    --
    -- Reason for failure, if any (local).
    -- This field is used to determine if the job has failed.
    reason TEXT,
    --
    -- Protocol-specific failure reason class, if any (local).
    reason_class INTEGER,
    --
    -- When the job was created (local clock).
    created_at_local TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    --
    -- When the job was created (synchronized UTC timestamp).
    created_at_sync INTEGER NOT NULL,
    --
    -- When the job was completed or failed (local clock).
    completed_at_local TIMESTAMP,
    --
    -- When the job was completed or failed (synchronized UTC timestamp).
    completed_at_sync INTEGER,
    --
    -- Potentially-publicly-accessible job payload,
    -- signed by the Customer (synchronized).
    public_payload TEXT,
    --
    -- Consumer -> Provider balance delta (synchronized).
    -- Encoding depends on the connection's currency.
    balance_delta TEXT,
    --
    -- Computed job hash to be signed with `consumer_signature`.
    hash BLOB,
    --
    -- Consumer signature over completed job's hash (synchronized).
    -- Guaranteed to be set along with `completed_at`
    -- on Consumer side, if not failed.
    consumer_signature BLOB,
    --
    -- When the `consumer_signature` was confirmed by the Provider (local).
    -- Once this is set, the balance delta is considered to be applied.
    signature_confirmed_at_local TIMESTAMP,
    --
    -- The most recent signature confirmation error, if any.
    confirmation_error TEXT
  );
--
CREATE INDEX idx_service_jobs_offer_snapshot_rowid --
ON service_jobs (offer_snapshot_rowid);
--
CREATE INDEX idx_service_jobs_consumer_peer_id --
ON service_jobs (consumer_peer_id);
--
CREATE INDEX idx_service_jobs_currency --
ON service_jobs (currency);
--
CREATE INDEX idx_service_jobs_provider_job_id --
ON service_jobs (provider_job_id);
--
CREATE INDEX idx_service_jobs_reason --
ON service_jobs (reason);
--
CREATE INDEX idx_service_jobs_created_at_local --
ON service_jobs (created_at_local);
--
CREATE INDEX idx_service_jobs_completed_at_local --
ON service_jobs (completed_at_local);
--
CREATE INDEX idx_service_jobs_signature_confirmed_at_local --
ON service_jobs (signature_confirmed_at_local);
