// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::DiscoveryError;
use channel::diem_channel::Receiver;
use diem_config::config::{Peer, PeerRole, PeerSet};
use diem_types::account_config::NetworkIdentity;
use futures::Stream;
use std::{
    collections::HashSet,
    pin::Pin,
    task::{Context, Poll},
};

/// Retrieves NetworkIdentity from the OnChain Network Identity
pub struct OnChainStream {
    network_identity_events: Receiver<(), Vec<NetworkIdentity>>,
    peer_set: PeerSet,
}

impl OnChainStream {
    pub(crate) fn new(network_identity_events: Receiver<(), Vec<NetworkIdentity>>) -> Self {
        OnChainStream {
            network_identity_events,
            peer_set: PeerSet::new(),
        }
    }
}

impl Stream for OnChainStream {
    type Item = Result<PeerSet, DiscoveryError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let maybe_update =
            futures::ready!(Pin::new(&mut self.network_identity_events).poll_next(cx));
        let mut temp_peer_set = self.peer_set.clone();

        if let Some(identities) = maybe_update {
            for identity in identities {
                let entry = temp_peer_set
                    .entry(identity.peer_id)
                    .or_insert_with(|| Peer::new(Vec::new(), HashSet::new(), PeerRole::Downstream));
                entry.keys = identity.pubkeys.into_iter().collect();
            }
            self.peer_set = temp_peer_set;
        }
        Poll::Ready(Some(Ok(self.peer_set.clone())))
    }
}
