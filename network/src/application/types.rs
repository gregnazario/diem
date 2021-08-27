// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{transport::ConnectionMetadata, ProtocolId};
use std::collections::HashMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeerError {
    AlreadyExists,
    NotFound,
    NetworkError(String),
    Parsing(String),
    RpcError(String),
    Timeout(String),
}

/// Descriptor of a Peer and how it should rank
#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub status: PeerState,
    pub scores: HashMap<ProtocolId, u32>,
    pub connection_metadata: ConnectionMetadata,
}

impl PeerInfo {
    pub fn new(connection_metadata: ConnectionMetadata) -> Self {
        PeerInfo {
            status: PeerState::Healthy,
            scores: HashMap::new(),
            connection_metadata,
        }
    }
}

#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub enum PeerState {
    Healthy,
    Unhealthy,
    Disconnecting,
    Disconnected,
}
