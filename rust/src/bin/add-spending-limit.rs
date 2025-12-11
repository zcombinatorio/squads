//! Add a spending limit to a Squads v4 Multisig (config authority only)
//!
//! Usage:
//!   cargo run --bin add-spending-limit -- <multisig_address> <amount> <period> [options]
//!
//! Arguments:
//!   multisig_address  - The multisig PDA address
//!   amount            - Amount in lamports (for SOL) or smallest unit (for tokens)
//!   period            - Reset period: "one-time", "day", "week", or "month"
//!
//! Options:
//!   --mint <address>  - Token mint address (default: SOL)
//!   --vault <index>   - Vault index (default: 0)
//!   --members <addrs> - Comma-separated list of members who can use this limit
//!                       (default: all current multisig members)
//!   --destinations <addrs> - Comma-separated allowed destination addresses
//!                            (default: any destination)
//!   mainnet           - Use mainnet instead of devnet
//!
//! Examples:
//!   # 1 SOL daily limit on devnet
//!   cargo run --bin add-spending-limit -- BJbRt... 1000000000 day
//!
//!   # 100 USDC weekly limit on mainnet
//!   cargo run --bin add-spending-limit -- BJbRt... 100000000 week --mint EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v mainnet

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    system_program,
    transaction::Transaction,
};
use squads_multisig::anchor_lang::{AccountDeserialize, InstructionData};
use squads_multisig::pda::get_spending_limit_pda;
use squads_multisig::squads_multisig_program;
use squads_multisig::state::{Multisig, Period};
use std::env;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";

fn parse_period(s: &str) -> Option<Period> {
    match s.to_lowercase().as_str() {
        "one-time" | "onetime" | "once" => Some(Period::OneTime),
        "day" | "daily" => Some(Period::Day),
        "week" | "weekly" => Some(Period::Week),
        "month" | "monthly" => Some(Period::Month),
        _ => None,
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 4 {
        println!("Usage: cargo run --bin add-spending-limit -- <multisig_address> <amount> <period> [options]");
        println!();
        println!("Arguments:");
        println!("  multisig_address  - The multisig PDA address");
        println!("  amount            - Amount in lamports (for SOL) or smallest unit (for tokens)");
        println!("  period            - Reset period: \"one-time\", \"day\", \"week\", or \"month\"");
        println!();
        println!("Options:");
        println!("  --mint <address>  - Token mint address (default: SOL, i.e., Pubkey::default())");
        println!("  --vault <index>   - Vault index (default: 0)");
        println!("  --members <addrs> - Comma-separated list of members who can use this limit");
        println!("  --destinations <addrs> - Comma-separated allowed destination addresses");
        println!("  mainnet           - Use mainnet instead of devnet");
        println!();
        println!("Examples:");
        println!("  cargo run --bin add-spending-limit -- BJbRt... 1000000000 day");
        println!("  cargo run --bin add-spending-limit -- BJbRt... 100000000 week --mint EPjFWdd5... mainnet");
        return;
    }

    let multisig_pda: Pubkey = args[1].parse().expect("Invalid multisig address");
    let amount: u64 = args[2].parse().expect("Invalid amount");
    let period = parse_period(&args[3]).expect("Invalid period. Use: one-time, day, week, or month");

    // Parse optional arguments
    let mut mint = Pubkey::default(); // SOL
    let mut vault_index: u8 = 0;
    let mut specified_members: Option<Vec<Pubkey>> = None;
    let mut destinations: Vec<Pubkey> = Vec::new();
    let mut network = "devnet";

    let mut i = 4;
    while i < args.len() {
        match args[i].as_str() {
            "--mint" => {
                i += 1;
                mint = args[i].parse().expect("Invalid mint address");
            }
            "--vault" => {
                i += 1;
                vault_index = args[i].parse().expect("Invalid vault index");
            }
            "--members" => {
                i += 1;
                specified_members = Some(
                    args[i]
                        .split(',')
                        .map(|s| s.trim().parse().expect("Invalid member address"))
                        .collect(),
                );
            }
            "--destinations" => {
                i += 1;
                destinations = args[i]
                    .split(',')
                    .map(|s| s.trim().parse().expect("Invalid destination address"))
                    .collect();
            }
            "mainnet" => {
                network = "mainnet";
            }
            _ => {}
        }
        i += 1;
    }

    let rpc_url = match network {
        "mainnet" => MAINNET_RPC,
        _ => DEVNET_RPC,
    };

    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let config_authority = read_keypair_file("../member1.json").expect("Failed to read member1.json");

    // Fetch multisig to get members if not specified
    let multisig_account = client
        .get_account(&multisig_pda)
        .expect("Failed to fetch multisig account");
    let multisig = Multisig::try_deserialize(&mut multisig_account.data.as_slice())
        .expect("Failed to deserialize multisig");

    // Use specified members or default to all multisig members
    let mut members: Vec<Pubkey> = specified_members.unwrap_or_else(|| {
        multisig.members.iter().map(|m| m.key).collect()
    });
    // Members must be sorted for the spending limit invariant
    members.sort();

    // Generate a unique create_key for this spending limit
    let create_key = Keypair::new();
    let (spending_limit_pda, _) = get_spending_limit_pda(&multisig_pda, &create_key.pubkey(), None);

    println!("=== Add Spending Limit ({}) ===\n", network.to_uppercase());
    println!("Multisig: {}", multisig_pda);
    println!("Config Authority: {}", config_authority.pubkey());
    println!("Spending Limit PDA: {}", spending_limit_pda);
    println!("Create Key: {}", create_key.pubkey());
    println!();
    println!("Spending Limit Configuration:");
    println!("  Amount: {} (in smallest units)", amount);
    println!("  Period: {:?}", period);
    println!("  Mint: {} {}", mint, if mint == Pubkey::default() { "(SOL)" } else { "" });
    println!("  Vault Index: {}", vault_index);
    println!("  Members ({}):", members.len());
    for member in &members {
        println!("    - {}", member);
    }
    if destinations.is_empty() {
        println!("  Destinations: Any");
    } else {
        println!("  Destinations ({}):", destinations.len());
        for dest in &destinations {
            println!("    - {}", dest);
        }
    }

    let instruction_data = squads_multisig_program::instruction::MultisigAddSpendingLimit {
        args: squads_multisig_program::MultisigAddSpendingLimitArgs {
            create_key: create_key.pubkey(),
            vault_index,
            mint,
            amount,
            period,
            members,
            destinations,
            memo: None,
        },
    };

    // Account order from MultisigAddSpendingLimit struct:
    // 1. multisig (seeds verified)
    // 2. config_authority (signer)
    // 3. spending_limit (init, PDA)
    // 4. rent_payer (signer, mut)
    // 5. system_program
    let accounts = vec![
        AccountMeta::new_readonly(multisig_pda, false),
        AccountMeta::new_readonly(config_authority.pubkey(), true),
        AccountMeta::new(spending_limit_pda, false),
        AccountMeta::new(config_authority.pubkey(), true),
        AccountMeta::new_readonly(system_program::ID, false),
    ];

    let instruction = Instruction {
        program_id: squads_multisig_program::ID,
        accounts,
        data: instruction_data.data(),
    };

    println!("\nCreating spending limit...");

    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&config_authority.pubkey()),
        &[&config_authority],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&transaction) {
        Ok(sig) => {
            println!("\nSpending limit created successfully!");
            println!("Transaction: {}", sig);
            println!("\nSpending Limit Address: {}", spending_limit_pda);
            println!("Create Key (save this!): {}", create_key.pubkey());

            let cluster_param = if network == "mainnet" { "" } else { "?cluster=devnet" };
            println!("\nView on Solana Explorer:");
            println!("https://explorer.solana.com/tx/{}{}", sig, cluster_param);
        }
        Err(e) => {
            println!("\nFailed to create spending limit: {}", e);
        }
    }
}
