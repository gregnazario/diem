// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    application::{
        interface::{ApplicationPeerNetworkIdSender, NetworkInterface},
        storage::{LockingHashMap, PeerMetadataStorage},
        types::{PeerError, PeerState},
    },
    error::NetworkError,
    protocols::{health_checker::HealthCheckerMsg, network::RpcError},
    transport::ConnectionMetadata,
};
use async_trait::async_trait;
use diem_config::network_id::{NetworkId, PeerNetworkId};
use diem_types::PeerId;
use std::{collections::hash_map::Entry, sync::Arc, time::Duration};

#[derive(Clone)]
struct DummySender {}

#[async_trait]
impl ApplicationPeerNetworkIdSender<HealthCheckerMsg> for DummySender {
    fn send_to(
        &mut self,
        _recipient: PeerNetworkId,
        _message: HealthCheckerMsg,
    ) -> Result<(), NetworkError> {
        unimplemented!()
    }

    fn send_to_many(
        &mut self,
        _recipients: impl Iterator<Item = PeerNetworkId>,
        _message: HealthCheckerMsg,
    ) -> Result<(), NetworkError> {
        unimplemented!()
    }

    async fn send_rpc(
        &mut self,
        _recipient: PeerNetworkId,
        _req_msg: HealthCheckerMsg,
        _timeout: Duration,
    ) -> Result<HealthCheckerMsg, RpcError> {
        unimplemented!()
    }
}

/// Dummy network so we can test the interfaces
struct DummyNetworkInterface {
    peer_metadata_storage: Arc<PeerMetadataStorage>,
    app_data: LockingHashMap<PeerId, usize>,
}

impl NetworkInterface<HealthCheckerMsg, DummySender> for DummyNetworkInterface {
    type AppDataKey = PeerId;
    type AppData = usize;

    fn peer_metadata_storage(&self) -> &PeerMetadataStorage {
        &self.peer_metadata_storage
    }

    fn sender(&self) -> DummySender {
        DummySender {}
    }

    fn insert_app_data(&self, peer_id: PeerId, data: Self::AppData) {
        self.app_data.insert(peer_id, data)
    }

    fn remove_app_data(&self, peer_id: &PeerId) {
        self.app_data.remove(peer_id)
    }

    fn read_app_data(&self, peer_id: &PeerId) -> Option<Self::AppData> {
        self.app_data.read(peer_id)
    }

    fn write_app_data<F: FnOnce(&mut Entry<PeerId, Self::AppData>) -> Result<(), PeerError>>(
        &self,
        peer_id: PeerId,
        modifier: F,
    ) -> Result<(), PeerError> {
        self.app_data.write(peer_id, modifier)
    }
}

#[test]
fn test_interface() {
    let network_id = NetworkId::Validator;
    let peer_metadata_storage = PeerMetadataStorage::test();
    let interface = DummyNetworkInterface {
        peer_metadata_storage: peer_metadata_storage.clone(),
        app_data: LockingHashMap::new(),
    };
    let peer_1 = PeerId::random();
    let peer_2 = PeerId::random();
    assert_eq!(0, interface.peers(network_id).len());
    assert_eq!(0, interface.connected_peers(network_id).len());

    // Insert 2 connections, and we should have two active peers
    let connection_1 = ConnectionMetadata::mock(peer_1);
    let connection_2 = ConnectionMetadata::mock(peer_2);
    peer_metadata_storage.insert_connection(network_id, connection_1);
    peer_metadata_storage.insert_connection(network_id, connection_2.clone());
    assert_eq!(2, interface.peers(network_id).len());
    assert_eq!(2, interface.connected_peers(network_id).len());

    // Disconnecting / disconnected are not counted in active
    update_state(
        peer_metadata_storage.clone(),
        PeerNetworkId::new(network_id, peer_1),
        PeerState::Disconnecting,
    );
    assert_eq!(2, interface.peers(network_id).len());
    assert_eq!(1, interface.connected_peers(network_id).len());

    // Removing a connection with a different connection id doesn't remove it from storage
    let different_connection_2 = ConnectionMetadata::mock(peer_2);
    peer_metadata_storage.remove_connection(network_id, &different_connection_2);
    assert_eq!(2, interface.peers(network_id).len());
    assert_eq!(1, interface.connected_peers(network_id).len());

    // Removing the same connection id removes it
    peer_metadata_storage.remove_connection(network_id, &connection_2);
    assert_eq!(1, interface.peers(network_id).len());
    assert_eq!(0, interface.connected_peers(network_id).len());
}

fn update_state(
    peer_metadata_storage: Arc<PeerMetadataStorage>,
    peer_network_id: PeerNetworkId,
    state: PeerState,
) {
    peer_metadata_storage
        .write(peer_network_id, |entry| match entry {
            Entry::Vacant(..) => Err(PeerError::NotFound),
            Entry::Occupied(inner) => {
                inner.get_mut().status = state;
                Ok(())
            }
        })
        .unwrap()
}
