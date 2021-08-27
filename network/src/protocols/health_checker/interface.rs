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
        health_checker::{
            HealthCheckerMsg, HealthCheckerNetworkEvents, HealthCheckerNetworkSender,
        },
        network::{Event, RpcError},
    },
    ProtocolId,
};
use async_trait::async_trait;
use diem_types::PeerId;
use futures::{stream::FusedStream, Stream};
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

pub struct HealthCheckNetworkInterface {
    peer_db: Arc<PeerDb>,
    sender: HealthCheckerNetworkSender,
    receiver: HealthCheckerNetworkEvents,
}

impl HealthCheckNetworkInterface {
    pub fn new(
        peer_db: Arc<PeerDb>,
        sender: HealthCheckerNetworkSender,
        receiver: HealthCheckerNetworkEvents,
    ) -> Self {
        Self {
            peer_db,
            sender,
            receiver,
        }
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

impl Stream for HealthCheckNetworkInterface {
    type Item = Event<HealthCheckerMsg>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().receiver).poll_next(cx)
    }
}

impl FusedStream for HealthCheckNetworkInterface {
    fn is_terminated(&self) -> bool {
        self.receiver.is_terminated()
    }
}
