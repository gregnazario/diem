// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    application::{
        db::PeerDb,
        types::{PeerError, PeerInfo, PeerState},
    },
    protocols::network::{Message, RpcError},
    ProtocolId,
};
use async_trait::async_trait;
use diem_types::PeerId;
use std::{
    collections::{hash_map::Entry, HashMap},
    time::Duration,
};

/// A generic `NetworkInterface` for applications to connect to networking
#[async_trait]
pub trait NetworkInterface<Request: Message + Send, Response> {
    fn db(&self) -> &PeerDb;

    /// Retrieve only active peers
    fn active_peers(&self) -> HashMap<PeerId, PeerInfo> {
        self.db()
            .read_all()
            .into_iter()
            .filter(|(_, peer_info)| peer_info.status == PeerState::Healthy)
            .collect()
    }

    /// Retrieve PeerInfo for the node
    fn peers(&self) -> HashMap<PeerId, PeerInfo> {
        self.db().read_all()
    }

    /// Update score for this protocol
    fn update_score<F: Fn(Option<&mut u32>) -> Result<(), PeerError>>(
        &self,
        peer_id: PeerId,
        protocol: ProtocolId,
        update: F,
    ) -> Result<(), PeerError> {
        self.db().write(peer_id, |entry| match entry {
            Entry::Vacant(..) => Err(PeerError::NotFound),
            Entry::Occupied(ref mut inner) => {
                let inner = inner.get_mut();
                let cur_score = inner.scores.get_mut(&protocol);
                update(cur_score)?;
                Ok(())
            }
        })
    }

    /// Send an RPC message on the network
    async fn send_rpc(
        &mut self,
        protocol_id: ProtocolId,
        peer_id: PeerId,
        msg: Request,
        timeout: Duration,
    ) -> Result<Response, RpcError>;
}

/// Trait around changing state of peers
pub trait PeerStateChange {
    fn db(&self) -> &PeerDb;

    /// Update state in `PeerDb`
    fn update_state(&self, peer_id: PeerId, state: PeerState) -> Result<(), PeerError> {
        self.db().write(peer_id, |entry| match entry {
            Entry::Vacant(..) => Err(PeerError::NotFound),
            Entry::Occupied(inner) => {
                inner.get_mut().status = state;
                Ok(())
            }
        })
    }
}
