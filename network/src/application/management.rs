// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    application::{db::PeerDb, interface::PeerStateChange, types::PeerError},
    transport::ConnectionMetadata,
};
use diem_types::PeerId;
use std::sync::Arc;

pub trait PeerManagementInterface {
    fn db(&self) -> &PeerDb;

    /// Create a new peer, should be done by `PeerManager`
    fn new_connection(&self, connection_metadata: ConnectionMetadata) {
        self.db()
            .insert(connection_metadata.remote_peer_id, connection_metadata)
    }

    /// Removes a peer, should be done by `PeerManager`
    fn remove_peer(&self, peer_id: PeerId) -> Result<(), PeerError> {
        self.db().remove(peer_id)
    }
}

pub struct PeerDbManagementInterface {
    db: Arc<PeerDb>,
}

impl PeerDbManagementInterface {
    pub fn new(db: Arc<PeerDb>) -> Self {
        PeerDbManagementInterface { db }
    }
}

impl PeerManagementInterface for PeerDbManagementInterface {
    fn db(&self) -> &PeerDb {
        &self.db
    }
}

impl PeerStateChange for PeerDbManagementInterface {
    fn db(&self) -> &PeerDb {
        &self.db
    }
}
