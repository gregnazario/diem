// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::testutils::{
    test_framework::TestFramework,
    test_node::{NodeId, NodeType, TestNode},
};
use diem_config::{
    config::NodeConfig,
    network_id::{NetworkId, PeerNetworkId},
};
use rand::{rngs::StdRng, SeedableRng};
use std::{collections::HashMap, marker::PhantomData};

pub struct TestFrameworkBuilder<Node: TestNode + Sync, Framework: TestFramework<Node>> {
    owners: u32,
    nodes: HashMap<NodeId, Node>,
    peer_network_id_to_node: HashMap<PeerNetworkId, NodeId>,
    rng: StdRng,
    _framework_marker: PhantomData<Framework>,
}

impl<Node: TestNode + Sync, Framework: TestFramework<Node>> TestFrameworkBuilder<Node, Framework> {
    pub fn new() -> Self {
        Self {
            owners: 0,
            nodes: HashMap::new(),
            peer_network_id_to_node: HashMap::new(),
            rng: StdRng::from_seed([0u8; 32]),
            _framework_marker: PhantomData::default(),
        }
    }

    pub fn build(self) -> Framework {
        TestFramework::new(self.nodes, self.peer_network_id_to_node)
    }

    /// A safety measure to make sure all owners are on purpose
    pub fn add_owners(mut self, count: u32) -> Self {
        self.owners += count;
        self
    }

    pub fn add_validator(mut self, owner: u32) -> Self {
        let config = NodeConfig::random_with_template(
            owner,
            &NodeConfig::default_for_validator(),
            &mut self.rng,
        );
        let peer_id = config
            .validator_network
            .as_ref()
            .expect("Validator must have a validator network")
            .peer_id();
        self.add_node(
            owner,
            NodeType::Validator,
            config,
            &[
                PeerNetworkId::new(NetworkId::Validator, peer_id),
                PeerNetworkId::new(NetworkId::Vfn, peer_id),
            ],
        )
    }

    pub fn add_vfn(mut self, owner: u32) -> Self {
        let config = NodeConfig::random_with_template(
            owner,
            &NodeConfig::default_for_validator_full_node(),
            &mut self.rng,
        );
        let peer_id = config
            .full_node_networks
            .iter()
            .find(|network| network.network_id == NetworkId::Public)
            .expect("Vfn must have a public network")
            .peer_id();
        self.add_node(
            owner,
            NodeType::ValidatorFullNode,
            config,
            &[
                PeerNetworkId::new(NetworkId::Vfn, peer_id),
                PeerNetworkId::new(NetworkId::Public, peer_id),
            ],
        )
    }

    pub fn add_pfn(mut self, owner: u32) -> Self {
        let config = NodeConfig::random_with_template(
            owner,
            &NodeConfig::default_for_public_full_node(),
            &mut self.rng,
        );
        let peer_id = config
            .full_node_networks
            .iter()
            .find(|network| network.network_id == NetworkId::Public)
            .expect("Pfn must have a public network")
            .peer_id();
        self.add_node(
            owner,
            NodeType::PublicFullNode,
            config,
            &[PeerNetworkId::new(NetworkId::Public, peer_id)],
        )
    }

    /// Add a node to the network, ensuring that it doesn't already exist
    fn add_node(
        mut self,
        owner: u32,
        node_type: NodeType,
        config: NodeConfig,
        peer_network_ids: &[PeerNetworkId],
    ) -> Self {
        assert!(owner < self.owners);

        let node_id = NodeId { owner, node_type };
        assert!(!self.nodes.contains_key(&node_id));

        let node = Framework::build_node(node_id, config, peer_network_ids);
        self.nodes.insert(node_id, node);
        for peer_network_id in peer_network_ids {
            // If we have duplicates, nothing will work
            assert!(!self.peer_network_id_to_node.contains_key(peer_network_id));
            self.peer_network_id_to_node
                .insert(*peer_network_id, node_id);
        }
        self
    }
}
