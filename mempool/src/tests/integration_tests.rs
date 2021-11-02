// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use futures::executor::block_on;

use diem_config::network_id::NetworkId;
use network::testutils::builder::TestFrameworkBuilder;

use crate::tests::test_framework::{test_transactions, MempoolTestFramework};
use network::testutils::{test_framework::TestFramework, test_node::NodeId};

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
        // Send first message
        test_framework.propagate_msg(v1, NetworkId::Validator).await;
        // Respond with ack
        test_framework.propagate_msg(v2, NetworkId::Validator).await;
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
fn basic_send_txns_fn_test() {
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
        test_framework.propagate_msg(pfn, NetworkId::Public).await;
        // Respond with ack
        test_framework.propagate_msg(vfn, NetworkId::Public).await;
        test_framework.node(vfn).assert_txns_in_mempool(&test_txns);

        // Send to validator
        test_framework.propagate_msg(vfn, NetworkId::Vfn).await;
        test_framework.propagate_msg(val, NetworkId::Vfn).await;
        let validator = test_framework.node(val);

        // Ensure messages show up
        validator.assert_txns_in_mempool(&test_txns);
        validator.commit_txns(&test_txns);
        validator.assert_txns_not_in_mempool(&test_txns);
    };

    block_on(future)
}

#[test]
fn basic_drop_test() {
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
        test_framework.propagate_msg(v1, NetworkId::Validator).await;
        // Send ack
        test_framework.propagate_msg(v2, NetworkId::Validator).await;
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
