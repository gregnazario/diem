// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    application::{
        db::PeerDb,
        interface::{NetworkInterface, PeerStateChange},
        management::PeerManagementInterface,
        types::PeerState,
    },
    error::NetworkError,
    protocols::{
        health_checker::{HealthCheckerMsg, HealthCheckerNetworkSender},
        network::RpcError,
    },
    ProtocolId,
};
use async_trait::async_trait;
use diem_types::PeerId;
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub struct HealthCheckNetworkInterface {
    peer_db: Arc<PeerDb>,
    sender: HealthCheckerNetworkSender,
}

impl HealthCheckNetworkInterface {
    pub fn new(peer_db: Arc<PeerDb>, sender: HealthCheckerNetworkSender) -> Self {
        Self { peer_db, sender }
    }

    pub fn sender(&self) -> HealthCheckerNetworkSender {
        self.sender.clone()
    }

    pub async fn disconnect_peer(&mut self, peer_id: PeerId) -> Result<(), NetworkError> {
        // Possibly already disconnected, but try anyways
        let _ = self.update_state(peer_id, PeerState::Disconnecting);
        let result = self.sender.disconnect_peer(peer_id).await;
        if result.is_ok() {
            let _ = self.remove_peer(peer_id);
        }
        result
    }
}

#[async_trait]
impl NetworkInterface<HealthCheckerMsg, HealthCheckerMsg> for HealthCheckNetworkInterface {
    fn db(&self) -> &PeerDb {
        &self.peer_db
    }

    async fn send_rpc(
        &mut self,
        _protocol_id: ProtocolId,
        _peer_id: PeerId,
        _msg: HealthCheckerMsg,
        _timeout: Duration,
    ) -> Result<HealthCheckerMsg, RpcError> {
        todo!()
    }
}

impl PeerStateChange for HealthCheckNetworkInterface {
    fn db(&self) -> &PeerDb {
        &self.peer_db
    }
}

impl PeerManagementInterface for HealthCheckNetworkInterface {
    fn db(&self) -> &PeerDb {
        &self.peer_db
    }
}
