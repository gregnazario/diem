// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    application::storage::PeerMetadataStorage,
    peer_manager::{ConnectionRequestSender, PeerManagerRequestSender},
    protocols::network::{NewNetworkEvents, NewNetworkSender},
    testutils::test_node::{ApplicationNetworkHandle, NodeId, NodeNetworkHandle, TestNode},
};
use channel::message_queues::QueueStyle;
use diem_config::{
    config::NodeConfig,
    network_id::{NetworkId, PeerNetworkId},
};
use netcore::transport::ConnectionOrigin;
use std::{collections::HashMap, hash::Hash, sync::Arc, vec::Vec};

/// A trait describing a test framework for a specific application
///
/// This is essentially an abstract implementation, to get around how rust handles traits
/// there are functions to get required variables in the implementation.
///
pub trait TestFramework<Node: TestNode + Sync> {
    fn new(
        nodes: HashMap<NodeId, Node>,
        peer_network_id_to_node: HashMap<PeerNetworkId, NodeId>,
    ) -> Self;

    fn nodes(&self) -> &HashMap<NodeId, Node>;

    fn nodes_mut(&mut self) -> &mut HashMap<NodeId, Node>;

    fn build_node(node_id: NodeId, config: NodeConfig, peer_network_ids: &[PeerNetworkId]) -> Node;

    fn node(&self, node_id: NodeId) -> &Node {
        self.nodes()
            .get(&node_id)
            .unwrap_or_else(|| panic!("Expect node {} to exist", node_id))
    }

    fn node_mut(&mut self, node_id: NodeId) -> &mut Node {
        self.nodes_mut()
            .get_mut(&node_id)
            .unwrap_or_else(|| panic!("Expect node {} to exist", node_id))
    }

    /// Find a common network between nodes, they're sorted in priority order
    fn find_common_network(&self, node_a_id: NodeId, node_b_id: NodeId) -> NetworkId {
        let node_a = self.node(node_a_id);
        let node_b = self.node(node_b_id);
        let b_network_ids = node_b.network_ids();
        let mut network_ids: Vec<_> = node_a
            .network_ids()
            .into_iter()
            .filter(|network_id| b_network_ids.contains(network_id))
            .collect();
        network_ids.sort();
        *network_ids.first().unwrap_or_else(|| {
            panic!(
                "There must be an intersection between nodes {} {}",
                node_a_id, node_b_id
            )
        })
    }

    /// Connects `Dialer` to `Listener`
    fn connect(&mut self, dialer: NodeId, listener: NodeId) {
        // The nodes should exist
        assert!(self.nodes().contains_key(&dialer) && self.nodes().contains_key(&listener));
        let network = self.find_common_network(dialer, listener);
        let listener_peer_network_id = self.node(listener).peer_network_id(network);
        if let Some(info) = self
            .node(dialer)
            .peer_metadata_storage()
            .read(listener_peer_network_id)
        {
            panic!("Already connected {} {} ({:?})", dialer, listener, info)
        }

        let listener_metadata =
            self.node(listener)
                .conn_metadata(network, ConnectionOrigin::Outbound, None);
        let dialer_metadata =
            self.node(dialer)
                .conn_metadata(network, ConnectionOrigin::Inbound, None);

        self.node_mut(listener).connect(network, dialer_metadata);
        self.node_mut(dialer).connect(network, listener_metadata);
    }

    /// Disconnects `Node A` and `Node B`
    fn disconnect(&mut self, node_a: NodeId, node_b: NodeId) {
        // The nodes should exist
        assert!(self.nodes().contains_key(&node_a) && self.nodes().contains_key(&node_b));
        let network = self.find_common_network(node_a, node_b);
        let node_a_peer_network_id = self.node(node_a).peer_network_id(network);
        if self
            .node(node_b)
            .peer_metadata_storage()
            .read(node_a_peer_network_id)
            .is_none()
        {
            panic!("Not connected {} {}", node_a, node_b);
        }

        let node_a_metadata =
            self.node(node_a)
                .conn_metadata(network, ConnectionOrigin::Outbound, None);
        let node_b_metadata =
            self.node(node_b)
                .conn_metadata(network, ConnectionOrigin::Inbound, None);

        self.node_mut(node_a).disconnect(network, node_b_metadata);
        self.node_mut(node_b).disconnect(network, node_a_metadata);
    }
}

/// Setup the multiple networks built for a specific node
pub fn setup_node_networks<NetworkSender: NewNetworkSender, NetworkEvents: NewNetworkEvents>(
    network_ids: &[NetworkId],
) -> (
    Vec<ApplicationNetworkHandle<NetworkSender, NetworkEvents>>,
    HashMap<NetworkId, NodeNetworkHandle>,
    Arc<PeerMetadataStorage>,
) {
    let mut reqs_handles = Vec::new();
    let mut conns_handles = HashMap::new();

    let peer_metadata_storage = PeerMetadataStorage::new(network_ids);

    // Build each individual network
    for network_id in network_ids {
        let (reqs_handle, conns_handle) = setup_network(*network_id);
        reqs_handles.push(reqs_handle);
        conns_handles.insert(*network_id, conns_handle);
    }

    (reqs_handles, conns_handles, peer_metadata_storage)
}

/// Builds all the channels used for networking
fn setup_network<NetworkSender: NewNetworkSender, NetworkEvents: NewNetworkEvents>(
    network_id: NetworkId,
) -> (
    ApplicationNetworkHandle<NetworkSender, NetworkEvents>,
    NodeNetworkHandle,
) {
    let (reqs_inbound_sender, reqs_inbound_receiver) = diem_channel();
    let (reqs_outbound_sender, reqs_outbound_receiver) = diem_channel();
    let (connection_outbound_sender, _connection_outbound_receiver) = diem_channel();
    let (connection_inbound_sender, connection_inbound_receiver) =
        crate::peer_manager::conn_notifs_channel::new();
    let network_sender = NetworkSender::new(
        PeerManagerRequestSender::new(reqs_outbound_sender),
        ConnectionRequestSender::new(connection_outbound_sender),
    );
    let network_events = NetworkEvents::new(reqs_inbound_receiver, connection_inbound_receiver);

    (
        (network_id, network_sender, network_events),
        NodeNetworkHandle {
            incoming_message_sender: reqs_inbound_sender,
            connection_update_sender: connection_inbound_sender,
            outgoing_messages: reqs_outbound_receiver,
        },
    )
}

/// A generic FIFO diem channel
fn diem_channel<K: Eq + Hash + Clone, T>() -> (
    channel::diem_channel::Sender<K, T>,
    channel::diem_channel::Receiver<K, T>,
) {
    static MAX_QUEUE_SIZE: usize = 8;
    channel::diem_channel::new(QueueStyle::FIFO, MAX_QUEUE_SIZE, None)
}
