//! Add a member to a Squads v4 Multisig (config authority only)
//!
//! Usage:
//!   cargo run --bin add_member -- <multisig_address> <new_member_address> [mainnet]
//!
//! Example:
//!   cargo run --bin add_member -- BJbRtXM8wecvRrJNbbpNLfuG8FTSoU6zPYW1NFrMH6Q3 NewMemberPubkeyHere mainnet

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
use squads_multisig::state::{Member, Permission, Permissions};
use std::env;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        println!("Usage: cargo run --bin add_member -- <multisig_address> <new_member_address> [mainnet]");
        println!("Example: cargo run --bin add_member -- BJbRtXM8wecvRrJNbbpNLfuG8FTSoU6zPYW1NFrMH6Q3 NewMemberPubkeyHere mainnet");
        return;
    }

    let multisig_pda: Pubkey = args[1].parse().expect("Invalid multisig address");
    let new_member_pubkey: Pubkey = args[2].parse().expect("Invalid new member address");
    let network = args.get(3).map(|s| s.as_str()).unwrap_or("devnet");

    let rpc_url = match network {
        "mainnet" => MAINNET_RPC,
        _ => DEVNET_RPC,
    };

    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let config_authority = read_keypair_file("../member1.json").expect("Failed to read member1.json");

    // New member gets full permissions (Initiate, Vote, Execute)
    let all_permissions = Permissions {
        mask: Permission::Initiate as u8 | Permission::Vote as u8 | Permission::Execute as u8,
    };

    let new_member = Member {
        key: new_member_pubkey,
        permissions: all_permissions,
    };

    println!("=== Add Member to Multisig ({}) ===\n", network.to_uppercase());
    println!("Multisig: {}", multisig_pda);
    println!("Config Authority: {}", config_authority.pubkey());
    println!("New Member: {}", new_member_pubkey);
    println!("Permissions: Initiate, Vote, Execute");

    let instruction_data = squads_multisig_program::instruction::MultisigAddMember {
        args: squads_multisig_program::MultisigAddMemberArgs {
            new_member,
            memo: None,
        },
    };

    let accounts = vec![
        AccountMeta::new(multisig_pda, false),
        AccountMeta::new_readonly(config_authority.pubkey(), true),
        AccountMeta::new(config_authority.pubkey(), true), // rent_payer
        AccountMeta::new_readonly(solana_sdk::system_program::ID, false), // system_program
    ];

    let instruction = Instruction {
        program_id: squads_multisig_program::ID,
        accounts,
        data: instruction_data.data(),
    };

    println!("\nAdding member...");

    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&config_authority.pubkey()),
        &[&config_authority],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&transaction) {
        Ok(sig) => {
            println!("\nMember added successfully!");
            println!("Transaction: {}", sig);

            let cluster_param = if network == "mainnet" { "" } else { "?cluster=devnet" };
            println!("\nView on Solana Explorer:");
            println!("https://explorer.solana.com/tx/{}{}", sig, cluster_param);
        }
        Err(e) => {
            println!("\nFailed to add member: {}", e);
        }
    }
}
