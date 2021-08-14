// Copyright (c) The Diem Core Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    test_utils::{compare_balances, setup_swarm_and_client_proxy},
    workspace_builder,
};
use cli::client_proxy::ClientProxy;
use diem_temppath::TempPath;
use diem_types::account_address::AccountAddress;
use move_move_command_line_common::files::MOVE_EXTENSION;
use std::{
    fs, io,
    io::Write,
    path::{Path, PathBuf},
};

static XUS: &str = "XUS";

#[test]
fn test_malformed_script() {
    let (_env, mut client) = setup_swarm_and_client_proxy(1, 0);
    let client = &mut client;
    enable_custom_scripts(client);
    client.create_next_account(false).unwrap();
    mint_coins(client, 0, 100);
    assert_balance(client, 0, 100.0);

    let (script_compiled_path, _) = compile_program(
        client,
        "testsuite/smoke-test/src/dev_modules/test_script.move",
        None,
        None,
    );

    // the script expects two arguments. Passing only one in the test, which will cause a failure.
    execute_script(client, 0, script_compiled_path, &["10"])
        .expect_err("malformed script did not fail!");

    // Previous transaction should not choke the system.
    mint_coins(client, 0, 10);
}

#[test]
fn test_execute_custom_module_and_script() {
    let (_env, mut client) = setup_swarm_and_client_proxy(1, 0);
    let client = &mut client;
    enable_custom_scripts(client);
    client.create_next_account(false).unwrap();
    mint_coins(client, 0, 50);
    assert_balance(client, 0, 50.0);

    let recipient_address = client.create_next_account(false).unwrap().address;
    mint_coins(client, 1, 1);
    assert_balance(client, 1, 1.0);

    let (sender_account, _) = client.get_account_address_from_parameter("0").unwrap();

    // Compile and publish that module.
    let (module_compiled_path, module_source_path) = compile_program(
        client,
        "testsuite/smoke-test/src/dev_modules/module.move",
        Some(sender_account),
        None,
    );
    client
        .publish_module(&["publish", "0", &module_compiled_path[..]])
        .unwrap();

    // Make a copy of script.move with "{{sender}}" substituted.
    let (script_compiled_path, _) = compile_program(
        client,
        "testsuite/smoke-test/src/dev_modules/script.move",
        Some(sender_account),
        Some(module_source_path.as_str()),
    );
    let formatted_recipient_address = format!("0x{}", recipient_address);
    execute_script(
        client,
        0,
        script_compiled_path,
        &[formatted_recipient_address.as_str(), "10"],
    )
    .unwrap();

    assert_balance(client, 0, 49.999_990);
    assert_balance(client, 1, 1.000_010);
}

fn enable_custom_scripts(client: &mut ClientProxy) {
    client
        .enable_custom_script(&["enable_custom_script"], true, true)
        .unwrap();
}

fn execute_script(
    client: &mut ClientProxy,
    sending_account: u64,
    script_compiled_path: String,
    arguments: &[&str],
) -> anyhow::Result<()> {
    let sending_account = sending_account.to_string();
    let mut args = vec![
        "execute",
        sending_account.as_str(),
        script_compiled_path.as_str(),
    ];
    args.extend_from_slice(arguments);
    client.execute_script(&args)
}

fn mint_coins(client: &mut ClientProxy, account: u64, coins: u64) {
    client
        .mint_coins(
            &[
                "mintb",
                account.to_string().as_str(),
                coins.to_string().as_str(),
                XUS,
            ],
            true,
        )
        .unwrap();
}

fn assert_balance(client: &mut ClientProxy, account: u64, amount: f64) {
    assert!(compare_balances(
        vec![(amount, XUS.to_string())],
        client
            .get_balances(&["b", account.to_string().as_str()])
            .unwrap(),
    ));
}

fn compile_program(
    client: &mut ClientProxy,
    script_template_path: &'static str,
    maybe_script_account: Option<AccountAddress>,
    maybe_module_source_path: Option<&str>,
) -> (String, String) {
    // Make a copy of script.move with "{{sender}}" substituted.
    let script_source_path = workspace_builder::workspace_root().join(script_template_path);
    let script_source_path = if let Some(script_account) = maybe_script_account {
        copy_file_with_sender_address(&script_source_path, script_account).unwrap()
    } else {
        script_source_path
    };
    let script_source_path = script_source_path.to_str().unwrap();

    // Get the path to the Move stdlib sources
    let move_stdlib_dir = move_stdlib::move_stdlib_modules_full_path();
    let diem_framework_dir = diem_framework::diem_stdlib_modules_full_path();

    // Compile and execute the script.
    let script_params = if let Some(module_source_path) = maybe_module_source_path {
        vec![
            "compile",
            script_source_path,
            module_source_path,
            move_stdlib_dir.as_str(),
            diem_framework_dir.as_str(),
        ]
    } else {
        vec![
            "compile",
            script_source_path,
            move_stdlib_dir.as_str(),
            diem_framework_dir.as_str(),
        ]
    };

    let mut script_compiled_paths = client.compile_program(&script_params).unwrap();
    let script_compiled_path = if script_compiled_paths.len() != 1 {
        panic!("compiler output has more than one file")
    } else {
        script_compiled_paths.pop().unwrap()
    };
    (script_compiled_path, script_source_path.to_string())
}

fn copy_file_with_sender_address(file_path: &Path, sender: AccountAddress) -> io::Result<PathBuf> {
    let tmp_source_path = TempPath::new().as_ref().with_extension(MOVE_EXTENSION);
    let mut tmp_source_file = std::fs::File::create(tmp_source_path.clone())?;
    let mut code = fs::read_to_string(file_path)?;
    code = code.replace("{{sender}}", &format!("0x{}", sender));
    writeln!(tmp_source_file, "{}", code)?;
    Ok(tmp_source_path)
}
