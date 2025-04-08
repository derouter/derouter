use either::Either;
use rusqlite::params;
use serde_repr::{Deserialize_repr, Serialize_repr};

/// Currency set for a connection.
#[derive(Serialize_repr, Deserialize_repr, Debug, Clone, Copy)]
#[repr(i64)]
pub enum Currency {
	/// Native Polygon token ($POL).
	Polygon = 0,
}

impl TryFrom<i64> for Currency {
	type Error = &'static str;

	fn try_from(value: i64) -> Result<Self, Self::Error> {
		match value {
			0 => Ok(Currency::Polygon),
			_ => Err("Invalid value for Currency"),
		}
	}
}

impl Currency {
	pub fn code(self) -> &'static str {
		match self {
			Currency::Polygon => "POL",
		}
	}
}

/// `offer_snapshot` may be either an existing offer snapshot rowid
/// (panics if it doesn't actually exist), or a new snapshot tuple
/// `(provider_peer_id, offer_id, protocol_id, protocol_payload)`
/// which will be upserted into the database.
/// Returns `(offer_snapshot_rowid, connection_rowid)`.
pub fn create_service_connection(
	conn: &mut rusqlite::Connection,
	offer_snapshot: Either<
		i64,
		(
			libp2p::PeerId, // provider_peer_id
			&String,        // offer_id
			&String,        // protocol_id
			&String,        // protocol_payload
		),
	>,
	consumer_peer_id: libp2p::PeerId,
	currency: Currency,
) -> (i64, i64) {
	let tx = conn.transaction().unwrap();

	let result = {
		let mut ensure_peer_stmt = tx
			.prepare_cached(
				r#"
					INSERT
					INTO peers (peer_id)
					VALUES (?1)
					ON CONFLICT DO NOTHING
				"#,
			)
			.unwrap();

		let offer_snapshot_rowid = if offer_snapshot.is_left() {
			offer_snapshot.unwrap_left()
		} else {
			let offer_snapshot = offer_snapshot.unwrap_right();

			#[derive(Debug)]
			struct OfferSnapshot<'a> {
				provider_peer_id: libp2p::PeerId,
				offer_id: &'a String,
				protocol_id: &'a String,
				protocol_payload: &'a String,
			}

			let offer_snapshot = OfferSnapshot {
				provider_peer_id: offer_snapshot.0,
				offer_id: offer_snapshot.1,
				protocol_id: offer_snapshot.2,
				protocol_payload: offer_snapshot.3,
			};

			log::debug!(
				"Upsert provider peer record: {:?}",
				offer_snapshot.provider_peer_id
			);

			match ensure_peer_stmt
				.execute(params!(offer_snapshot.provider_peer_id.to_base58()))
				.unwrap()
				.cmp(&0)
			{
				std::cmp::Ordering::Less => unreachable!(),
				std::cmp::Ordering::Equal => {
					log::debug!("Provider peer record already exists in DB")
				}
				std::cmp::Ordering::Greater => {
					log::debug!("Inserted provider peer record")
				}
			}

			log::debug!("Upsert and enable {:?}", offer_snapshot);

			let mut upsert_offer_snapshot_stmt = tx
				.prepare_cached(
					r#"
						INSERT
						INTO offer_snapshots (
							provider_peer_id, -- ?1
							protocol_id,      -- ?2
							offer_id,         -- ?3
							active,
							protocol_payload -- ?4
						)
						VALUES (
							?1, ?2, ?3, 1, ?4
						)
						ON CONFLICT DO
							UPDATE SET active = 1
						RETURNING
							ROWID
					"#,
				)
				.unwrap();

			upsert_offer_snapshot_stmt
				.query_row(
					params!(
						offer_snapshot.provider_peer_id.to_base58(),
						offer_snapshot.protocol_id,
						offer_snapshot.offer_id,
						offer_snapshot.protocol_payload
					),
					|row| row.get(0),
				)
				.unwrap()
		};

		log::debug!("Upsert consumer peer record: {:?}", consumer_peer_id);

		match ensure_peer_stmt
			.execute(params!(consumer_peer_id.to_base58()))
			.unwrap()
			.cmp(&0)
		{
			std::cmp::Ordering::Less => unreachable!(),
			std::cmp::Ordering::Equal => {
				log::debug!("Consumer pper record already exists in DB")
			}
			std::cmp::Ordering::Greater => {
				log::debug!("Inserted consumer peer record")
			}
		}

		#[derive(Debug)]
		struct ServiceConnectionRow {
			offer_snapshot_rowid: i64,
			consumer_peer_id: String,
		}

		let mut insert_service_connection_stmt = tx
			.prepare_cached(
				r#"
					INSERT
					INTO service_connections (
						offer_snapshot_rowid, -- ?1
						consumer_peer_id,     -- ?2
						currency              -- ?3
					) VALUES (
						?1, ?2, ?3
					)
				"#,
			)
			.unwrap();

		let service_connection_row = ServiceConnectionRow {
			offer_snapshot_rowid,
			consumer_peer_id: consumer_peer_id.to_base58(),
		};

		log::debug!("Insert {:?}", service_connection_row);

		insert_service_connection_stmt
			.execute(params!(
				service_connection_row.offer_snapshot_rowid,
				service_connection_row.consumer_peer_id,
				currency as i64
			))
			.unwrap();

		(offer_snapshot_rowid, tx.last_insert_rowid())
	};

	tx.commit().unwrap();
	result
}
