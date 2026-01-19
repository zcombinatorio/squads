//! Inspect spending limits for a Squads v4 Multisig
//!
//! Usage:
//!   # Inspect a specific spending limit by address
//!   cargo run --bin inspect-spending-limit -- <spending_limit_address> [mainnet]
//!
//!   # Derive and inspect spending limit for a multisig (uses 'combinator' create_key)
//!   cargo run --bin inspect-spending-limit -- --multisig <multisig_address> [mainnet]
//!
//! Examples:
//!   cargo run --bin inspect-spending-limit -- SpendingLimitPDA...
//!   cargo run --bin inspect-spending-limit -- SpendingLimitPDA... mainnet
//!   cargo run --bin inspect-spending-limit -- --multisig MultisigPDA... mainnet

use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use squads_multisig::anchor_lang::AccountDeserialize;
use squads_multisig::pda::get_spending_limit_pda;
use squads_multisig::squads_multisig_program;
use squads_multisig::state::SpendingLimit;
use std::env;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";
const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

fn format_period(period: &squads_multisig::state::Period) -> &'static str {
    match period {
        squads_multisig::state::Period::OneTime => "One-Time",
        squads_multisig::state::Period::Day => "Daily",
        squads_multisig::state::Period::Week => "Weekly",
        squads_multisig::state::Period::Month => "Monthly",
    }
}

fn print_spending_limit(pubkey: &Pubkey, limit: &SpendingLimit, index: Option<usize>, network: &str) {
    let is_sol = limit.mint == Pubkey::default();

    if let Some(i) = index {
        println!("\n[Spending Limit #{}]", i + 1);
    }
    println!("Address: {}", pubkey);
    println!("Multisig: {}", limit.multisig);
    println!();

    // Token info
    if is_sol {
        println!("Token:       SOL (Native)");
        println!(
            "Amount:      {:.9} SOL ({} lamports)",
            limit.amount as f64 / LAMPORTS_PER_SOL,
            limit.amount
        );
        println!(
            "Remaining:   {:.9} SOL ({} lamports)",
            limit.remaining_amount as f64 / LAMPORTS_PER_SOL,
            limit.remaining_amount
        );
    } else {
        println!("Mint:        {}", limit.mint);
        println!("Amount:      {}", limit.amount);
        println!("Remaining:   {}", limit.remaining_amount);
    }

    // Usage stats
    let used = limit.amount.saturating_sub(limit.remaining_amount);
    let usage_pct = if limit.amount > 0 {
        (used as f64 / limit.amount as f64) * 100.0
    } else {
        0.0
    };
    println!("Used:        {:.1}%", usage_pct);

    println!("Period:      {}", format_period(&limit.period));
    println!("Vault Index: {}", limit.vault_index);
    println!("Last Reset:  slot {}", limit.last_reset);

    // Members
    if limit.members.is_empty() {
        println!("Members:     (none)");
    } else if limit.members.len() == 1 {
        println!("Members:     {}", limit.members[0]);
    } else {
        println!("Members:     {} addresses", limit.members.len());
        for member in &limit.members {
            println!("             - {}", member);
        }
    }

    // Destinations
    if limit.destinations.is_empty() {
        println!("Destinations: (any)");
    } else {
        println!("Destinations: {} restricted", limit.destinations.len());
        for dest in &limit.destinations {
            println!("             - {}", dest);
        }
    }

    // Explorer link
    let cluster_param = if network == "mainnet" { "" } else { "?cluster=devnet" };
    println!();
    println!(
        "Explorer: https://explorer.solana.com/address/{}{}",
        pubkey, cluster_param
    );
}

fn inspect_single(client: &RpcClient, spending_limit_pda: Pubkey, network: &str) {
    println!("=== Spending Limit Details ({}) ===\n", network.to_uppercase());

    match client.get_account(&spending_limit_pda) {
        Ok(account) => {
            match SpendingLimit::try_deserialize(&mut account.data.as_slice()) {
                Ok(limit) => {
                    print_spending_limit(&spending_limit_pda, &limit, None, network);
                }
                Err(e) => {
                    println!("Error: Failed to deserialize spending limit account");
                    println!("Details: {}", e);
                    println!();
                    println!("This may not be a valid Squads spending limit account.");
                }
            }
        }
        Err(e) => {
            println!("Error: Failed to fetch account");
            println!("Details: {}", e);
        }
    }
}

fn inspect_multisig(client: &RpcClient, multisig_pda: Pubkey, network: &str) {
    println!("=== Spending Limit for Multisig ({}) ===\n", network.to_uppercase());
    println!("Multisig: {}", multisig_pda);

    // Derive the spending limit PDA using the deterministic "combinator" create_key
    let (create_key, _) = Pubkey::find_program_address(
        &[b"combinator"],
        &squads_multisig_program::ID,
    );
    let (spending_limit_pda, _) = get_spending_limit_pda(&multisig_pda, &create_key, None);

    println!("Create Key: {} (derived from 'combinator')", create_key);
    println!("Spending Limit PDA: {}", spending_limit_pda);
    println!();

    match client.get_account(&spending_limit_pda) {
        Ok(account) => {
            match SpendingLimit::try_deserialize(&mut account.data.as_slice()) {
                Ok(limit) => {
                    print_spending_limit(&spending_limit_pda, &limit, None, network);
                }
                Err(e) => {
                    println!("Error: Failed to deserialize spending limit account");
                    println!("Details: {}", e);
                    println!();
                    println!("This may not be a valid Squads spending limit account.");
                }
            }
        }
        Err(e) => {
            println!("No spending limit found for this multisig.");
            println!();
            println!("The spending limit PDA does not exist, which means either:");
            println!("  1. No spending limit has been created for this multisig yet");
            println!("  2. The spending limit was created with a different create_key");
            println!();
            println!("To create a spending limit:");
            println!("  cargo run --bin add-spending-limit -- {} <amount> <period> [mainnet]", multisig_pda);
            println!();
            println!("RPC error: {}", e);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Usage:");
        println!("  # Inspect a specific spending limit");
        println!("  cargo run --bin inspect-spending-limit -- <spending_limit_address> [mainnet]");
        println!();
        println!("  # List all spending limits for a multisig");
        println!("  cargo run --bin inspect-spending-limit -- --multisig <multisig_address> [mainnet]");
        println!();
        println!("Examples:");
        println!("  cargo run --bin inspect-spending-limit -- SpendingLimitPDA...");
        println!("  cargo run --bin inspect-spending-limit -- --multisig MultisigPDA... mainnet");
        return;
    }

    // Parse arguments
    let is_multisig_mode = args.get(1).map(|s| s == "--multisig").unwrap_or(false);

    let (address_str, network) = if is_multisig_mode {
        if args.len() < 3 {
            println!("Error: --multisig requires an address");
            return;
        }
        (args[2].as_str(), args.get(3).map(|s| s.as_str()).unwrap_or("devnet"))
    } else {
        (args[1].as_str(), args.get(2).map(|s| s.as_str()).unwrap_or("devnet"))
    };

    let address: Pubkey = match address_str.parse() {
        Ok(pk) => pk,
        Err(_) => {
            println!("Error: Invalid address: {}", address_str);
            return;
        }
    };

    let rpc_url = match network {
        "mainnet" => MAINNET_RPC,
        _ => DEVNET_RPC,
    };

    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    if is_multisig_mode {
        inspect_multisig(&client, address, network);
    } else {
        inspect_single(&client, address, network);
    }
}
