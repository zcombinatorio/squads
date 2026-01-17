//! Create Treasury + Mint Multisigs (matching DAO program structure)
//!
//! This script creates two multisigs that mirror the structure used in
//! programs/futarchy for DAO initialization:
//!
//! Treasury Multisig (2-of-3):
//!   - TREASURY_MULTISIG_KEY_A (protocol)
//!   - TREASURY_MULTISIG_KEY_B (protocol)
//!   - TREASURY_COSIGNER (hardcoded)
//!
//! Mint Multisig (2-of-2):
//!   - MINT_MULTISIG_KEY_A (protocol)
//!   - MINT_MULTISIG_KEY_B (protocol)
//!
//! Usage:
//!   cargo run --bin create-dao-multisigs              # Devnet
//!   cargo run --bin create-dao-multisigs -- mainnet   # Mainnet

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
    state::{Member, Permissions},
};
use std::env;

// ============================================================================
// PROTOCOL CONSTANTS (from programs/futarchy/src/constants.rs)
// ============================================================================

// Treasury multisig configuration
const TREASURY_MULTISIG_CONFIG_AUTH: &str = "HHroB8P1q3kijtyML9WPvfTXG8JicfmUoGZjVzam64PX";
const TREASURY_MULTISIG_KEY_A: &str = "HHroB8P1q3kijtyML9WPvfTXG8JicfmUoGZjVzam64PX";
const TREASURY_MULTISIG_KEY_B: &str = "3ogXyF6ovq5SqsneuGY6gHLG27NK6gw13SqfXMwRBYai";
const TREASURY_COSIGNER: &str = "Dobm8QnaCPQoc6koxC3wqBQqPTfDwspATb2u6EcWC9Aw";
const TREASURY_THRESHOLD: u16 = 2; // 2-of-3

// Mint multisig configuration
const MINT_MULTISIG_CONFIG_AUTH: &str = "Dobm8QnaCPQoc6koxC3wqBQqPTfDwspATb2u6EcWC9Aw";
const MINT_MULTISIG_KEY_A: &str = "Dobm8QnaCPQoc6koxC3wqBQqPTfDwspATb2u6EcWC9Aw";
const MINT_MULTISIG_KEY_B: &str = "2xrEGvtxXKujqnHceiSzYDTAbTJEX3yGGPJgywH7LmcD";
const MINT_THRESHOLD: u16 = 2; // 2-of-2

// All permissions mask (Initiate | Vote | Execute = 1 | 2 | 4 = 7)
const ALL_PERMISSIONS: u8 = 7;

// ============================================================================
// Network Configuration
// ============================================================================
const CREATOR_KEYPAIR_PATH: &str = "../member1.json";
const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";
const SQUADS_TREASURY_DEVNET: &str = "HM5y4mz3Bt9JY9mr1hkyhnvqxSH4H2u2451j7Hc2dtvK";
const SQUADS_TREASURY_MAINNET: &str = "5DH2e3cJmFpyi6mk65EGFediunm4ui6BiKNUNrhWtD1b";

fn main() {
    let args: Vec<String> = env::args().collect();

    let network = args.get(1).map(|s| s.as_str()).unwrap_or("devnet");
    let cosigner: Pubkey = TREASURY_COSIGNER.parse().unwrap();

    let (rpc_url, treasury_addr, cluster_param) = match network {
        "mainnet" => (MAINNET_RPC, SQUADS_TREASURY_MAINNET, ""),
        _ => (DEVNET_RPC, SQUADS_TREASURY_DEVNET, "?cluster=devnet"),
    };

    println!("=== Creating DAO Multisigs ({}) ===\n", network.to_uppercase());
    println!("Cosigner: {}\n", cosigner);

    // Connect to Solana
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    // Load creator keypair (pays for transactions)
    let creator = read_keypair_file(CREATOR_KEYPAIR_PATH)
        .expect("Failed to read member1.json - see CLAUDE.md for setup instructions");
    let creator_pubkey = creator.pubkey();

    // Check creator has funds
    let balance = client.get_balance(&creator_pubkey).expect("Failed to get balance");
    println!("Creator: {}", creator_pubkey);
    println!("Balance: {} SOL\n", balance as f64 / 1_000_000_000.0);

    if balance < 20_000_000 {
        // 0.02 SOL minimum (creating 2 multisigs)
        eprintln!("ERROR: Insufficient balance. Need at least 0.02 SOL for transaction fees.");
        eprintln!("Fund this wallet: {}", creator_pubkey);
        std::process::exit(1);
    }

    // Parse protocol keys
    let treasury_config_auth: Pubkey = TREASURY_MULTISIG_CONFIG_AUTH.parse().unwrap();
    let treasury_key_a: Pubkey = TREASURY_MULTISIG_KEY_A.parse().unwrap();
    let treasury_key_b: Pubkey = TREASURY_MULTISIG_KEY_B.parse().unwrap();
    let mint_config_auth: Pubkey = MINT_MULTISIG_CONFIG_AUTH.parse().unwrap();
    let mint_key_a: Pubkey = MINT_MULTISIG_KEY_A.parse().unwrap();
    let mint_key_b: Pubkey = MINT_MULTISIG_KEY_B.parse().unwrap();

    let squads_treasury: Pubkey = treasury_addr.parse().unwrap();
    let (program_config_pda, _) = get_program_config_pda(None);

    let all_permissions = Permissions { mask: ALL_PERMISSIONS };

    // ========================================================================
    // Create Treasury Multisig (2-of-3)
    // ========================================================================
    println!("Creating Treasury Multisig (2-of-3)...");

    let treasury_create_key = Keypair::new();
    let (treasury_multisig_pda, _) = get_multisig_pda(&treasury_create_key.pubkey(), None);

    let treasury_accounts = MultisigCreateAccountsV2 {
        program_config: program_config_pda,
        treasury: squads_treasury,
        multisig: treasury_multisig_pda,
        create_key: treasury_create_key.pubkey(),
        creator: creator_pubkey,
        system_program: system_program::ID,
    };

    let treasury_args = MultisigCreateArgsV2 {
        config_authority: Some(treasury_config_auth),
        threshold: TREASURY_THRESHOLD,
        members: vec![
            Member { key: treasury_key_a, permissions: all_permissions },
            Member { key: treasury_key_b, permissions: all_permissions },
            Member { key: cosigner, permissions: all_permissions },
        ],
        time_lock: 0,
        rent_collector: None,
        memo: None,
    };

    let treasury_ix = multisig_create_v2(treasury_accounts, treasury_args, None);

    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get blockhash");
    let treasury_tx = Transaction::new_signed_with_payer(
        &[treasury_ix],
        Some(&creator_pubkey),
        &[&creator, &treasury_create_key],
        recent_blockhash,
    );

    let treasury_sig = client
        .send_and_confirm_transaction(&treasury_tx)
        .expect("Failed to create treasury multisig");

    let (treasury_vault_pda, _) = get_vault_pda(&treasury_multisig_pda, 0, None);

    println!("  ✓ Treasury Multisig created: {}", treasury_multisig_pda);
    println!("  ✓ Treasury Vault: {}", treasury_vault_pda);
    println!("  ✓ Transaction: {}\n", treasury_sig);

    // ========================================================================
    // Create Mint Multisig (2-of-2)
    // ========================================================================
    println!("Creating Mint Multisig (2-of-2)...");

    let mint_create_key = Keypair::new();
    let (mint_multisig_pda, _) = get_multisig_pda(&mint_create_key.pubkey(), None);

    let mint_accounts = MultisigCreateAccountsV2 {
        program_config: program_config_pda,
        treasury: squads_treasury,
        multisig: mint_multisig_pda,
        create_key: mint_create_key.pubkey(),
        creator: creator_pubkey,
        system_program: system_program::ID,
    };

    let mint_args = MultisigCreateArgsV2 {
        config_authority: Some(mint_config_auth),
        threshold: MINT_THRESHOLD,
        members: vec![
            Member { key: mint_key_a, permissions: all_permissions },
            Member { key: mint_key_b, permissions: all_permissions },
        ],
        time_lock: 0,
        rent_collector: None,
        memo: None,
    };

    let mint_ix = multisig_create_v2(mint_accounts, mint_args, None);

    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get blockhash");
    let mint_tx = Transaction::new_signed_with_payer(
        &[mint_ix],
        Some(&creator_pubkey),
        &[&creator, &mint_create_key],
        recent_blockhash,
    );

    let mint_sig = client
        .send_and_confirm_transaction(&mint_tx)
        .expect("Failed to create mint multisig");

    let (mint_vault_pda, _) = get_vault_pda(&mint_multisig_pda, 0, None);

    println!("  ✓ Mint Multisig created: {}", mint_multisig_pda);
    println!("  ✓ Mint Vault: {}", mint_vault_pda);
    println!("  ✓ Transaction: {}\n", mint_sig);

    // ========================================================================
    // Summary
    // ========================================================================
    println!("========== SUCCESS ==========");
    println!("Network: {}\n", network.to_uppercase());

    println!("TREASURY MULTISIG (2-of-3):");
    println!("  Address: {}", treasury_multisig_pda);
    println!("  Vault:   {} (send funds here)", treasury_vault_pda);
    println!("  Config Authority: {}", treasury_config_auth);
    println!("  Members:");
    println!("    1. {} (Protocol Key A)", treasury_key_a);
    println!("    2. {} (Protocol Key B)", treasury_key_b);
    println!("    3. {} (Cosigner)", cosigner);
    println!();

    println!("MINT MULTISIG (2-of-2):");
    println!("  Address: {}", mint_multisig_pda);
    println!("  Vault:   {} (mint authority)", mint_vault_pda);
    println!("  Config Authority: {}", mint_config_auth);
    println!("  Members:");
    println!("    1. {} (Protocol Key A)", mint_key_a);
    println!("    2. {} (Protocol Key B)", mint_key_b);
    println!();

    println!("View on Squads App:");
    println!("  Treasury: https://v4.squads.so/squads/{}/home", treasury_multisig_pda);
    println!("  Mint:     https://v4.squads.so/squads/{}/home", mint_multisig_pda);
    println!();

    println!("View on Solana Explorer:");
    println!("  Treasury: https://explorer.solana.com/address/{}{}", treasury_multisig_pda, cluster_param);
    println!("  Mint:     https://explorer.solana.com/address/{}{}", mint_multisig_pda, cluster_param);
}
