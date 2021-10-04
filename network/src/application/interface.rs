// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    application::{
        storage::PeerMetadataStorage,
        types::{PeerError, PeerInfo, PeerState},
    },
    error::NetworkError,
    protocols::network::{ApplicationNetworkSender, Message, RpcError},
};
use async_trait::async_trait;
use diem_config::network_id::{NetworkId, PeerNetworkId};
use diem_types::PeerId;
use itertools::Itertools;
use std::{
    collections::{hash_map::Entry, HashMap},
    marker::PhantomData,
    time::Duration,
};

/// A generic `NetworkInterface` for applications to connect to networking
///
/// Each application would implement their own `NetworkInterface`.  This would hold `AppData` specific
/// to the application as well as a specific `Sender` for cloning across threads and sending requests.
#[async_trait]
pub trait NetworkInterface {
    /// A cloneable sender for sending outbound messages
    type Sender;
    /// The application specific key for `AppData`
    type AppDataKey;
    /// The application specific data to be stored
    type AppData;

    /// Provides the `PeerMetadataStorage` for other functions.  Not expected to be used externally.
    fn peer_metadata_storage(&self) -> &PeerMetadataStorage;

    /// Give a copy of the sender for the network
    fn sender(&self) -> Self::Sender;

    /// Retrieve only connected peers
    fn connected_peers(&self, network_id: NetworkId) -> HashMap<PeerNetworkId, PeerInfo> {
        self.filtered_peers(network_id, |(_, peer_info)| {
            peer_info.status == PeerState::Connected
        })
    }

    /// Filter peers with according `filter`
    fn filtered_peers<F: FnMut(&(&PeerId, &PeerInfo)) -> bool>(
        &self,
        network_id: NetworkId,
        filter: F,
    ) -> HashMap<PeerNetworkId, PeerInfo> {
        self.peer_metadata_storage()
            .read_filtered(network_id, filter)
    }

    /// Retrieve PeerInfo for the node
    fn peers(&self, network_id: NetworkId) -> HashMap<PeerNetworkId, PeerInfo> {
        self.peer_metadata_storage().read_all(network_id)
    }

    /// Insert application specific data
    fn insert_app_data(&self, app_data_key: Self::AppDataKey, data: Self::AppData);

    /// Removes application specific data
    fn remove_app_data(&self, app_data_key: &Self::AppDataKey);

    /// Read application specific data
    fn read_app_data(&self, app_data_key: &Self::AppDataKey) -> Option<Self::AppData>;

    /// Write application specific data, allows for read before write operations
    fn write_app_data<
        F: FnOnce(&mut Entry<Self::AppDataKey, Self::AppData>) -> Result<(), PeerError>,
    >(
        &self,
        app_data_key: Self::AppDataKey,
        modifier: F,
    ) -> Result<(), PeerError>;
}

/// A sender that combines multiple `ApplicationNetworkSender` to send across multiple networks
/// Therefore, sending messages to multiple networks can be routed by just `PeerNetworkId` without
/// having to do lookups for `NetworkId` related senders.  Additionally, will give some checking
/// around attempting to send messages to networks that don't exist.
#[derive(Clone)]
struct MultiNetworkSender<
    TMessage: Message + Send,
    Sender: ApplicationNetworkSender<TMessage> + Send,
> {
    senders: HashMap<NetworkId, Sender>,
    _phantom: PhantomData<TMessage>,
}

impl<TMessage: Message + Send, Sender: ApplicationNetworkSender<TMessage> + Send>
    MultiNetworkSender<TMessage, Sender>
{
    fn sender(&mut self, network_id: &NetworkId) -> &mut Sender {
        self.senders.get_mut(network_id).expect("Unknown NetworkId")
    }
}

#[async_trait]
impl<TMessage: Clone + Message + Send, Sender: ApplicationNetworkSender<TMessage> + Send>
    ApplicationPeerNetworkIdSender<TMessage> for MultiNetworkSender<TMessage, Sender>
{
    fn send_to(&mut self, recipient: PeerNetworkId, message: TMessage) -> Result<(), NetworkError> {
        self.sender(&recipient.network_id())
            .send_to(recipient.peer_id(), message)
    }

    fn send_to_many(
        &mut self,
        recipients: impl Iterator<Item = PeerNetworkId>,
        message: TMessage,
    ) -> Result<(), NetworkError> {
        for (network_id, recipients) in
            &recipients.group_by(|peer_network_id| peer_network_id.network_id())
        {
            let sender = self.sender(&network_id);
            let peer_ids = recipients.map(|peer_network_id| peer_network_id.peer_id());
            sender.send_to_many(peer_ids, message.clone())?;
        }
        Ok(())
    }

    async fn send_rpc(
        &mut self,
        recipient: PeerNetworkId,
        req_msg: TMessage,
        timeout: Duration,
    ) -> Result<TMessage, RpcError> {
        self.sender(&recipient.network_id())
            .send_rpc(recipient.peer_id(), req_msg, timeout)
            .await
    }
}

/// A trait for sending messages using `PeerNetworkId` rather than `NetworkId`.  At the networking
/// level, it only knows about a single `NetworkId`.  But, the applications know about multiple
/// networks, and communicate with `PeerNetworkId` as the uniqueness constraint.  This lets them
/// use that uniqueness constraint and we can hide the details from the applications.
#[async_trait]
trait ApplicationPeerNetworkIdSender<TMessage: Send>: Clone {
    fn send_to(&mut self, recipient: PeerNetworkId, message: TMessage) -> Result<(), NetworkError>;

    fn send_to_many(
        &mut self,
        recipients: impl Iterator<Item = PeerNetworkId>,
        message: TMessage,
    ) -> Result<(), NetworkError>;

    async fn send_rpc(
        &mut self,
        recipient: PeerNetworkId,
        req_msg: TMessage,
        timeout: Duration,
    ) -> Result<TMessage, RpcError>;
}
