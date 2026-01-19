//! Use a spending limit to transfer funds without proposal approval
//!
//! Usage:
//!   cargo run --bin use-spending-limit -- <spending_limit_address> <destination> <amount> [mainnet]
//!   cargo run --bin use-spending-limit -- --multisig <multisig_address> <destination> <amount> [mainnet]
//!
//! Arguments:
//!   spending_limit_address  - The spending limit PDA (or use --multisig to derive it)
//!   destination             - Destination wallet address
//!   amount                  - Amount in lamports (for SOL) or smallest unit (for tokens)
//!
//! Examples:
//!   # Transfer 0.1 SOL using spending limit PDA directly
//!   cargo run --bin use-spending-limit -- SpendingLimitPDA... DestWallet... 100000000
//!
//!   # Transfer using multisig address (derives spending limit via 'combinator')
//!   cargo run --bin use-spending-limit -- --multisig MultisigPDA... DestWallet... 100000000 mainnet

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    system_program,
    transaction::Transaction,
};
use spl_associated_token_account::{
    get_associated_token_address,
    instruction::create_associated_token_account_idempotent,
};
use squads_multisig::anchor_lang::{AccountDeserialize, InstructionData};
use squads_multisig::pda::{get_spending_limit_pda, get_vault_pda};
use squads_multisig::squads_multisig_program;
use squads_multisig::state::SpendingLimit;
use std::env;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";
const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 4 {
        println!("Usage: cargo run --bin use-spending-limit -- <spending_limit_address> <destination> <amount> [mainnet]");
        println!("       cargo run --bin use-spending-limit -- --multisig <multisig_address> <destination> <amount> [mainnet]");
        println!();
        println!("Arguments:");
        println!("  spending_limit_address  - The spending limit PDA (or use --multisig to derive it)");
        println!("  destination             - Destination wallet address");
        println!("  amount                  - Amount in lamports (for SOL) or smallest unit (for tokens)");
        println!();
        println!("Examples:");
        println!("  cargo run --bin use-spending-limit -- SpendingLimitPDA... DestWallet... 100000000");
        println!("  cargo run --bin use-spending-limit -- --multisig MultisigPDA... DestWallet... 100000000 mainnet");
        return;
    }

    // Check for --force flag anywhere in args
    let force = args.iter().any(|a| a == "--force");
    let args: Vec<String> = args.into_iter().filter(|a| a != "--force").collect();

    // Parse arguments - handle --multisig flag
    let (spending_limit_pda, destination, amount, network) = if args.get(1).map(|s| s.as_str()) == Some("--multisig") {
        if args.len() < 5 {
            println!("Error: --multisig requires: <multisig_address> <destination> <amount> [mainnet]");
            return;
        }
        let multisig_pda: Pubkey = args[2].parse().expect("Invalid multisig address");
        let dest: Pubkey = args[3].parse().expect("Invalid destination address");
        let amt: u64 = args[4].parse().expect("Invalid amount");
        let net = args.get(5).map(|s| s.as_str()).unwrap_or("devnet");

        // Derive spending limit PDA using "combinator" createKey
        let (create_key, _) = Pubkey::find_program_address(
            &[b"combinator"],
            &squads_multisig_program::ID,
        );
        let (spending_limit, _) = get_spending_limit_pda(&multisig_pda, &create_key, None);
        println!("Derived spending limit PDA: {}", spending_limit);
        (spending_limit, dest, amt, net)
    } else {
        let spending_limit: Pubkey = args[1].parse().expect("Invalid spending limit address");
        let dest: Pubkey = args[2].parse().expect("Invalid destination address");
        let amt: u64 = args[3].parse().expect("Invalid amount");
        let net = args.get(4).map(|s| s.as_str()).unwrap_or("devnet");
        (spending_limit, dest, amt, net)
    };

    let rpc_url = match network {
        "mainnet" => MAINNET_RPC,
        _ => DEVNET_RPC,
    };

    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let member = read_keypair_file("../member1.json").expect("Failed to read member1.json");

    // Fetch the spending limit to get multisig, vault_index, mint, and validate member
    let spending_limit_account = client
        .get_account(&spending_limit_pda)
        .expect("Failed to fetch spending limit account");
    let spending_limit = SpendingLimit::try_deserialize(&mut spending_limit_account.data.as_slice())
        .expect("Failed to deserialize spending limit");

    let multisig_pda = spending_limit.multisig;
    let vault_index = spending_limit.vault_index;
    let mint = spending_limit.mint;
    let is_sol = mint == Pubkey::default();

    // Validate member is authorized (skip with --force)
    if !force && !spending_limit.members.contains(&member.pubkey()) {
        println!("Error: Your wallet {} is not authorized to use this spending limit", member.pubkey());
        println!();
        println!("Authorized members:");
        for m in &spending_limit.members {
            println!("  - {}", m);
        }
        return;
    }

    // Validate destination if restricted (skip with --force)
    if !force && !spending_limit.destinations.is_empty() && !spending_limit.destinations.contains(&destination) {
        println!("Error: Destination {} is not in the allowed destinations list", destination);
        println!();
        println!("Allowed destinations:");
        for d in &spending_limit.destinations {
            println!("  - {}", d);
        }
        return;
    }

    // Check remaining amount (skip with --force to test on-chain validation)
    if !force && amount > spending_limit.remaining_amount {
        println!("Error: Requested amount {} exceeds remaining limit {}", amount, spending_limit.remaining_amount);
        if is_sol {
            println!("  Requested: {:.9} SOL", amount as f64 / LAMPORTS_PER_SOL);
            println!("  Remaining: {:.9} SOL", spending_limit.remaining_amount as f64 / LAMPORTS_PER_SOL);
        }
        return;
    }

    if force {
        println!("WARNING: --force flag used, skipping local validation");
    }

    // Derive vault PDA
    let (vault_pda, _) = get_vault_pda(&multisig_pda, vault_index, None);

    println!("=== Use Spending Limit ({}) ===\n", network.to_uppercase());
    println!("Spending Limit: {}", spending_limit_pda);
    println!("Multisig: {}", multisig_pda);
    println!("Vault: {}", vault_pda);
    println!("Member: {}", member.pubkey());
    println!();
    if is_sol {
        println!("Token: SOL (Native)");
        println!("Amount: {} lamports ({:.9} SOL)", amount, amount as f64 / LAMPORTS_PER_SOL);
        let remaining_after = spending_limit.remaining_amount.saturating_sub(amount);
        println!("Remaining after: {} lamports ({:.9} SOL)",
            remaining_after,
            remaining_after as f64 / LAMPORTS_PER_SOL
        );
    } else {
        println!("Mint: {}", mint);
        println!("Amount: {}", amount);
        println!("Remaining after: {}", spending_limit.remaining_amount.saturating_sub(amount));
    }
    println!("Destination: {}", destination);
    println!("Period: {:?}", spending_limit.period);

    // Build the instruction
    let decimals = if is_sol { 9 } else {
        // Fetch mint to get decimals
        let mint_account = client.get_account(&mint).expect("Failed to fetch mint account");
        // SPL Token mint has decimals at offset 44
        mint_account.data[44]
    };

    let instruction_data = squads_multisig_program::instruction::SpendingLimitUse {
        args: squads_multisig_program::SpendingLimitUseArgs {
            amount,
            decimals,
            memo: None,
        },
    };

    // Build accounts based on whether it's SOL or SPL token
    let accounts = if is_sol {
        // SOL transfer accounts
        vec![
            AccountMeta::new_readonly(multisig_pda, false),
            AccountMeta::new_readonly(member.pubkey(), true),
            AccountMeta::new(spending_limit_pda, false),
            AccountMeta::new(vault_pda, false),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ]
    } else {
        // SPL token transfer accounts
        let vault_token_account = get_associated_token_address(&vault_pda, &mint);
        let destination_token_account = get_associated_token_address(&destination, &mint);

        vec![
            AccountMeta::new_readonly(multisig_pda, false),
            AccountMeta::new_readonly(member.pubkey(), true),
            AccountMeta::new(spending_limit_pda, false),
            AccountMeta::new(vault_pda, false),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(system_program::ID, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault_token_account, false),
            AccountMeta::new(destination_token_account, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ]
    };

    let spending_limit_ix = Instruction {
        program_id: squads_multisig_program::ID,
        accounts,
        data: instruction_data.data(),
    };

    // Build instructions - for SPL tokens, create destination ATA if needed
    let instructions = if is_sol {
        vec![spending_limit_ix]
    } else {
        let destination_token_account = get_associated_token_address(&destination, &mint);
        let create_ata_ix = create_associated_token_account_idempotent(
            &member.pubkey(),
            &destination,
            &mint,
            &spl_token::ID,
        );
        println!("Will create destination ATA if needed: {}", destination_token_account);
        vec![create_ata_ix, spending_limit_ix]
    };

    println!("\nExecuting transfer...");

    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&member.pubkey()),
        &[&member],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&transaction) {
        Ok(sig) => {
            println!("\nTransfer successful!");
            println!("Transaction: {}", sig);

            let cluster_param = if network == "mainnet" { "" } else { "?cluster=devnet" };
            println!("\nView on Solana Explorer:");
            println!("https://explorer.solana.com/tx/{}{}", sig, cluster_param);
        }
        Err(e) => {
            println!("\nTransfer failed: {}", e);
            println!();
            println!("Common issues:");
            println!("  - Spending limit may have reset (check period)");
            println!("  - Vault may not have sufficient funds");
            println!("  - For SPL tokens, destination token account may not exist");
        }
    }
}
