// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    application::storage::PeerMetadataStorage,
    peer_manager::{ConnectionNotification, PeerManagerNotification, PeerManagerRequest},
    protocols::wire::handshake::v1::ProtocolIdSet,
    transport::ConnectionMetadata,
    DisconnectReason, ProtocolId,
};
use diem_config::{
    config::{PeerRole, RoleType},
    network_id::{NetworkContext, NetworkId, PeerNetworkId},
};
use diem_types::PeerId;
use netcore::transport::ConnectionOrigin;
use std::collections::HashMap;

/// A connection handle describing the network for a node
/// Use this to interact with the node
pub struct NodeNetworkHandle {
    /// To send new incoming network messages
    pub incoming_message_sender:
        channel::diem_channel::Sender<(PeerId, ProtocolId), PeerManagerNotification>,
    /// To send new incoming connections or disconnections
    pub connection_update_sender: crate::peer_manager::conn_notifs_channel::Sender,
    /// Outgoing messages from the node
    pub outgoing_messages:
        channel::diem_channel::Receiver<(PeerId, ProtocolId), PeerManagerRequest>,
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

    fn node_network_handles(&self) -> &HashMap<NetworkId, NodeNetworkHandle>;

    fn node_network_handles_mut(&mut self) -> &mut HashMap<NetworkId, NodeNetworkHandle>;

    fn node_type(&self) -> NodeType;

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

    fn connect(&self, network_id: NetworkId, metadata: ConnectionMetadata) {
        let self_peer_id = self.peer_network_id(network_id).peer_id();
        let remote_peer_id = metadata.remote_peer_id;

        let network_context = NetworkContext::new(self.role(), network_id, self_peer_id);
        let notif = ConnectionNotification::NewPeer(metadata.clone(), network_context);
        self.peer_metadata_storage()
            .insert_connection(network_id, metadata);
        let network_handle = self
            .node_network_handles()
            .get(&network_id)
            .unwrap_or_else(|| {
                panic!(
                    "Expected network {} to exist on node {}",
                    network_id,
                    self.node_id()
                )
            });
        network_handle
            .connection_update_sender
            .push(remote_peer_id, notif)
            .unwrap();
    }

    fn disconnect(&self, network_id: NetworkId, metadata: ConnectionMetadata) {
        let self_peer_id = self.peer_network_id(network_id).peer_id();
        let remote_peer_id = metadata.remote_peer_id;

        let network_context = NetworkContext::new(self.role(), network_id, self_peer_id);
        let notif = ConnectionNotification::LostPeer(
            metadata,
            network_context,
            DisconnectReason::ConnectionLost,
        );
        self.peer_metadata_storage()
            .remove(&PeerNetworkId::new(network_id, remote_peer_id));
        let network_handle = self
            .node_network_handles()
            .get(&network_id)
            .unwrap_or_else(|| {
                panic!(
                    "Expected network {} to exist on node {}",
                    network_id,
                    self.node_id()
                )
            });
        network_handle
            .connection_update_sender
            .push(remote_peer_id, notif)
            .unwrap();
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
