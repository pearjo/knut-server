use futures::future::{BoxFuture, FutureExt};
use hyper::{body::Buf, Body};
use log::{debug, info};
use std::{ops::Deref, str};
use uuid::Uuid;

use crate::{
    event::Event,
    pairing::{Pairing, Permissions},
    pointer,
    tlv::{self, Type, Value},
    transport::http::handler::TlvHandlerExt,
};

pub struct Pairings;

impl Pairings {
    pub fn new() -> Pairings { Pairings }
}

#[derive(Debug, Clone)]
enum StepNumber {
    Unknown = 0,
    Res = 2,
}

#[derive(Debug, Clone)]
enum HandlerNumber {
    Add = 3,
    Remove = 4,
    List = 5,
}

pub enum HandlerType {
    Add {
        pairing_id: Vec<u8>,
        ltpk: Vec<u8>,
        permissions: Permissions,
    },
    Remove {
        pairing_id: Vec<u8>,
    },
    List,
}

impl TlvHandlerExt for Pairings {
    type ParseResult = HandlerType;
    type Result = tlv::Container;

    fn parse(&self, body: Body) -> BoxFuture<Result<HandlerType, tlv::ErrorContainer>> {
        async {
            let aggregated_body = hyper::body::aggregate(body)
                .await
                .map_err(|_| tlv::ErrorContainer::new(StepNumber::Unknown as u8, tlv::Error::Unknown))?;

            debug!("received body: {:?}", aggregated_body.chunk());

            let mut decoded = tlv::decode(aggregated_body.chunk());
            if decoded.get(&(Type::State as u8)) != Some(&vec![1]) {
                return Err(tlv::ErrorContainer::new(0, tlv::Error::Unknown));
            }
            match decoded.get(&(Type::Method as u8)) {
                Some(handler) => match handler[0] {
                    x if x == HandlerNumber::Add as u8 => {
                        let pairing_id = decoded
                            .remove(&(Type::Identifier as u8))
                            .ok_or(tlv::ErrorContainer::new(StepNumber::Res as u8, tlv::Error::Unknown))?;
                        let ltpk = decoded
                            .remove(&(Type::PublicKey as u8))
                            .ok_or(tlv::ErrorContainer::new(StepNumber::Res as u8, tlv::Error::Unknown))?;
                        let perms = decoded
                            .remove(&(Type::Permissions as u8))
                            .ok_or(tlv::ErrorContainer::new(StepNumber::Res as u8, tlv::Error::Unknown))?;
                        let permissions = Permissions::from_byte(perms[0])
                            .map_err(|_| tlv::ErrorContainer::new(StepNumber::Res as u8, tlv::Error::Unknown))?;
                        Ok(HandlerType::Add {
                            pairing_id,
                            ltpk,
                            permissions,
                        })
                    },
                    x if x == HandlerNumber::Remove as u8 => {
                        let pairing_id = decoded
                            .remove(&(Type::Identifier as u8))
                            .ok_or(tlv::ErrorContainer::new(StepNumber::Res as u8, tlv::Error::Unknown))?;
                        Ok(HandlerType::Remove { pairing_id })
                    },
                    x if x == HandlerNumber::List as u8 => Ok(HandlerType::List),
                    _ => Err(tlv::ErrorContainer::new(StepNumber::Unknown as u8, tlv::Error::Unknown)),
                },
                None => Err(tlv::ErrorContainer::new(StepNumber::Unknown as u8, tlv::Error::Unknown)),
            }
        }
        .boxed()
    }

    fn handle(
        &mut self,
        handler: HandlerType,
        controller_id: pointer::ControllerId,
        config: pointer::Config,
        storage: pointer::Storage,
        event_emitter: pointer::EventEmitter,
    ) -> BoxFuture<Result<tlv::Container, tlv::ErrorContainer>> {
        async move {
            match handler {
                HandlerType::Add {
                    pairing_id,
                    ltpk,
                    permissions,
                } => match handle_add(
                    controller_id,
                    config,
                    storage,
                    event_emitter,
                    pairing_id,
                    ltpk,
                    permissions,
                )
                .await
                {
                    Ok(res) => Ok(res),
                    Err(err) => Err(tlv::ErrorContainer::new(StepNumber::Res as u8, err)),
                },
                HandlerType::Remove { pairing_id } => {
                    match handle_remove(controller_id, storage, event_emitter, pairing_id).await {
                        Ok(res) => Ok(res),
                        Err(err) => Err(tlv::ErrorContainer::new(StepNumber::Res as u8, err)),
                    }
                },
                HandlerType::List => match handle_list(controller_id, storage).await {
                    Ok(res) => Ok(res),
                    Err(err) => Err(tlv::ErrorContainer::new(StepNumber::Res as u8, err)),
                },
            }
        }
        .boxed()
    }
}

async fn handle_add(
    controller_id: pointer::ControllerId,
    config: pointer::Config,
    storage: pointer::Storage,
    event_emitter: pointer::EventEmitter,
    pairing_id: Vec<u8>,
    ltpk: Vec<u8>,
    permissions: Permissions,
) -> Result<tlv::Container, tlv::Error> {
    info!("pairings M1: received add pairing request");

    check_admin(&controller_id, &storage).await?;

    let uuid_str = str::from_utf8(&pairing_id)?;
    let pairing_uuid = Uuid::parse_str(uuid_str)?;

    let mut s = storage.lock().await;
    match s.load_pairing(&pairing_uuid).await {
        Ok(mut pairing) => {
            if ed25519_dalek::PublicKey::from_bytes(&pairing.public_key)?
                != ed25519_dalek::PublicKey::from_bytes(&ltpk)?
            {
                return Err(tlv::Error::Unknown);
            }
            pairing.permissions = permissions;
            pairing.identifier = Some(uuid_str.to_string());
            s.save_pairing(&pairing).await?;

            drop(s);

            event_emitter
                .lock()
                .await
                .emit(&Event::ControllerPaired { id: pairing.id })
                .await;
        },
        Err(_) => {
            if let Some(max_peers) = config.lock().await.max_peers {
                if s.count_pairings().await? + 1 > max_peers {
                    return Err(tlv::Error::MaxPeers);
                }
            }

            let mut public_key = [0; 32];
            public_key.clone_from_slice(&ltpk);
            let pairing = Pairing::new(pairing_uuid, uuid_str.to_string(), permissions, public_key);
            s.save_pairing(&pairing).await?;

            drop(s);

            event_emitter
                .lock()
                .await
                .emit(&Event::ControllerPaired { id: pairing.id })
                .await;
        },
    }

    info!("pairings M2: sending add pairing response");

    Ok(vec![Value::State(StepNumber::Res as u8)])
}

async fn handle_remove(
    controller_id: pointer::ControllerId,
    storage: pointer::Storage,
    event_emitter: pointer::EventEmitter,
    pairing_id: Vec<u8>,
) -> Result<tlv::Container, tlv::Error> {
    info!("pairings M1: received remove pairing request");

    check_admin(&controller_id, &storage).await?;

    let uuid_str = str::from_utf8(&pairing_id)?;
    let pairing_uuid = Uuid::parse_str(uuid_str)?;
    // The storage lock must be released before the event: its listeners use the storage.
    let removed = storage.lock().await.delete_pairing(&pairing_uuid).await;
    match removed {
        Ok(()) => {
            event_emitter
                .lock()
                .await
                .emit(&Event::ControllerUnpaired { id: pairing_uuid })
                .await;
        },
        // Removing a pairing that does not exist succeeds; nothing changed.
        Err(error) if error.is_not_found() => {
            info!("pairings M1: pairing {} to remove does not exist", uuid_str);
        },
        Err(error) => return Err(error.into()),
    }

    info!("pairings M2: sending remove pairing response");

    Ok(vec![Value::State(StepNumber::Res as u8)])
}

async fn handle_list(
    controller_id: pointer::ControllerId,
    storage: pointer::Storage,
) -> Result<tlv::Container, tlv::Error> {
    info!("pairings M1: received list pairings request");

    check_admin(&controller_id, &storage).await?;

    let pairings = storage.lock().await.list_pairings().await?;
    let mut list = vec![Value::State(StepNumber::Res as u8)];
    for (i, pairing) in pairings.iter().enumerate() {
        if i > 0 {
            list.push(Value::Separator);
        }
        list.push(Value::Identifier(pairing.identifier()));
        list.push(Value::PublicKey(pairing.public_key.to_vec()));
        list.push(Value::Permissions(pairing.permissions.clone()));
    }

    info!("pairings M2: sending list pairings response");

    Ok(list)
}

async fn check_admin(controller_id: &pointer::ControllerId, storage: &pointer::Storage) -> Result<(), tlv::Error> {
    let controller_id: Uuid = controller_id
        .read()
        .unwrap()
        .deref()
        .ok_or(tlv::Error::Authentication)?;
    match storage.lock().await.load_pairing(&controller_id).await {
        Err(_) => Err(tlv::Error::Authentication),
        Ok(controller) => match controller.permissions {
            Permissions::Admin => Ok(()),
            _ => Err(tlv::Error::Authentication),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::lock::Mutex;
    use std::sync::{Arc, RwLock};

    use crate::{
        event::EventEmitter,
        storage::{FileStorage, Storage},
        tlv::Encodable,
        Config,
    };

    const IPHONE: &str = "6F0E1A2B-3C4D-4E5F-8A9B-0C1D2E3F4A5B";
    const APPLE_TV: &str = "A1B2C3D4-E5F6-4789-9ABC-DEF012345678";

    /// The Home app adds a home hub's pairing and then compares the listed
    /// identifiers with its own; it removes the ones it does not recognise.
    #[tokio::test]
    async fn lists_pairings_as_added() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/hap-test-pairings");
        let _ = std::fs::remove_dir_all(&dir);
        let mut storage = FileStorage::new(&dir).await.unwrap();
        // An iPhone paired before identifiers were kept.
        let iphone = Pairing::from_bytes(
            format!(
                r#"{{"id":"{}","permissions":"0x01","public_key":[{}]}}"#,
                IPHONE.to_lowercase(),
                ["1"; 32].join(",")
            )
            .as_bytes(),
        )
        .unwrap();
        storage.save_pairing(&iphone).await.unwrap();

        let storage: pointer::Storage = Arc::new(Mutex::new(Box::new(storage)));
        let controller_id: pointer::ControllerId = Arc::new(RwLock::new(Some(iphone.id)));
        let config: pointer::Config = Arc::new(Mutex::new(Config::default()));
        let event_emitter: pointer::EventEmitter = Arc::new(Mutex::new(EventEmitter::new()));

        handle_add(
            controller_id.clone(),
            config,
            storage.clone(),
            event_emitter,
            APPLE_TV.as_bytes().to_vec(),
            vec![2; 32],
            Permissions::Admin,
        )
        .await
        .unwrap();
        let listed = handle_list(controller_id, storage).await.unwrap().encode();

        let identifiers: Vec<String> = listed
            .windows(2 + IPHONE.len())
            .filter(|w| w[0] == Type::Identifier as u8 && w[1] as usize == IPHONE.len())
            .map(|w| String::from_utf8_lossy(&w[2..]).into_owned())
            .collect();
        let mut identifiers = identifiers;
        identifiers.sort();
        assert_eq!(identifiers, vec![IPHONE.to_string(), APPLE_TV.to_string()]);

        // One empty separator between the two pairings, none after the last.
        let separators = listed.windows(2).filter(|w| w == &[Type::Separator as u8, 0]).count();
        assert_eq!(separators, 1);
        assert!(!listed.ends_with(&[Type::Separator as u8, 0]));
    }

    /// The Home app removes a home hub's pairing again when it cleans up,
    /// also when the accessory never had it.
    #[tokio::test]
    async fn removing_a_missing_pairing_succeeds() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/hap-test-remove");
        let _ = std::fs::remove_dir_all(&dir);
        let mut storage = FileStorage::new(&dir).await.unwrap();
        let iphone_id = Uuid::parse_str(IPHONE).unwrap();
        let iphone = Pairing::new(iphone_id, IPHONE.to_string(), Permissions::Admin, [1; 32]);
        storage.save_pairing(&iphone).await.unwrap();

        let storage: pointer::Storage = Arc::new(Mutex::new(Box::new(storage)));
        let controller_id: pointer::ControllerId = Arc::new(RwLock::new(Some(iphone_id)));
        let event_emitter: pointer::EventEmitter = Arc::new(Mutex::new(EventEmitter::new()));

        let response = handle_remove(controller_id, storage.clone(), event_emitter, APPLE_TV.as_bytes().to_vec())
            .await
            .unwrap();

        assert_eq!(response.encode(), vec![Value::State(StepNumber::Res as u8)].encode());
        assert_eq!(storage.lock().await.count_pairings().await.unwrap(), 1, "the iPhone stays paired");
    }

    /// The server's listener counts the pairings when one is removed, so the
    /// handler must not hold the storage while it sends the event.
    #[tokio::test]
    async fn removing_a_pairing_releases_the_storage_before_the_event() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/hap-test-remove-event");
        let _ = std::fs::remove_dir_all(&dir);
        let mut storage = FileStorage::new(&dir).await.unwrap();
        let iphone_id = Uuid::parse_str(IPHONE).unwrap();
        let apple_tv_id = Uuid::parse_str(APPLE_TV).unwrap();
        storage
            .save_pairing(&Pairing::new(iphone_id, IPHONE.to_string(), Permissions::Admin, [1; 32]))
            .await
            .unwrap();
        storage
            .save_pairing(&Pairing::new(apple_tv_id, APPLE_TV.to_string(), Permissions::Admin, [2; 32]))
            .await
            .unwrap();

        let storage: pointer::Storage = Arc::new(Mutex::new(Box::new(storage)));
        let controller_id: pointer::ControllerId = Arc::new(RwLock::new(Some(iphone_id)));
        let mut emitter = EventEmitter::new();
        let listener_storage = storage.clone();
        emitter.add_listener(Box::new(move |_| {
            let storage = listener_storage.clone();
            Box::pin(async move {
                storage.lock().await.count_pairings().await.unwrap();
            })
        }));
        let event_emitter: pointer::EventEmitter = Arc::new(Mutex::new(emitter));

        let removal = handle_remove(controller_id, storage.clone(), event_emitter, APPLE_TV.as_bytes().to_vec());
        tokio::time::timeout(std::time::Duration::from_secs(5), removal)
            .await
            .expect("removing a pairing deadlocks")
            .unwrap();

        assert_eq!(storage.lock().await.count_pairings().await.unwrap(), 1);
    }
}
