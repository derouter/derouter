use libp2p::PeerId;
use rusqlite::{OptionalExtension, params};

use crate::{
	dto::{OfferRemoved, OfferSnapshot, ProviderHeartbeat, ProviderRecord},
	state::SharedState,
	util::TIME_ZERO,
};

use super::proto;

pub(super) async fn handle_heartbeat(
	state: &SharedState,
	source: PeerId,
	heartbeat: proto::gossipsub::Heartbeat,
) -> eyre::Result<()> {
	let mut updated_providers = Vec::<ProviderRecord>::new();
	let mut heartbeat_providers = Vec::<ProviderHeartbeat>::new();
	let mut updated_offers = Vec::<OfferSnapshot>::new();
	let mut removed_offers = Vec::<OfferRemoved>::new();

	// REFACTOR: Try flattening the block.
	// See https://github.com/rust-lang/rust/issues/97331#issuecomment-2746121745.
	{
		let mut database = state.database.lock().await;
		let tx = database.transaction().unwrap();

		struct Peer {
			their_provider_updated_at: Option<chrono::DateTime<chrono::Utc>>,
			their_latest_heartbeat_timestamp: Option<chrono::DateTime<chrono::Utc>>,
		}

		let existing_peer = tx
			.query_row(
				r#"
          SELECT
            their_provider_updated_at,
            their_latest_heartbeat_timestamp
          FROM
            peers
          WHERE
            peer_id = ?1
        "#,
				[source.to_base58()],
				|row| {
					Ok(Peer {
						their_provider_updated_at: row.get(0)?,
						their_latest_heartbeat_timestamp: row.get(1)?,
					})
				},
			)
			.optional()
			.unwrap();

		#[derive(Debug)]
		struct OfferSnapshotRow {
			provider_peer_id: String,
			protocol_id: String,
			offer_id: String,
			protocol_payload: String,
		}

		if let Some(peer) = existing_peer {
			if let Some(their_latest_heartbeat_timestamp) =
				peer.their_latest_heartbeat_timestamp
			{
				if their_latest_heartbeat_timestamp >= heartbeat.timestamp {
					return Err(eyre::eyre!(
						"Outdated heartbeat timestamp: stored = {}, incoming = {}",
						their_latest_heartbeat_timestamp,
						heartbeat.timestamp,
					));
				}
			}

			if let Some(provider_details) = heartbeat.provider {
				let local_updated_at =
					peer.their_provider_updated_at.unwrap_or(*TIME_ZERO);

				match provider_details.updated_at.cmp(&local_updated_at) {
					std::cmp::Ordering::Less => {
						return Err(eyre::eyre!(
							"{:?}'s provider updated_at value {} is less than stored ({})",
							source,
							provider_details.updated_at,
							local_updated_at
						));
					}

					std::cmp::Ordering::Equal => {
						log::debug!("Provider not updated");

						tx.execute(
							r#"
                UPDATE
                  peers
                SET
                  their_latest_heartbeat_timestamp = ?2,
                  our_latest_heartbeat_timestamp = CURRENT_TIMESTAMP
                WHERE
                  peer_id = ?1
              "#,
							params!(
								source.to_base58(),  // ?1
								heartbeat.timestamp, // ?2
							),
						)
						.unwrap();

						heartbeat_providers.push(ProviderHeartbeat {
							peer_id: source.to_base58(),
							latest_heartbeat_at: chrono::Utc::now(),
						});
					}

					std::cmp::Ordering::Greater => {
						log::debug!("Provider details updated");

						tx.execute(
							r#"
                UPDATE
                  peers
                SET
                  provider_name = ?2,
                  provider_teaser = ?3,
                  provider_description = ?4,
                  their_provider_updated_at = ?5,
                  our_provider_updated_at = CURRENT_TIMESTAMP,
                  their_latest_heartbeat_timestamp = ?6,
                  our_latest_heartbeat_timestamp = CURRENT_TIMESTAMP
                WHERE
                  peer_id = ?1
              "#,
							params!(
								source.to_base58(),           // ?1
								provider_details.name,        // ?2
								provider_details.teaser,      // ?3
								provider_details.description, // ?4
								provider_details.updated_at,  // ?5
								heartbeat.timestamp,          // ?6
							),
						)
						.unwrap();

						updated_providers.push(ProviderRecord {
							peer_id: source.to_base58(),
							name: provider_details.name,
							teaser: provider_details.teaser,
							description: provider_details.description,
							updated_at: chrono::Utc::now(),
							latest_heartbeat_at: chrono::Utc::now(),
						});

						#[derive(Debug)]
						struct ActiveOfferSnapshotRow {
							rowid: i64,
							protocol_id: String,
							offer_id: String,
							protocol_payload: String,
						}

						let mut statement = tx
							.prepare_cached(
								r#"
                  SELECT
										ROWID,           -- #0
                    protocol_id,     -- #1
                    offer_id,        -- #2
										protocol_payload -- #3
                  FROM
                    offer_snapshots
                  WHERE
                    provider_peer_id = ?1 AND
                    active = 1
                "#,
							)
							.unwrap();

						let active_offer_snapshots = statement
							.query_map(params!(source.to_base58()), |row| {
								Ok(ActiveOfferSnapshotRow {
									rowid: row.get(0)?,
									protocol_id: row.get(1)?,
									offer_id: row.get(2)?,
									protocol_payload: row.get(3)?,
								})
							})
							.unwrap()
							.map(|s| s.unwrap());

						for active_snapshot in active_offer_snapshots {
							let protocol_id = &active_snapshot.protocol_id;
							let offer_id = &active_snapshot.offer_id;

							let incoming_snapshot = provider_details
								.offers
								.get(protocol_id)
								.and_then(|map| map.get(offer_id));

							if let Some(incoming_snapshot) = incoming_snapshot {
								let incoming_payload_string =
									serde_json::to_string(&incoming_snapshot.protocol_payload)
										.expect("should serialize offer payload");

								if incoming_payload_string == active_snapshot.protocol_payload {
									log::debug!(
										"Offer snapshot did not change: {:?}",
										active_snapshot
									);
								} else {
									log::debug!(
										"Disable due to payload change: {:?}",
										active_snapshot
									);

									tx.execute(
										r#"
											UPDATE
												offer_snapshots
											SET
												active = 0
											WHERE
												ROWID = ?1
										"#,
										params!(active_snapshot.rowid),
									)
									.unwrap();

									let new_snapshot = OfferSnapshotRow {
										provider_peer_id: source.to_base58(),
										protocol_id: protocol_id.clone(),
										offer_id: offer_id.clone(),
										protocol_payload: incoming_payload_string,
									};

									log::debug!("Upsert and enable {:?}", new_snapshot);

									let snapshot_rowid: i64 = tx
										.query_row(
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
												ON CONFLICT (
													provider_peer_id,
													protocol_id,
													offer_id,
													protocol_payload
												)
												DO UPDATE SET active = 1
												RETURNING
													ROWID
										"#,
											params!(
												new_snapshot.provider_peer_id, // ?1
												new_snapshot.protocol_id,      // ?2
												new_snapshot.offer_id,         // ?3
												new_snapshot.protocol_payload, // ?4
											),
											|row| row.get(0),
										)
										.unwrap();

									updated_offers.push(OfferSnapshot {
										snapshot_id: snapshot_rowid,
										provider_peer_id: source.to_base58(),
										protocol_id: protocol_id.clone(),
										offer_id: offer_id.clone(),
										protocol_payload: incoming_snapshot
											.protocol_payload
											.clone(),
										active: true,
									});
								}
							} else {
								log::debug!("Disable missing {:?}", active_snapshot);

								tx.execute(
									r#"
                    UPDATE
                      offer_snapshots
                    SET
                      active = 0
                    WHERE
                      ROWID = ?1
                  "#,
									params!(active_snapshot.rowid),
								)
								.unwrap();

								removed_offers.push(OfferRemoved {
									snapshot_id: active_snapshot.rowid,
									protocol_id: protocol_id.to_string(),
									provider_peer_id: source.to_base58(),
									offer_id: offer_id.to_string(),
								});
							}
						}

						for (protocol_id, offers_by_protocol) in provider_details.offers {
							for (offer_id, incoming_offer) in offers_by_protocol {
								let existing_active_snapshot = tx
									.query_row(
										r#"
                      SELECT
                        1
                      FROM
                        offer_snapshots
                      WHERE
                        provider_peer_id = ?1 AND
                        protocol_id = ?2 AND
                        offer_id = ?3 AND
												active = 1
                    "#,
										params!(source.to_base58(), &protocol_id, &offer_id),
										|_| Ok(1),
									)
									.optional()
									.unwrap();

								if existing_active_snapshot.is_none() {
									let new_snapshot = OfferSnapshotRow {
										provider_peer_id: source.to_base58(),
										protocol_id: protocol_id.clone(),
										offer_id: offer_id.clone(),
										protocol_payload: serde_json::to_string(
											&incoming_offer.protocol_payload,
										)
										.expect("should serialize offer payload"),
									};

									log::debug!("Upsert and enable {:?}", new_snapshot);

									let snapshot_rowid: i64 = tx
										.query_row(
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
											params!(
												new_snapshot.provider_peer_id, // ?1
												new_snapshot.protocol_id,      // ?2
												new_snapshot.offer_id,         // ?3
												new_snapshot.protocol_payload, // ?4
											),
											|row| row.get(0),
										)
										.unwrap();

									updated_offers.push(OfferSnapshot {
										snapshot_id: snapshot_rowid,
										provider_peer_id: source.to_base58(),
										offer_id,
										protocol_id: protocol_id.clone(),
										protocol_payload: incoming_offer.protocol_payload,
										active: true,
									});
								}
							}
						}
					}
				}
			} else {
				log::debug!("Ignoring hearbeat without provider data");
			}
		} else if let Some(provider_details) = heartbeat.provider {
			log::debug!("Insert new provider peer");

			tx.execute(
				r#"
					INSERT INTO peers (
						peer_id,                   -- ?1
						provider_name,             -- ?2
						provider_teaser,           -- ?3
						provider_description,      -- ?4
						their_provider_updated_at, -- ?5
						our_provider_updated_at,
						their_latest_heartbeat_timestamp, -- ?6
						our_latest_heartbeat_timestamp
					) VALUES (
						?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP, ?6, CURRENT_TIMESTAMP
					)
				"#,
				params!(
					source.to_base58(),           // ?1
					provider_details.name,        // ?2
					provider_details.teaser,      // ?3
					provider_details.description, // ?4
					provider_details.updated_at,  // ?5
					heartbeat.timestamp,          // ?6
				),
			)
			.unwrap();

			for (protocol_id, offers_by_protocol) in provider_details.offers {
				for (offer_id, offer) in offers_by_protocol {
					let new_snapshot = OfferSnapshotRow {
						provider_peer_id: source.to_base58(),
						protocol_id: protocol_id.clone(),
						offer_id: offer_id.clone(),
						protocol_payload: serde_json::to_string(&offer.protocol_payload)
							.expect("should serialize offer payload"),
					};

					log::debug!("Insert {:?}", new_snapshot);

					tx.execute(
						r#"
							INSERT INTO offer_snapshots (
								provider_peer_id, -- ?1
								protocol_id,      -- ?2
								offer_id,         -- ?3
								active,
								protocol_payload -- ?4
							) VALUES (
								?1, ?2, ?3, 1, ?4
							)
						"#,
						params!(
							new_snapshot.provider_peer_id, // ?1
							new_snapshot.protocol_id,      // ?2
							new_snapshot.offer_id,         // ?3
							new_snapshot.protocol_payload, // ?4
						),
					)
					.unwrap();

					updated_offers.push(OfferSnapshot {
						snapshot_id: tx.last_insert_rowid(),
						provider_peer_id: source.to_base58(),
						offer_id,
						protocol_id: protocol_id.clone(),
						protocol_payload: offer.protocol_payload,
						active: true,
					});
				}
			}
		} else {
			log::debug!("Ignoring hearbeat without provider data");
		}

		tx.commit().unwrap();
	}

	let consumer_lock = state.rpc.lock().await;
	let tx = &consumer_lock.event_tx;

	for provider in updated_providers {
		let _ = tx.send(crate::state::RpcEvent::ProviderUpdated(provider));
	}

	for provider in heartbeat_providers {
		let _ = tx.send(crate::state::RpcEvent::ProviderHeartbeat(provider));
	}

	for offer in removed_offers {
		let _ = tx.send(crate::state::RpcEvent::OfferRemoved(offer));
	}

	for offer in updated_offers {
		let _ = tx.send(crate::state::RpcEvent::OfferUpdated(offer));
	}

	Ok(())
}
