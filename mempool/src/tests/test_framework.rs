// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use futures::{channel::oneshot, SinkExt};
use tokio::runtime::Runtime;

use diem_config::{
    config::NodeConfig,
    network_id::{NetworkId, PeerNetworkId},
};
use diem_infallible::{Mutex, RwLock};
use diem_types::{
    mempool_status::MempoolStatusCode, on_chain_config::ON_CHAIN_CONFIG_REGISTRY,
    transaction::SignedTransaction,
};
use event_notifications::EventSubscriptionService;
use mempool_notifications::MempoolNotifier;
use network::application::storage::PeerMetadataStorage;
use storage_interface::{mock::MockDbReaderWriter, DbReaderWriter};
use vm_validator::mocks::mock_vm_validator::MockVMValidator;

use crate::{
    core_mempool::CoreMempool,
    network::{MempoolNetworkEvents, MempoolNetworkSender},
    shared_mempool::start_shared_mempool,
    tests::common::TestTransaction,
    ConsensusRequest, MempoolClientRequest, MempoolClientSender,
};
use network::testutils::{
    test_framework::{setup_node_networks, TestFramework},
    test_node::{
        ApplicationNetworkHandle, InboundNetworkHandle, NodeId, NodeType, OutboundMessageReceiver,
        TestNode,
    },
};

pub type MempoolConsensusSender = futures::channel::mpsc::Sender<ConsensusRequest>;

/// An individual node which runs on it's own runtime
/// TODO: Add ability to mock StateSync updates to remove transactions
/// TODO: Add ability to reject transactions via Consensus
pub struct MempoolNode {
    pub node_id: NodeId,
    pub network_ids: Vec<NetworkId>,
    pub peer_network_ids: HashMap<NetworkId, PeerNetworkId>,
    pub mempool: Arc<Mutex<CoreMempool>>,
    pub runtime: Arc<Runtime>,
    pub config: NodeConfig,

    pub mempool_client_sender: MempoolClientSender,
    pub mempool_consensus_sender: MempoolConsensusSender,
    pub mempool_notifications: MempoolNotifier,

    pub inbound_handles: HashMap<NetworkId, InboundNetworkHandle>,
    pub outbound_handles: HashMap<NetworkId, OutboundMessageReceiver>,
    pub other_inbound_handles: HashMap<PeerNetworkId, InboundNetworkHandle>,
    pub peer_metadata_storage: Arc<PeerMetadataStorage>,
}

impl std::fmt::Display for MempoolNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.node_id)
    }
}

impl TestNode for MempoolNode {
    fn node_id(&self) -> NodeId {
        self.node_id
    }

    fn node_type(&self) -> NodeType {
        self.node_id.node_type
    }

    fn node_inbound_handle(&self, network_id: NetworkId) -> InboundNetworkHandle {
        self.inbound_handles
            .get(&network_id)
            .expect("Must have inbound handle for network")
            .clone()
    }

    fn add_other_inbound_handle(
        &mut self,
        peer_network_id: PeerNetworkId,
        handle: InboundNetworkHandle,
    ) {
        if self
            .other_inbound_handles
            .insert(peer_network_id, handle)
            .is_some()
        {
            panic!(
                "Double added handle for {} on {}",
                peer_network_id, self.node_id
            )
        }
    }

    fn other_inbound_handle(&self, peer_network_id: PeerNetworkId) -> InboundNetworkHandle {
        self.other_inbound_handles
            .get(&peer_network_id)
            .expect("Must have inbound handle for other peer")
            .clone()
    }

    fn outbound_handle(&mut self, network_id: NetworkId) -> &mut OutboundMessageReceiver {
        self.outbound_handles.get_mut(&network_id).unwrap()
    }

    fn peer_metadata_storage(&self) -> &PeerMetadataStorage {
        &self.peer_metadata_storage
    }

    fn peer_network_ids(&self) -> &HashMap<NetworkId, PeerNetworkId> {
        &self.peer_network_ids
    }
}

impl MempoolNode {
    /// Queues transactions for sending on a node, uses client
    pub async fn add_txns_via_client(&mut self, txns: &[TestTransaction]) {
        for txn in txns.iter() {
            let transaction = txn.make_signed_transaction_with_max_gas_amount(5);
            let (sender, receiver) = oneshot::channel();

            self.mempool_client_sender
                .send(MempoolClientRequest::SubmitTransaction(transaction, sender))
                .await
                .unwrap();
            let status = receiver.await.unwrap().unwrap();
            assert_eq!(status.0.code, MempoolStatusCode::Accepted)
        }
    }

    /// Commits transactions and removes them from the local mempool, stops them from being broadcasted later
    pub fn commit_txns(&self, txns: &[TestTransaction]) {
        if NodeType::Validator == self.node_id.node_type {
            let mut mempool = self.mempool.lock();
            for txn in txns.iter() {
                mempool.remove_transaction(
                    &TestTransaction::get_address(txn.address),
                    txn.sequence_number,
                    false,
                );
            }
        } else {
            panic!("Can't commit transactions on anything but a validator");
        }
    }

    pub fn assert_txns_in_mempool(&self, txns: &[TestTransaction]) {
        let block = self.mempool.lock().get_block(100, HashSet::new());
        if !txns
            .iter()
            .all(|txn| block_contains_test_transaction(&block, txn))
        {
            let txns: Vec<_> = txns
                .iter()
                .map(|txn| (txn.address, txn.sequence_number))
                .collect();
            panic!("Expected to contain the test transactions {:?}", txns);
        }
        assert_eq!(block.len(), txns.len());
    }

    pub fn assert_txns_not_in_mempool(&self, txns: &[TestTransaction]) {
        let block = self.mempool.lock().get_block(100, HashSet::new());
        if txns
            .iter()
            .any(|txn| block_contains_test_transaction(&block, txn))
        {
            let txns: Vec<_> = txns
                .iter()
                .map(|txn| (txn.address, txn.sequence_number))
                .collect();
            panic!("Expected to not have the test transactions {:?}", txns);
        }
    }
}

pub struct MempoolTestFramework {
    pub nodes: HashMap<NodeId, MempoolNode>,
}

impl TestFramework<MempoolNode> for MempoolTestFramework {
    fn new(nodes: HashMap<NodeId, MempoolNode>) -> Self {
        Self { nodes }
    }

    fn nodes_mut(&mut self) -> &mut HashMap<NodeId, MempoolNode> {
        &mut self.nodes
    }

    fn build_node(
        node_id: NodeId,
        config: NodeConfig,
        peer_network_ids: &[PeerNetworkId],
    ) -> MempoolNode {
        // Collect mappings of network_id to peer_network_id
        let mut network_ids = Vec::new();
        let mut network_id_mapping = HashMap::new();
        for peer_network_id in peer_network_ids {
            let network_id = peer_network_id.network_id();
            assert!(
                !network_id_mapping.contains_key(&network_id),
                "Duplicate network id for node"
            );
            network_ids.push(network_id);
            network_id_mapping.insert(network_id, *peer_network_id);
        }
        let runtime = node_runtime(node_id);

        let (application_handles, inbound_handles, outbound_handles, peer_metadata_storage) =
            setup_node_networks(&network_ids);
        let (mempool_client_sender, mempool_consensus_sender, mempool_notifications, mempool) =
            setup_mempool(
                config.clone(),
                application_handles,
                peer_metadata_storage.clone(),
                &runtime,
            );

        MempoolNode {
            node_id,
            config,
            network_ids,
            peer_network_ids: network_id_mapping,
            mempool,
            runtime,
            mempool_client_sender,
            mempool_consensus_sender,
            mempool_notifications,
            inbound_handles,
            outbound_handles,
            other_inbound_handles: HashMap::new(),
            peer_metadata_storage,
        }
    }
}

fn setup_mempool(
    config: NodeConfig,
    network_handles: Vec<ApplicationNetworkHandle<MempoolNetworkSender, MempoolNetworkEvents>>,
    peer_metadata_storage: Arc<PeerMetadataStorage>,
    runtime: &Runtime,
) -> (
    MempoolClientSender,
    MempoolConsensusSender,
    MempoolNotifier,
    Arc<Mutex<CoreMempool>>,
) {
    let (sender, _subscriber) = futures::channel::mpsc::unbounded();
    let (ac_endpoint_sender, ac_endpoint_receiver) = mpsc_channel();
    let (consensus_sender, consensus_events) = mpsc_channel();
    let (mempool_notifier, mempool_listener) =
        mempool_notifications::new_mempool_notifier_listener_pair();

    let mempool = Arc::new(Mutex::new(CoreMempool::new(&config)));
    let vm_validator = Arc::new(RwLock::new(MockVMValidator));
    let db_rw = Arc::new(RwLock::new(DbReaderWriter::new(MockDbReaderWriter)));
    let db_ro = Arc::new(MockDbReaderWriter);

    let mut event_subscriber = EventSubscriptionService::new(ON_CHAIN_CONFIG_REGISTRY, db_rw);
    let reconfig_event_subscriber = event_subscriber.subscribe_to_reconfigurations().unwrap();

    start_shared_mempool(
        runtime.handle(),
        &config,
        mempool.clone(),
        network_handles,
        ac_endpoint_receiver,
        consensus_events,
        mempool_listener,
        reconfig_event_subscriber,
        db_ro,
        vm_validator,
        vec![sender],
        peer_metadata_storage,
    );

    (
        ac_endpoint_sender,
        consensus_sender,
        mempool_notifier,
        mempool,
    )
}

fn node_runtime(node_id: NodeId) -> Arc<Runtime> {
    Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .thread_name(format!("node-{:?}", node_id))
            .enable_all()
            .build()
            .expect("[test-framework] failed to create runtime"),
    )
}

fn mpsc_channel<T>() -> (
    futures::channel::mpsc::Sender<T>,
    futures::channel::mpsc::Receiver<T>,
) {
    futures::channel::mpsc::channel(1_024)
}

pub fn test_transaction(seq_num: u64) -> TestTransaction {
    TestTransaction::new(1, seq_num, 1)
}

pub fn test_transactions(start: u64, num: u64) -> Vec<TestTransaction> {
    let mut txns = vec![];
    for seq_num in start..start.checked_add(num).unwrap() {
        txns.push(test_transaction(seq_num))
    }
    txns
}

fn block_contains_test_transaction(block: &[SignedTransaction], txn: &TestTransaction) -> bool {
    block.iter().any(|signed_txn| {
        signed_txn.sequence_number() == txn.sequence_number
            && signed_txn.sender() == TestTransaction::get_address(txn.address)
    })
}
