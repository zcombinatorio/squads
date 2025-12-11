//! Remove a spending limit from a Squads v4 Multisig (config authority only)
//!
//! Usage:
//!   cargo run --bin remove-spending-limit -- <multisig_address> <spending_limit_address> [mainnet]
//!
//! Arguments:
//!   multisig_address        - The multisig PDA address
//!   spending_limit_address  - The spending limit PDA to remove
//!
//! Example:
//!   cargo run --bin remove-spending-limit -- BJbRt... SpendingLimitPDA... mainnet

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    transaction::Transaction,
};
use squads_multisig::anchor_lang::{AccountDeserialize, InstructionData};
use squads_multisig::squads_multisig_program;
use squads_multisig::state::SpendingLimit;
use std::env;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        println!("Usage: cargo run --bin remove-spending-limit -- <multisig_address> <spending_limit_address> [mainnet]");
        println!();
        println!("Arguments:");
        println!("  multisig_address        - The multisig PDA address");
        println!("  spending_limit_address  - The spending limit PDA to remove");
        println!();
        println!("Example:");
        println!("  cargo run --bin remove-spending-limit -- BJbRt... SpendingLimitPDA... mainnet");
        return;
    }

    let multisig_pda: Pubkey = args[1].parse().expect("Invalid multisig address");
    let spending_limit_pda: Pubkey = args[2].parse().expect("Invalid spending limit address");
    let network = args.get(3).map(|s| s.as_str()).unwrap_or("devnet");

    let rpc_url = match network {
        "mainnet" => MAINNET_RPC,
        _ => DEVNET_RPC,
    };

    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let config_authority = read_keypair_file("../member1.json").expect("Failed to read member1.json");

    // Fetch and display spending limit info before removal
    match client.get_account(&spending_limit_pda) {
        Ok(account) => {
            if let Ok(spending_limit) = SpendingLimit::try_deserialize(&mut account.data.as_slice()) {
                println!("=== Remove Spending Limit ({}) ===\n", network.to_uppercase());
                println!("Multisig: {}", multisig_pda);
                println!("Config Authority: {}", config_authority.pubkey());
                println!("Spending Limit: {}", spending_limit_pda);
                println!();
                println!("Spending Limit Details:");
                println!("  Amount: {}", spending_limit.amount);
                println!("  Remaining: {}", spending_limit.remaining_amount);
                println!("  Period: {:?}", spending_limit.period);
                println!("  Mint: {} {}", spending_limit.mint, if spending_limit.mint == Pubkey::default() { "(SOL)" } else { "" });
                println!("  Vault Index: {}", spending_limit.vault_index);
                println!("  Members: {:?}", spending_limit.members);

                // Verify the spending limit belongs to this multisig
                if spending_limit.multisig != multisig_pda {
                    println!("\nError: Spending limit does not belong to this multisig!");
                    println!("  Spending limit's multisig: {}", spending_limit.multisig);
                    println!("  Provided multisig: {}", multisig_pda);
                    return;
                }
            }
        }
        Err(e) => {
            println!("Warning: Could not fetch spending limit account: {}", e);
            println!("Proceeding with removal attempt...\n");
        }
    }

    let instruction_data = squads_multisig_program::instruction::MultisigRemoveSpendingLimit {
        args: squads_multisig_program::MultisigRemoveSpendingLimitArgs {
            memo: None,
        },
    };

    // Account order from MultisigRemoveSpendingLimit struct:
    // 1. multisig (seeds verified)
    // 2. config_authority (signer)
    // 3. spending_limit (mut, close)
    // 4. rent_collector (mut) - receives the rent
    let accounts = vec![
        AccountMeta::new_readonly(multisig_pda, false),
        AccountMeta::new_readonly(config_authority.pubkey(), true),
        AccountMeta::new(spending_limit_pda, false),
        AccountMeta::new(config_authority.pubkey(), false), // rent goes back to config authority
    ];

    let instruction = Instruction {
        program_id: squads_multisig_program::ID,
        accounts,
        data: instruction_data.data(),
    };

    println!("\nRemoving spending limit...");

    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&config_authority.pubkey()),
        &[&config_authority],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&transaction) {
        Ok(sig) => {
            println!("\nSpending limit removed successfully!");
            println!("Transaction: {}", sig);
            println!("Rent has been returned to: {}", config_authority.pubkey());

            let cluster_param = if network == "mainnet" { "" } else { "?cluster=devnet" };
            println!("\nView on Solana Explorer:");
            println!("https://explorer.solana.com/tx/{}{}", sig, cluster_param);
        }
        Err(e) => {
            println!("\nFailed to remove spending limit: {}", e);
        }
    }
}
