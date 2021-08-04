// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{event::EventHandle, PeerId};
use diem_crypto::x25519;
use move_core_types::{
    ident_str,
    identifier::IdentStr,
    move_resource::{MoveResource, MoveStructType},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkIdentity {
    pub peer_id: PeerId,
    pub pubkeys: Vec<x25519::PublicKey>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkIdentityResource {
    identities: Vec<NetworkIdentity>,
}

impl NetworkIdentityResource {
    pub fn identities(&self) -> &[NetworkIdentity] {
        &self.identities
    }
}

impl MoveStructType for NetworkIdentityResource {
    const MODULE_NAME: &'static IdentStr = ident_str!("NetworkIdentity");
    const STRUCT_NAME: &'static IdentStr = ident_str!("NetworkIdentity");
}

impl MoveResource for NetworkIdentityResource {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkIdentityEventHandle {
    identity_change_events: EventHandle,
}

impl NetworkIdentityEventHandle {
    pub fn identity_change_events(&self) -> &EventHandle {
        &self.identity_change_events
    }
}

impl MoveStructType for NetworkIdentityEventHandle {
    const MODULE_NAME: &'static IdentStr = ident_str!("NetworkIdentity");
    const STRUCT_NAME: &'static IdentStr = ident_str!("NetworkIdentityEventHandle");
}

impl MoveResource for NetworkIdentityEventHandle {}
