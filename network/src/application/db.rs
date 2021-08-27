// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0
use crate::{
    application::types::{PeerError, PeerInfo},
    transport::ConnectionMetadata,
};
use diem_infallible::RwLock;
use diem_types::PeerId;
use std::collections::{
    hash_map::{Entry, Entry::Occupied},
    HashMap,
};

/// Structure for storing PeerInfo
/// HashMap based storage
pub struct PeerDb {
    peers: RwLock<HashMap<PeerId, PeerInfo>>,
}

impl PeerDb {
    pub fn new() -> PeerDb {
        PeerDb {
            peers: RwLock::new(HashMap::new()),
        }
    }

    /// Read a copy of the stored `PeerInfo`
    pub fn read(&self, peer_id: &PeerId) -> Option<PeerInfo> {
        self.peers.read().get(peer_id).cloned()
    }

    /// Read a copy of the entire state
    pub fn read_all(&self) -> HashMap<PeerId, PeerInfo> {
        self.peers.read().clone()
    }

    /// Insert new entry
    pub fn insert(&self, peer_id: PeerId, connection_metadata: ConnectionMetadata) {
        let mut peers = self.peers.write();
        let entry = peers
            .entry(peer_id)
            .or_insert_with(|| PeerInfo::new(connection_metadata.clone()));
        entry.connection_metadata = connection_metadata;
    }

    /// Remove old entries
    pub fn remove(&self, peer_id: PeerId) -> Result<(), PeerError> {
        let mut peers = self.peers.write();
        if let Occupied(e) = peers.entry(peer_id) {
            e.remove();
            Ok(())
        } else {
            Err(PeerError::NotFound)
        }
    }

    /// Take in a function to modify the data, must handle concurrency control with the input function
    pub fn write<F: FnOnce(&mut Entry<PeerId, PeerInfo>) -> Result<(), PeerError>>(
        &self,
        peer_id: PeerId,
        modifier: F,
    ) -> Result<(), PeerError> {
        modifier(&mut self.peers.write().entry(peer_id))
    }
}
