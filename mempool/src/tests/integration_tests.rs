// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::tests::{
    common::TestTransaction,
    test_framework::{test_transaction, test_transactions, MempoolTestFramework},
};
use diem_config::network_id::NetworkId;
use futures::executor::block_on;
use network::testutils::{
    builder::TestFrameworkBuilder, test_framework::TestFramework, test_node::NodeId,
};

#[test]
fn basic_send_txns_validator_test() {
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
        let v1_node = test_framework.node_mut(v1);
        v1_node.add_txns_via_client(&test_txns).await;
        v1_node.assert_txns_in_mempool(&test_txns);
        // Send first message and ack
        test_framework.propagate_msg_and_ack(v1, v2).await;
        // Ensure messages show up
        let v1_node = test_framework.node(v1);
        let v2_node = test_framework.node(v2);

        v2_node.assert_txns_in_mempool(&test_txns);
        v2_node.commit_txns(&test_txns);

        v1_node.assert_txns_in_mempool(&test_txns);
        v2_node.assert_txns_not_in_mempool(&test_txns);
    };
    block_on(future)
}

#[test]
fn fn_to_val_test() {
    let owner_1 = 0;
    let mut test_framework: MempoolTestFramework = TestFrameworkBuilder::new()
        .add_owners(1)
        .add_validator(owner_1)
        .add_vfn(owner_1)
        .add_pfn(owner_1)
        .build();

    let val = NodeId::validator(owner_1);
    let vfn = NodeId::vfn(owner_1);
    let pfn = NodeId::pfn(owner_1);
    let test_txns = test_transactions(0, 3);
    test_framework.connect(vfn, val);
    test_framework.connect(pfn, vfn);

    let future = async move {
        let pfn_node = test_framework.node_mut(pfn);
        pfn_node.add_txns_via_client(&test_txns).await;
        pfn_node.assert_txns_in_mempool(&test_txns);
        // Send to vfn
        test_framework.propagate_msg_and_ack(pfn, vfn).await;
        test_framework.node(vfn).assert_txns_in_mempool(&test_txns);

        // Send to validator
        test_framework.propagate_msg_and_ack(vfn, val).await;
        let validator = test_framework.node(val);

        // Ensure messages show up
        validator.assert_txns_in_mempool(&test_txns);
        validator.commit_txns(&test_txns);
        validator.assert_txns_not_in_mempool(&test_txns);
    };

    block_on(future)
}

#[test]
fn drop_msg_test() {
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
        // Drop first try
        test_framework.drop_msg(v1, NetworkId::Validator).await;
        test_framework
            .node(v2)
            .assert_txns_not_in_mempool(&test_txns);

        // Send first message
        test_framework.propagate_msg(v1, NetworkId::Validator).await;
        // But, drop the ack
        test_framework.drop_msg(v2, NetworkId::Validator).await;
        test_framework.node(v2).assert_txns_in_mempool(&test_txns);

        // Try again
        test_framework.propagate_msg_and_ack(v1, v2).await;
        // Ensure messages show up
        let v1_node = test_framework.node(v1);
        let v2_node = test_framework.node(v2);
        v1_node.assert_txns_in_mempool(&test_txns);
        v2_node.assert_txns_in_mempool(&test_txns);
        v2_node.commit_txns(&test_txns);

        v1_node.assert_txns_in_mempool(&test_txns);
        v2_node.assert_txns_not_in_mempool(&test_txns);
    };

    block_on(future)
}

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

    let future = async move {
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

// TODO: Mempool is full test
// TODO: Test max broadcast limit
