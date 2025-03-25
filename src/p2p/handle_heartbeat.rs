use libp2p::PeerId;
use rusqlite::{OptionalExtension, params, types::Null};

use crate::{
	dto::{OfferRemoved, OfferUpdated, ProviderHeartbeat, ProviderUpdated},
	state::SharedState,
	util::TIME_ZERO,
};

use super::proto;

pub(super) async fn handle_heartbeat(
	state: &SharedState,
	source: PeerId,
	heartbeat: proto::gossipsub::Heartbeat,
) -> eyre::Result<()> {
	let mut updated_providers = Vec::<ProviderUpdated>::new();
	let mut heartbeat_providers = Vec::<ProviderHeartbeat>::new();
	let mut updated_offers = Vec::<OfferUpdated>::new();
	let mut removed_offers = Vec::<OfferRemoved>::new();

	// FIXME: Try flattening the block.
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
							"{:?}'s provider updated_at ({}) is less than stored value ({})",
							source,
							provider_details.updated_at,
							local_updated_at
						));
					}

					std::cmp::Ordering::Equal => {
						log::debug!("Provider details not updated");

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
							peer_id: source,
							last_heartbeat_at: chrono::Utc::now(),
						});
					}

					std::cmp::Ordering::Greater => {
						log::debug!("Updating provider details");

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

						updated_providers.push(ProviderUpdated {
							peer_id: source,
							name: provider_details.name,
							teaser: provider_details.teaser,
							description: provider_details.description,

							// FIXME: I'm not sure about this. Should it be our clock instead?
							updated_at: provider_details.updated_at,

							last_heartbeat_at: chrono::Utc::now(),
						});

						struct ExistingOffer {
							protocol_id: String,
							offer_id: String,
						}

						let mut stmt = tx
							.prepare_cached(
								r#"
                  SELECT
                    protocol_id,
                    offer_id
                  FROM
                    offers
                  WHERE
                    provider_peer_id = ?1 AND
                    enabled = 1
                "#,
							)
							.unwrap();

						let existing_offers = stmt
							.query_map(params!(source.to_base58()), |row| {
								Ok(ExistingOffer {
									protocol_id: row.get(0)?,
									offer_id: row.get(1)?,
								})
							})
							.unwrap();

						for offer in existing_offers {
							let protocol_id = &offer.as_ref().unwrap().protocol_id;
							let offer_id = &offer.as_ref().unwrap().offer_id;

							if provider_details
								.offers
								.get(protocol_id)
								.and_then(|o| o.get(offer_id))
								.is_none()
							{
								log::debug!("Disabling offer");

								tx.execute(
									r#"
                    UPDATE
                      offers
                    SET
                      enabled = 0,
                      updated_at = CURRENT_TIMESTAMP
                    WHERE
                      provider_peer_id = ?1 AND
                      protocol_id = ?2 AND
                      offer_id = ?3
                  "#,
									params!(
										source.to_base58(), // ?1
										protocol_id,        // ?2
										offer_id,           // ?3
									),
								)
								.unwrap();

								removed_offers.push(OfferRemoved {
									_provider_peer_id: source,
									protocol_id: protocol_id.clone(),
									_offer_id: offer_id.clone(),
								});
							}
						}

						for (protocol_id, offers_by_protocol) in provider_details.offers {
							for (offer_id, incoming_offer) in offers_by_protocol {
								struct Offer {
									protocol_payload: String,
									enabled: bool,
								}

								let existing_offer = tx
									.query_row(
										r#"
                      SELECT
                        protocol_payload,
                        enabled
                      FROM
                        offers
                      WHERE
                        provider_peer_id = ?1 AND
                        protocol_id = ?2 AND
                        offer_id = ?3
                    "#,
										params![source.to_base58(), &protocol_id, &offer_id],
										|row| {
											Ok(Offer {
												protocol_payload: row.get(0)?,
												enabled: row.get_ref(1)?.as_i64()? == 1,
											})
										},
									)
									.optional()
									.unwrap();

								if let Some(existing_offer) = existing_offer {
									let incoming_offer_payload =
										serde_json::to_string(&incoming_offer.protocol_payload)
											.expect("should serialize offer payload");

									if existing_offer.protocol_payload != incoming_offer_payload {
										log::debug!("Updating offer due to payload change");

										tx.execute(
											r#"
                        UPDATE
                          offers
                        SET
                          protocol_payload = ?4,
                          enabled = 1,
                          updated_at = CURRENT_TIMESTAMP
                        WHERE
                          provider_peer_id = ?1 AND
                          protocol_id = ?2 AND
                          offer_id = ?3
                      "#,
											params!(
												source.to_base58(),     // ?1
												protocol_id,            // ?2
												offer_id,               // ?3
												incoming_offer_payload, // ?4
											),
										)
										.unwrap();

										updated_offers.push(OfferUpdated {
											provider_peer_id: source,
											offer_id,
											protocol_id: protocol_id.clone(),
											_protocol_payload: incoming_offer.protocol_payload,
										});
									} else if !existing_offer.enabled {
										log::debug!("Enabling offer");

										tx.execute(
											r#"
                        UPDATE
                          offers
                        SET
                          enabled = 1,
                          updated_at = CURRENT_TIMESTAMP
                        WHERE
                          provider_peer_id = ?1 AND
                          protocol_id = ?2 AND
                          offer_id = ?3
                      "#,
											params!(
												source.to_base58(), // ?1
												protocol_id,        // ?2
												offer_id,           // ?3
											),
										)
										.unwrap();

										updated_offers.push(OfferUpdated {
											provider_peer_id: source,
											offer_id,
											protocol_id: protocol_id.clone(),
											_protocol_payload: incoming_offer.protocol_payload,
										});
									}
								} else {
									log::debug!("Inserting new offer");

									tx.execute(
										r#"
											INSERT INTO offers (
												provider_peer_id, -- ?1
												protocol_id, -- ?2
												offer_id, -- ?3
												protocol_payload -- ?4
											) VALUES (
												?1, ?2, ?3, ?4
											)
										"#,
										params!(
											source.to_base58(), // ?1
											protocol_id,        // ?2
											offer_id,           // ?3
											serde_json::to_string(&incoming_offer.protocol_payload)
												.expect("should serialize offer payload"), // ?4
										),
									)
									.unwrap();

									updated_offers.push(OfferUpdated {
										provider_peer_id: source,
										offer_id,
										protocol_id: protocol_id.clone(),
										_protocol_payload: incoming_offer.protocol_payload,
									});
								}
							}
						}
					}
				}
			} else {
				log::debug!("Heartbeat has no provider details, clearing peer record");

				tx.execute(
					r#"
          UPDATE
            peers
          SET
            provider_name = ?2,
            provider_teaser = ?3,
            provider_description = ?4,
            their_provider_updated_at = ?5,
            our_provider_updated_at = ?6,
            their_latest_heartbeat_timestamp = ?7,
            our_latest_heartbeat_timestamp = ?8
          WHERE
            peer_id = ?1
        "#,
					params!(
						source.to_base58(),
						Null,
						Null,
						Null,
						Null,
						Null,
						heartbeat.timestamp,
						chrono::Utc::now(),
					),
				)
				.unwrap();

				updated_providers.push(ProviderUpdated {
					peer_id: source,
					name: None,
					teaser: None,
					description: None,
					updated_at: chrono::Utc::now(), // FIXME: ?
					last_heartbeat_at: chrono::Utc::now(),
				});
			}
		} else if let Some(provider_details) = heartbeat.provider {
			log::debug!("Inserting new peer with provider");

			tx.execute(
				r#"
        INSERT INTO peers (
          peer_id, -- ?1
          provider_name, -- ?2
          provider_teaser, -- ?3
          provider_description, -- ?4
          their_provider_updated_at, -- ?5
          our_provider_updated_at, -- ?6
          their_latest_heartbeat_timestamp, -- ?7
          our_latest_heartbeat_timestamp -- ?8
        ) VALUES (
          ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8
        )
      "#,
				params!(
					source.to_base58(),
					provider_details.name,
					provider_details.teaser,
					provider_details.description,
					provider_details.updated_at,
					chrono::Utc::now(),
					heartbeat.timestamp,
					chrono::Utc::now(),
				),
			)
			.unwrap();

			for (protocol_id, offers_by_protocol) in provider_details.offers {
				for (offer_id, offer) in offers_by_protocol {
					log::debug!("Inserting new offer");

					tx.execute(
						r#"
            INSERT INTO offers (
              provider_peer_id, -- ?1
              protocol_id, -- ?2
              offer_id, -- ?3
              protocol_payload -- ?4
            ) VALUES (
              ?1, ?2, ?3, ?4
            )
          "#,
						params!(
							source.to_base58(),
							&protocol_id,
							&offer_id,
							serde_json::to_string(&offer.protocol_payload)
								.expect("should serialize offer payload")
						),
					)
					.unwrap();

					updated_offers.push(OfferUpdated {
						provider_peer_id: source,
						offer_id,
						protocol_id: protocol_id.clone(),
						_protocol_payload: offer.protocol_payload,
					});
				}
			}
		} else {
			log::debug!("Inserting new peer without provider");

			tx.execute(
				r#"
        INSERT INTO peers (
          peer_id, -- ?1
          their_latest_heartbeat_timestamp, -- ?2
          our_latest_heartbeat_timestamp -- ?3
        ) VALUES (
          ?1, ?2, ?3
        )
      "#,
				params!(source.to_base58(), heartbeat.timestamp, chrono::Utc::now(),),
			)
			.unwrap();
		}

		tx.commit().unwrap();
	}

	let consumer_lock = state.consumer.lock().await;
	let tx = &consumer_lock.notification_tx;

	for provider in updated_providers {
		let _ = tx.send(crate::state::ConsumerNotification::ProviderUpdated(
			provider,
		));
	}

	for provider in heartbeat_providers {
		let _ = tx.send(crate::state::ConsumerNotification::ProviderHeartbeat(
			provider,
		));
	}

	for offer in removed_offers {
		let _ = tx.send(crate::state::ConsumerNotification::OfferRemoved(offer));
	}

	for offer in updated_offers {
		let _ = tx.send(crate::state::ConsumerNotification::OfferUpdated(offer));
	}

	Ok(())
}
