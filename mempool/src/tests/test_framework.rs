// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use futures::{channel::oneshot, SinkExt, StreamExt};
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
use network::{
    application::storage::PeerMetadataStorage,
    peer_manager::{PeerManagerNotification, PeerManagerRequest},
    protocols::{direct_send::Message, rpc::InboundRpcRequest},
};
use storage_interface::{mock::MockDbReaderWriter, DbReaderWriter};
use vm_validator::mocks::mock_vm_validator::MockVMValidator;

use crate::{
    core_mempool::CoreMempool,
    network::{MempoolNetworkEvents, MempoolNetworkSender, MempoolSyncMsg},
    shared_mempool::start_shared_mempool,
    tests::common::TestTransaction,
    ConsensusRequest, MempoolClientRequest, MempoolClientSender,
};
use network::testutils::{
    test_framework::{setup_node_networks, TestFramework},
    test_node::{ApplicationNetworkHandle, NodeId, NodeNetworkHandle, NodeType, TestNode},
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

    pub node_network_handles: HashMap<NetworkId, NodeNetworkHandle>,
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

    fn node_network_handles(&self) -> &HashMap<NetworkId, NodeNetworkHandle> {
        &self.node_network_handles
    }

    fn node_network_handles_mut(&mut self) -> &mut HashMap<NetworkId, NodeNetworkHandle> {
        &mut self.node_network_handles
    }

    fn node_type(&self) -> NodeType {
        self.node_id.node_type
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
    pub peer_network_id_to_node: HashMap<PeerNetworkId, NodeId>,
}

impl TestFramework<MempoolNode> for MempoolTestFramework {
    fn new(
        nodes: HashMap<NodeId, MempoolNode>,
        peer_network_id_to_node: HashMap<PeerNetworkId, NodeId>,
    ) -> Self {
        Self {
            nodes,
            peer_network_id_to_node,
        }
    }

    fn nodes(&self) -> &HashMap<NodeId, MempoolNode> {
        &self.nodes
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

        let (reqs_handles, node_network_handles, peer_metadata) = setup_node_networks(&network_ids);
        let (mempool_client_sender, mempool_consensus_sender, mempool_notifications, mempool) =
            setup_mempool(
                config.clone(),
                reqs_handles,
                peer_metadata.clone(),
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
            node_network_handles,
            peer_metadata_storage: peer_metadata,
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

impl MempoolTestFramework {
    pub async fn drop_msg(&mut self, sender_id: NodeId, network_id: NetworkId) {
        let sender = self.node_mut(sender_id);
        let network_handle = sender.node_network_handles.get_mut(&network_id).unwrap();
        let _ = network_handle.outgoing_messages.next().await;
    }

    pub async fn propagate_msg_and_ack(&mut self, sender_id: NodeId, receiver_id: NodeId) {
        let network_id = self.find_common_network(sender_id, receiver_id);
        self.propagate_msg(sender_id, network_id).await;
        self.propagate_msg(receiver_id, network_id).await;
    }

    pub async fn propagate_msg(&mut self, sender_id: NodeId, network_id: NetworkId) {
        let sender = self.node_mut(sender_id);

        let network_handle = sender.node_network_handles.get_mut(&network_id).unwrap();
        let maybe_request = network_handle.outgoing_messages.next().await;
        let (peer_id, protocol_id, data, maybe_rpc) = if let Some(request) = maybe_request {
            match request {
                PeerManagerRequest::SendRpc(peer_id, msg) => (
                    peer_id,
                    msg.protocol_id,
                    msg.data,
                    Some((msg.timeout, msg.res_tx)),
                ),
                PeerManagerRequest::SendDirectSend(peer_id, msg) => {
                    (peer_id, msg.protocol_id, msg.mdata, None)
                }
            }
        } else {
            panic!("Expected a message to propagate")
        };

        let msg: MempoolSyncMsg = protocol_id.from_bytes(&data).unwrap();
        let sender_peer_network_id = self.node(sender_id).peer_network_id(network_id);
        let receiver_peer_network_id = PeerNetworkId::new(network_id, peer_id);
        let receiver_id = *self
            .peer_network_id_to_node
            .get(&receiver_peer_network_id)
            .unwrap();
        let receiver = self.node_mut(receiver_id);
        let incoming_message_sender = &receiver
            .node_network_handles
            .get(&network_id)
            .unwrap()
            .incoming_message_sender;

        //if maybe_rpc.is_some()
        //    && matches!(msg, MempoolSyncMsg::BroadcastTransactionsResponse { .. })
        //{
        //    panic!("Can't receive response via RPC")
        //}

        let sender_peer_id = sender_peer_network_id.peer_id();
        if let Some((_timeout, res_tx)) = maybe_rpc {
            incoming_message_sender
                .push(
                    (sender_peer_id, protocol_id),
                    PeerManagerNotification::RecvRpc(
                        sender_peer_id,
                        InboundRpcRequest {
                            protocol_id,
                            data,
                            res_tx,
                        },
                    ),
                )
                .unwrap()
        } else {
            incoming_message_sender
                .push(
                    (sender_peer_id, protocol_id),
                    PeerManagerNotification::RecvMessage(
                        sender_peer_id,
                        Message {
                            protocol_id,
                            mdata: data,
                        },
                    ),
                )
                .unwrap()
        }
    }
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
