//! Create a 3/5 Squads v4 Multisig
//!
//! This script creates a multisig wallet with:
//! - 3 of 5 threshold (3 signatures required)
//! - Config authority (can modify settings without proposals)
//! - 5 members with full permissions (Initiate, Vote, Execute)
//!
//! Usage:
//!   cargo run              # Creates on devnet (default)
//!   cargo run -- mainnet   # Creates on mainnet

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
    system_program,
    transaction::Transaction,
};
use squads_multisig::{
    client::{multisig_create_v2, MultisigCreateAccountsV2, MultisigCreateArgsV2},
    pda::{get_multisig_pda, get_program_config_pda, get_vault_pda},
    state::{Member, Permission, Permissions},
};
use std::env;

// ============================================================================
// CONFIGURATION - Edit these values before running
// ============================================================================

/// Member 1: Config authority and creator (must have keypair file)
/// This wallet pays for the transaction and becomes config authority
const MEMBER1_KEYPAIR_PATH: &str = "../member1.json";

/// Members 2-5: Add your team's wallet addresses here
const MEMBER2: &str = "7iforHAXo5q5kL8M8fMTLnqQHEbPrdFuzoZbLENVnRNM";
const MEMBER3: &str = "4iNr6EePYbrrDHw6GHVTVnEsCWEhNpNhnM7mCafe8Ya9";
const MEMBER4: &str = "KyzmrMzqWdTyJh87kgwDVswYTkdydBECCnUANn7Xxkh";
const MEMBER5: &str = "53Sb8FiUTRJbqs6SC5KgbMLqfwT98qPPTVroodLJKQ9m";

/// Signature threshold (how many approvals needed)
const THRESHOLD: u16 = 3;

// ============================================================================
// Network Configuration (don't edit unless you know what you're doing)
// ============================================================================
const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";

const SQUADS_TREASURY_DEVNET: &str = "HM5y4mz3Bt9JY9mr1hkyhnvqxSH4H2u2451j7Hc2dtvK";
const SQUADS_TREASURY_MAINNET: &str = "5DH2e3cJmFpyi6mk65EGFediunm4ui6BiKNUNrhWtD1b";

fn main() {
    // Parse CLI args: cargo run -- mainnet OR cargo run (devnet default)
    let args: Vec<String> = env::args().collect();
    let network = args.get(1).map(|s| s.as_str()).unwrap_or("devnet");

    let (rpc_url, treasury_addr, cluster_param) = match network {
        "mainnet" => (MAINNET_RPC, SQUADS_TREASURY_MAINNET, ""),
        _ => (DEVNET_RPC, SQUADS_TREASURY_DEVNET, "?cluster=devnet"),
    };

    println!("=== Creating {}/{} Multisig ({}) ===\n", THRESHOLD, 5, network.to_uppercase());

    // Connect to Solana
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    // Load member1 keypair (creator and config authority)
    let member1 = read_keypair_file(MEMBER1_KEYPAIR_PATH)
        .expect("Failed to read member1.json - see CLAUDE.md for setup instructions");

    // Parse all member addresses
    let member1_pubkey = member1.pubkey();
    let member2_pubkey: Pubkey = MEMBER2.parse().expect("Invalid MEMBER2 address");
    let member3_pubkey: Pubkey = MEMBER3.parse().expect("Invalid MEMBER3 address");
    let member4_pubkey: Pubkey = MEMBER4.parse().expect("Invalid MEMBER4 address");
    let member5_pubkey: Pubkey = MEMBER5.parse().expect("Invalid MEMBER5 address");

    // Check creator has funds for transaction
    let balance = client.get_balance(&member1_pubkey).expect("Failed to get balance");
    println!("Creator: {}", member1_pubkey);
    println!("Balance: {} SOL\n", balance as f64 / 1_000_000_000.0);

    if balance < 10_000_000 {
        // 0.01 SOL minimum
        println!("ERROR: Insufficient balance. Need at least 0.01 SOL for transaction fees.");
        println!("Fund this wallet: {}", member1_pubkey);
        return;
    }

    // Generate unique create_key for this multisig
    let create_key = Keypair::new();

    // Derive PDAs
    let (multisig_pda, _) = get_multisig_pda(&create_key.pubkey(), None);
    let (program_config_pda, _) = get_program_config_pda(None);
    let treasury: Pubkey = treasury_addr.parse().unwrap();

    // All members get full permissions
    let all_permissions = Permissions {
        mask: Permission::Initiate as u8 | Permission::Vote as u8 | Permission::Execute as u8,
    };

    // Build multisig creation accounts
    let accounts = MultisigCreateAccountsV2 {
        program_config: program_config_pda,
        treasury,
        multisig: multisig_pda,
        create_key: create_key.pubkey(),
        creator: member1_pubkey,
        system_program: system_program::ID,
    };

    // Build multisig creation args
    let args = MultisigCreateArgsV2 {
        config_authority: Some(member1_pubkey), // Member1 can change settings without proposals
        threshold: THRESHOLD,
        members: vec![
            Member { key: member1_pubkey, permissions: all_permissions },
            Member { key: member2_pubkey, permissions: all_permissions },
            Member { key: member3_pubkey, permissions: all_permissions },
            Member { key: member4_pubkey, permissions: all_permissions },
            Member { key: member5_pubkey, permissions: all_permissions },
        ],
        time_lock: 0,         // No time lock on execution
        rent_collector: None, // No rent collection
        memo: None,
    };

    // Create the instruction
    let instruction = multisig_create_v2(accounts, args, None);

    println!("Creating multisig...");

    // Build and send transaction
    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&member1_pubkey),
        &[&member1, &create_key],
        recent_blockhash,
    );

    let signature = client
        .send_and_confirm_transaction(&transaction)
        .expect("Failed to create multisig");

    // Get vault address (where funds are stored)
    let (vault_pda, _) = get_vault_pda(&multisig_pda, 0, None);

    // Print summary
    println!("\n========== SUCCESS ==========");
    println!("Network: {}", network.to_uppercase());
    println!("Multisig Address: {}", multisig_pda);
    println!("Vault Address: {} (send funds here)", vault_pda);
    println!("Config Authority: {}", member1_pubkey);
    println!("Threshold: {} of 5", THRESHOLD);
    println!("\nMembers:");
    println!("  1. {} (Config Authority)", member1_pubkey);
    println!("  2. {}", member2_pubkey);
    println!("  3. {}", member3_pubkey);
    println!("  4. {}", member4_pubkey);
    println!("  5. {}", member5_pubkey);
    println!("\nTransaction: {}", signature);
    println!("\nView on Solana Explorer:");
    println!("https://explorer.solana.com/address/{}{}", multisig_pda, cluster_param);
    println!("\nView on Squads App:");
    println!("https://v4.squads.so/squads/{}/home", multisig_pda);
}
