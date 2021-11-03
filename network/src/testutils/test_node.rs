// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    application::storage::PeerMetadataStorage,
    peer_manager::{ConnectionNotification, PeerManagerNotification, PeerManagerRequest},
    protocols::{direct_send::Message, rpc::InboundRpcRequest, wire::handshake::v1::ProtocolIdSet},
    transport::ConnectionMetadata,
    DisconnectReason, ProtocolId,
};
use diem_config::{
    config::{PeerRole, RoleType},
    network_id::{NetworkContext, NetworkId, PeerNetworkId},
};
use diem_types::PeerId;
use futures::StreamExt;
use netcore::transport::ConnectionOrigin;
use std::{collections::HashMap, sync::Arc};

pub type InboundMessageSender =
    channel::diem_channel::Sender<(PeerId, ProtocolId), PeerManagerNotification>;
pub type ConnectionUpdateSender = crate::peer_manager::conn_notifs_channel::Sender;
pub type OutboundMessageReceiver =
    channel::diem_channel::Receiver<(PeerId, ProtocolId), PeerManagerRequest>;

/// A connection handle describing the network for a node
/// Use this to interact with the node
#[derive(Clone)]
pub struct InboundNetworkHandle {
    /// To send new incoming network messages
    pub inbound_message_sender: InboundMessageSender,
    /// To send new incoming connections or disconnections
    pub connection_update_sender: ConnectionUpdateSender,
    /// To update the local state (normally done by peer manager)
    pub peer_metadata_storage: Arc<PeerMetadataStorage>,
}

impl InboundNetworkHandle {
    /// Push connection update, and update the local storage
    pub fn connect(
        &self,
        self_peer_id: PeerId,
        network_id: NetworkId,
        conn_metadata: ConnectionMetadata,
    ) {
        // PeerManager pushes this data before it's received by events
        self.peer_metadata_storage
            .insert_connection(network_id, conn_metadata.clone());
        self.connection_update_sender
            .push(
                conn_metadata.remote_peer_id,
                ConnectionNotification::NewPeer(
                    conn_metadata,
                    // TODO will `RoleType` matter
                    NetworkContext::new(RoleType::Validator, network_id, self_peer_id),
                ),
            )
            .unwrap();
    }

    /// Push disconnect update, and update the local storage
    pub fn disconnect(
        &self,
        self_peer_id: PeerId,
        network_id: NetworkId,
        conn_metadata: ConnectionMetadata,
    ) {
        // PeerManager pushes this data before it's received by events
        self.peer_metadata_storage.remove(&PeerNetworkId::new(
            network_id,
            conn_metadata.remote_peer_id,
        ));
        self.connection_update_sender
            .push(
                conn_metadata.remote_peer_id,
                ConnectionNotification::LostPeer(
                    conn_metadata,
                    // TODO will `RoleType` matter
                    NetworkContext::new(RoleType::Validator, network_id, self_peer_id),
                    DisconnectReason::ConnectionLost,
                ),
            )
            .unwrap();
    }
}

/// An application specific network handle
pub type ApplicationNetworkHandle<Sender, Events> = (NetworkId, Sender, Events);

/// A unique identifier of a node across the entire network
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct NodeId {
    pub owner: u32,
    pub node_type: NodeType,
}

impl NodeId {
    pub fn validator(owner: u32) -> NodeId {
        NodeId {
            owner,
            node_type: NodeType::Validator,
        }
    }

    pub fn vfn(owner: u32) -> NodeId {
        NodeId {
            owner,
            node_type: NodeType::ValidatorFullNode,
        }
    }

    pub fn pfn(owner: u32) -> NodeId {
        NodeId {
            owner,
            node_type: NodeType::PublicFullNode,
        }
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{:?}", self.owner, self.node_type)
    }
}

/// An enum defining the type of node
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum NodeType {
    Validator,
    ValidatorFullNode,
    PublicFullNode,
}

/// A trait defining an application specific node with networking abstracted
///
/// This is built as an abstract implementation of networking around a node
pub trait TestNode {
    fn node_id(&self) -> NodeId;

    fn node_type(&self) -> NodeType;

    /// For sending to this node.  Generally should not be used after setup
    fn node_inbound_handle(&self, network_id: NetworkId) -> InboundNetworkHandle;

    fn add_other_inbound_handle(
        &mut self,
        peer_network_id: PeerNetworkId,
        handle: InboundNetworkHandle,
    );

    /// For sending to other nodes
    fn other_inbound_handle(&self, peer_network_id: PeerNetworkId) -> InboundNetworkHandle;

    /// For receiving messages from other nodes
    fn outbound_handle(&mut self, network_id: NetworkId) -> &mut OutboundMessageReceiver;

    fn peer_metadata_storage(&self) -> &PeerMetadataStorage;

    fn peer_network_ids(&self) -> &HashMap<NetworkId, PeerNetworkId>;

    fn peer_network_id(&self, network_id: NetworkId) -> PeerNetworkId {
        *self.peer_network_ids().get(&network_id).unwrap_or_else(|| {
            panic!(
                "Expected network {} to exist on node {}",
                network_id,
                self.node_id()
            )
        })
    }

    fn network_ids(&self) -> Vec<NetworkId> {
        self.peer_network_ids().keys().copied().collect()
    }

    fn role(&self) -> RoleType {
        match self.node_id().node_type {
            NodeType::Validator => RoleType::Validator,
            _ => RoleType::FullNode,
        }
    }

    fn peer_role(&self) -> PeerRole {
        match self.node_id().node_type {
            NodeType::Validator => PeerRole::Validator,
            NodeType::ValidatorFullNode => PeerRole::ValidatorFullNode,
            NodeType::PublicFullNode => PeerRole::Unknown,
        }
    }

    /// Connects a node to another node.  The other's inbound handle must already be added.
    fn connect(&self, network_id: NetworkId, metadata: ConnectionMetadata) {
        let self_peer_id = self.peer_network_id(network_id).peer_id();
        let self_metadata = self.conn_metadata(network_id, ConnectionOrigin::Inbound, None);
        let remote_peer_id = metadata.remote_peer_id;
        // Tell the other node it's good to send to the connected peer now
        self.other_inbound_handle(PeerNetworkId::new(network_id, remote_peer_id))
            .connect(remote_peer_id, network_id, self_metadata);
        // Then connect us
        self.node_inbound_handle(network_id)
            .connect(self_peer_id, network_id, metadata);
    }

    /// Disconnects a node from another node
    fn disconnect(&self, network_id: NetworkId, metadata: ConnectionMetadata) {
        let self_peer_id = self.peer_network_id(network_id).peer_id();
        let self_metadata = self.conn_metadata(network_id, ConnectionOrigin::Inbound, None);
        let remote_peer_id = metadata.remote_peer_id;
        // Tell the other node it's disconnected
        self.other_inbound_handle(PeerNetworkId::new(network_id, remote_peer_id))
            .disconnect(remote_peer_id, network_id, self_metadata);
        // Then disconnect us
        self.node_inbound_handle(network_id)
            .disconnect(self_peer_id, network_id, metadata);
    }

    /// Find a common network between nodes, they're sorted in priority order
    fn find_common_network(&self, other: &Self) -> Option<NetworkId> {
        match self.node_type() {
            NodeType::Validator => match other.node_type() {
                NodeType::Validator => Some(NetworkId::Validator),
                NodeType::ValidatorFullNode => Some(NetworkId::Vfn),
                NodeType::PublicFullNode => None,
            },
            NodeType::ValidatorFullNode => match other.node_type() {
                NodeType::Validator => Some(NetworkId::Vfn),
                _ => Some(NetworkId::Public),
            },
            NodeType::PublicFullNode => match other.node_type() {
                NodeType::Validator => None,
                _ => Some(NetworkId::Public),
            },
        }
    }

    /// Build `ConnectionMetadata` for a connection on another node
    fn conn_metadata(
        &self,
        network_id: NetworkId,
        origin: ConnectionOrigin,
        maybe_protocols: Option<ProtocolIdSet>,
    ) -> ConnectionMetadata {
        let peer_network_id = self.peer_network_id(network_id);
        let mut metadata = ConnectionMetadata::mock_with_role_and_origin(
            peer_network_id.peer_id(),
            self.peer_role(),
            origin,
        );
        if let Some(protocols) = maybe_protocols {
            metadata.application_protocols = protocols;
        } else {
            metadata.application_protocols = ProtocolIdSet::all_known()
        }
        metadata
    }
}

/// Drops the next queued network message on `Node`'s network (`NetworkId`).  Doesn't propagate
/// to downstream node
pub async fn drop_next_network_msg<Node: TestNode>(node: &mut Node, network_id: NetworkId) -> PeerManagerRequest {
    let outbound_receiver = node.outbound_handle(network_id);
    outbound_receiver.next().await.expect("Expecting a message")
}

/// Sends the next queued network message on `Node`'s network (`NetworkId`)
pub async fn send_next_network_msg<Node: TestNode>(node: &mut Node, network_id: NetworkId) {
    let outbound_receiver = node.outbound_handle(network_id);
    let request = outbound_receiver.next().await.expect("Expecting a message");

    let (remote_peer_id, protocol_id, data, maybe_rpc_info) = match request {
        PeerManagerRequest::SendRpc(peer_id, msg) => (
            peer_id,
            msg.protocol_id,
            msg.data,
            Some((msg.timeout, msg.res_tx)),
        ),
        PeerManagerRequest::SendDirectSend(peer_id, msg) => {
            (peer_id, msg.protocol_id, msg.mdata, None)
        }
    };

    let sender_peer_network_id = node.peer_network_id(network_id);
    let receiver_peer_network_id = PeerNetworkId::new(network_id, remote_peer_id);
    let receiver_handle = node.other_inbound_handle(receiver_peer_network_id);
    let sender_peer_id = sender_peer_network_id.peer_id();

    // TODO: Add timeout functionality
    let peer_manager_notif = if let Some((_timeout, res_tx)) = maybe_rpc_info {
        PeerManagerNotification::RecvRpc(
            sender_peer_id,
            InboundRpcRequest {
                protocol_id,
                data,
                res_tx,
            },
        )
    } else {
        PeerManagerNotification::RecvMessage(
            sender_peer_id,
            Message {
                protocol_id,
                mdata: data,
            },
        )
    };
    println!("{} -> {}", sender_peer_network_id, receiver_peer_network_id);
    receiver_handle
        .inbound_message_sender
        .push((sender_peer_id, protocol_id), peer_manager_notif)
        .unwrap();
}
