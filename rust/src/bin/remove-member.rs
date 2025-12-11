//! Remove a member from a Squads v4 Multisig (config authority only)
//!
//! Usage:
//!   cargo run --bin remove_member -- <multisig_address> <member_to_remove> [mainnet]
//!
//! Example:
//!   cargo run --bin remove_member -- BJbRtXM8wecvRrJNbbpNLfuG8FTSoU6zPYW1NFrMH6Q3 53Sb8FiUTRJbqs6SC5KgbMLqfwT98qPPTVroodLJKQ9m mainnet

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    transaction::Transaction,
};
use squads_multisig::anchor_lang::InstructionData;
use squads_multisig::squads_multisig_program;
use std::env;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        println!("Usage: cargo run --bin remove_member -- <multisig_address> <member_to_remove> [mainnet]");
        println!("Example: cargo run --bin remove_member -- BJbRtXM8wecvRrJNbbpNLfuG8FTSoU6zPYW1NFrMH6Q3 53Sb8FiUTRJbqs6SC5KgbMLqfwT98qPPTVroodLJKQ9m mainnet");
        return;
    }

    let multisig_pda: Pubkey = args[1].parse().expect("Invalid multisig address");
    let member_to_remove: Pubkey = args[2].parse().expect("Invalid member address");
    let network = args.get(3).map(|s| s.as_str()).unwrap_or("devnet");

    let rpc_url = match network {
        "mainnet" => MAINNET_RPC,
        _ => DEVNET_RPC,
    };

    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let config_authority = read_keypair_file("../member1.json").expect("Failed to read member1.json");

    println!("=== Remove Member from Multisig ({}) ===\n", network.to_uppercase());
    println!("Multisig: {}", multisig_pda);
    println!("Config Authority: {}", config_authority.pubkey());
    println!("Member to Remove: {}", member_to_remove);

    let instruction_data = squads_multisig_program::instruction::MultisigRemoveMember {
        args: squads_multisig_program::MultisigRemoveMemberArgs {
            old_member: member_to_remove,
            memo: None,
        },
    };

    let accounts = vec![
        AccountMeta::new(multisig_pda, false),
        AccountMeta::new_readonly(config_authority.pubkey(), true),
        AccountMeta::new_readonly(squads_multisig_program::ID, false), // rent_payer (None)
        AccountMeta::new_readonly(squads_multisig_program::ID, false), // system_program (None)
    ];

    let instruction = Instruction {
        program_id: squads_multisig_program::ID,
        accounts,
        data: instruction_data.data(),
    };

    println!("\nRemoving member...");

    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&config_authority.pubkey()),
        &[&config_authority],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&transaction) {
        Ok(sig) => {
            println!("\nMember removed successfully!");
            println!("Transaction: {}", sig);

            let cluster_param = if network == "mainnet" { "" } else { "?cluster=devnet" };
            println!("\nView on Solana Explorer:");
            println!("https://explorer.solana.com/tx/{}{}", sig, cluster_param);
        }
        Err(e) => {
            println!("\nFailed to remove member: {}", e);
        }
    }
}
