// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::tests::{
    common::TestTransaction,
    test_framework::{test_transaction, test_transactions, MempoolTestFramework},
};
use futures::executor::block_on;
use netcore::transport::ConnectionOrigin;
use network::testutils::test_node::drop_next_network_msg;
use network::testutils::{
    builder::TestFrameworkBuilder,
    test_framework::TestFramework,
    test_node::{send_next_network_msg, NodeId, TestNode},
};
use std::sync::Arc;
use tokio::sync::Semaphore;

#[test]
fn basic_send_txns_validator_test() {
    let mut test_framework: MempoolTestFramework = TestFrameworkBuilder::new()
        .add_owners(2)
        .add_validator(0)
        .add_validator(1)
        .build();

    let mut sender = test_framework.take_node(NodeId::validator(0));
    let mut receiver = test_framework.take_node(NodeId::validator(1));
    let network_id = sender.find_common_network(&receiver).unwrap();
    let receiver_metadata = receiver.conn_metadata(network_id, ConnectionOrigin::Outbound, None);

    let sender_test_txns = test_transactions(0, 3);
    let receiver_test_txns = sender_test_txns.clone();

    // Mempools should be clean at start
    sender.assert_txns_not_in_mempool(&sender_test_txns);
    receiver.assert_txns_not_in_mempool(&sender_test_txns);

    let sender_future = async move {
        sender.connect(network_id, receiver_metadata);
        sender.add_txns_via_client(&sender_test_txns).await;
        sender.assert_txns_in_mempool(&sender_test_txns);

        // Send the first message, (if RPC wait for ack)
        send_next_network_msg(&mut sender, network_id).await;

        // Committed transactions should disappear locally
        sender.commit_txns(&sender_test_txns);
        sender.assert_txns_not_in_mempool(&sender_test_txns);
        sender
    };

    let receiver_future = async move {
        send_next_network_msg(&mut receiver, network_id).await;
        receiver.assert_txns_in_mempool(&receiver_test_txns);
        receiver
    };

    let _ = block_on(futures::future::join(sender_future, receiver_future));
}

#[test]
fn fn_to_val_test() {
    let mut test_framework: MempoolTestFramework = TestFrameworkBuilder::new()
        .add_owners(1)
        .add_validator(0)
        .add_vfn(0)
        .add_pfn(0)
        .build();

    let mut val = test_framework.take_node(NodeId::validator(0));
    let mut vfn = test_framework.take_node(NodeId::vfn(0));
    let mut pfn = test_framework.take_node(NodeId::pfn(0));
    let pfn_txns = test_transactions(0, 3);
    let vfn_txns = pfn_txns.clone();
    let val_txns = pfn_txns.clone();

    let pfn_vfn_network = pfn.find_common_network(&vfn).unwrap();
    let vfn_metadata = vfn.conn_metadata(pfn_vfn_network, ConnectionOrigin::Outbound, None);
    let vfn_val_network = vfn.find_common_network(&val).unwrap();
    let val_metadata = val.conn_metadata(vfn_val_network, ConnectionOrigin::Outbound, None);

    // NOTE: Always return node at end, or it will be dropped and channels closed
    let pfn_future = async move {
        pfn.connect(pfn_vfn_network, vfn_metadata);
        pfn.add_txns_via_client(&pfn_txns).await;
        pfn.assert_txns_in_mempool(&pfn_txns);
        // Forward to VFN
        send_next_network_msg(&mut pfn, pfn_vfn_network).await;
        pfn
    };

    let vfn_future = async move {
        vfn.connect(vfn_val_network, val_metadata);

        // Respond to PFN
        send_next_network_msg(&mut vfn, pfn_vfn_network).await;
        vfn.assert_txns_in_mempool(&vfn_txns);

        // Forward to VAL
        send_next_network_msg(&mut vfn, vfn_val_network).await;
        vfn
    };

    let val_future = async move {
        // Respond to VFN
        send_next_network_msg(&mut val, vfn_val_network).await;
        val.assert_txns_in_mempool(&val_txns);
        val
    };

    let _ = block_on(futures::future::join3(pfn_future, vfn_future, val_future));
}

#[test]
fn drop_msg_test() {
    let mut test_framework: MempoolTestFramework = TestFrameworkBuilder::new()
        .add_owners(2)
        .add_validator(0)
        .add_validator(1)
        .build();
    let mut sender = test_framework.take_node(NodeId::validator(0));
    let mut receiver = test_framework.take_node(NodeId::validator(1));
    let network_id = sender.find_common_network(&receiver).unwrap();
    let receiver_metadata = receiver.conn_metadata(network_id, ConnectionOrigin::Outbound, None);

    let sender_test_txns = test_transactions(0, 3);
    let receiver_test_txns = sender_test_txns.clone();

    // Mempools should be clean at start
    sender.assert_txns_not_in_mempool(&sender_test_txns);
    receiver.assert_txns_not_in_mempool(&sender_test_txns);

    let sender_future = async move {
        sender.connect(network_id, receiver_metadata);
        sender.add_txns_via_client(&sender_test_txns).await;
        sender.assert_txns_in_mempool(&sender_test_txns);

        // Send the first message
        send_next_network_msg(&mut sender, network_id).await;

        // Send it again, first ack was dropped
        send_next_network_msg(&mut sender, network_id).await;
        sender
    };

    let receiver_future = async move {
        // Drop first ack (but txns were accepted)
        drop_next_network_msg(&mut receiver, network_id).await;
        receiver.assert_txns_in_mempool(&receiver_test_txns);
        // Send next ack
        send_next_network_msg(&mut receiver, network_id).await;
        receiver
    };

    let _ = block_on(futures::future::join(sender_future, receiver_future));
}
/*
#[test]
fn drop_connection_test() {
    let owner_1 = 0;
    let owner_2 = 1;
    let mut test_framework: MempoolTestFramework = TestFrameworkBuilder::new()
        .add_owners(2)
        .add_validator(owner_1)
        .add_validator(owner_2)
        .build();

    let v1 = NodeId::validator(owner_1);
    let v2 = NodeId::validator(owner_2);
    let test_txns = test_transactions(0, 3);
    test_framework.connect(v1, v2);
    let future = async move {
        test_framework
            .node_mut(v1)
            .add_txns_via_client(&test_txns)
            .await;
        // Send first message
        test_framework.propagate_msg(v1, NetworkId::Validator).await;

        // But, drop the ack and disconnect
        test_framework.drop_msg(v2, NetworkId::Validator).await;
        test_framework.disconnect(v1, v2);

        // When the node reconnects, everything should be good with deduplication
        test_framework.connect(v1, v2);
        test_framework.propagate_msg_and_ack(v1, v2).await;
        test_framework.node(v2).assert_txns_in_mempool(&test_txns);

        // Now disconnect, to simulate a reboot of the node, replace node
        test_framework.disconnect(v1, v2);

        let v2_node = test_framework.node_mut(v2);
        let config = v2_node.config.clone();
        let peer_network_ids: Vec<_> = v2_node
            .peer_network_ids
            .iter()
            .map(|(_, peer)| *peer)
            .collect();
        *v2_node = MempoolTestFramework::build_node(v2, config, &peer_network_ids);
        v2_node.assert_txns_not_in_mempool(&test_txns);

        // Now reconnect, and we should get all the transactions again
        test_framework.connect(v1, v2);
        test_framework.propagate_msg_and_ack(v1, v2).await;
        test_framework.node(v2).assert_txns_in_mempool(&test_txns);
    };

    block_on(future)
}

#[test]
fn test_waiting_on_txns() {
    let owner_1 = 0;
    let owner_2 = 1;
    let mut test_framework: MempoolTestFramework = TestFrameworkBuilder::new()
        .add_owners(2)
        .add_validator(owner_1)
        .add_validator(owner_2)
        .build();

    let v1 = NodeId::validator(owner_1);
    let v2 = NodeId::validator(owner_2);
    let txn_1 = test_transaction(0);
    let test_txns = vec![txn_1.clone(), test_transaction(2)];
    test_framework.connect(v1, v2);

    let validator_1 = async move {
        test_framework
            .node_mut(v1)
            .add_txns_via_client(&test_txns)
            .await;
        // Send first message
        test_framework.propagate_msg(v1, NetworkId::Validator).await;

        // Only the first txn should make it
        test_framework
            .node(v1)
            .assert_txns_in_mempool(&[txn_1.clone()]);
        test_framework.node(v2).assert_txns_in_mempool(&[txn_1]);

        // Ensure now the 2nd and third get through
        let all_txns = test_transactions(0, 3);
        test_framework
            .node_mut(v1)
            .add_txns_via_client(&[test_transaction(1)])
            .await;
        test_framework.node(v1).assert_txns_in_mempool(&all_txns);
        test_framework.propagate_msg(v1, NetworkId::Validator).await;
        test_framework.node(v2).assert_txns_in_mempool(&all_txns);
    };

    let validator_2 = async move {};
    block_on(future)
}

#[test]
fn transactions_merge_different_sets_test() {
    let owner_1 = 0;
    let owner_2 = 1;
    let mut test_framework: MempoolTestFramework = TestFrameworkBuilder::new()
        .add_owners(2)
        .add_validator(owner_1)
        .add_validator(owner_2)
        .build();

    let v1 = NodeId::validator(owner_1);
    let v2 = NodeId::validator(owner_2);
    let txn_1 = test_transaction(0);
    let txn_2 = TestTransaction::new(2, 0, 1);
    let all_txns = &[txn_1.clone(), txn_2.clone()];
    test_framework.connect(v1, v2);

    let future = async move {
        test_framework
            .node_mut(v1)
            .add_txns_via_client(&[txn_1.clone()])
            .await;
        test_framework.node(v1).assert_txns_in_mempool(&[txn_1]);
        test_framework
            .node_mut(v2)
            .add_txns_via_client(&[txn_2.clone()])
            .await;
        test_framework.node(v2).assert_txns_in_mempool(&[txn_2]);
        // Communicate with acks
        test_framework.propagate_msg_and_ack(v1, v2).await;
        test_framework.propagate_msg_and_ack(v2, v1).await;

        // Both should now have a combined amount
        test_framework.node(v1).assert_txns_in_mempool(all_txns);
        test_framework.node(v2).assert_txns_in_mempool(all_txns);
    };
    block_on(future)
}

#[test]
fn update_gas_price_test() {
    let owner_1 = 0;
    let owner_2 = 1;
    let mut test_framework: MempoolTestFramework = TestFrameworkBuilder::new()
        .add_owners(2)
        .add_validator(owner_1)
        .add_validator(owner_2)
        .build();

    let v1 = NodeId::validator(owner_1);
    let v2 = NodeId::validator(owner_2);
    let first_txn = &[TestTransaction::new(1, 0, 1)];
    let updated_txn = &[TestTransaction::new(1, 0, 5)];
    test_framework.connect(v1, v2);
    let future = async move {
        let v1_node = test_framework.node_mut(v1);
        v1_node.add_txns_via_client(first_txn).await;

        test_framework.propagate_msg_and_ack(v1, v2).await;
        let v2_node = test_framework.node(v2);
        v2_node.assert_txns_in_mempool(first_txn);

        // Update and broadcast updated
        let v1_node = test_framework.node_mut(v1);
        v1_node.add_txns_via_client(updated_txn).await;
        v1_node.assert_txns_in_mempool(updated_txn);

        test_framework.propagate_msg_and_ack(v1, v2).await;
        let v2_node = test_framework.node(v2);
        v2_node.assert_txns_in_mempool(updated_txn);
    };
    block_on(future)
}
*/
// TODO: Mempool is full test
// TODO: Test max broadcast limit
