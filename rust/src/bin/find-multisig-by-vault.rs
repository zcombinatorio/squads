use solana_client::rpc_client::RpcClient;
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use solana_transaction_status::{EncodedTransaction, UiMessage, UiTransactionEncoding, option_serializer::OptionSerializer};
use squads_multisig::anchor_lang::AccountDeserialize;
use squads_multisig::pda::get_vault_pda;
use squads_multisig::state::Multisig;
use squads_multisig::squads_multisig_program;
use std::collections::HashSet;
use std::env;
use std::str::FromStr;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";

/// Mainnet goes through Helius (the public endpoint rate-limits the account scans
/// these tools do). The key is read from the environment so it stays out of source:
///   export HELIUS_API_KEY=...        # or
///   export MAINNET_RPC_URL=https://... # full override
fn mainnet_rpc() -> String {
    if let Ok(url) = env::var("MAINNET_RPC_URL") {
        if !url.is_empty() {
            return url;
        }
    }
    match env::var("HELIUS_API_KEY") {
        Ok(k) if !k.is_empty() => format!("https://mainnet.helius-rpc.com/?api-key={}", k),
        _ => {
            eprintln!("Error: set HELIUS_API_KEY (or MAINNET_RPC_URL) for mainnet access.");
            std::process::exit(1);
        }
    }
}

const SIG_LIMIT: usize = 25;
const VAULT_INDEX_SCAN: u8 = 32;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run --bin find-multisig-by-vault -- <vault_address> [devnet]");
        println!("Example: cargo run --bin find-multisig-by-vault -- EhYH2LaJ4oEgM8jk1kpQ6avwP3tMrvKPdJFDRpbMTZrf");
        return;
    }

    let vault: Pubkey = args[1].parse().expect("Invalid vault address");
    let network = args.get(2).map(|s| s.as_str()).unwrap_or("mainnet");
    let rpc_url = match network {
        "devnet" => DEVNET_RPC.to_string(),
        _ => mainnet_rpc(),
    };
    let squads_program_id = squads_multisig_program::ID;

    let client = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());

    println!("=== Find Multisig By Vault ({}) ===", network.to_uppercase());
    println!("Vault: {}", vault);
    println!("Squads program: {}\n", squads_program_id);

    let sigs = client
        .get_signatures_for_address_with_config(
            &vault,
            GetConfirmedSignaturesForAddress2Config {
                before: None,
                until: None,
                limit: Some(SIG_LIMIT),
                commitment: Some(CommitmentConfig::confirmed()),
            },
        )
        .expect("Failed to fetch signatures");

    if sigs.is_empty() {
        println!("No transactions found for this vault.");
        return;
    }
    println!("Found {} recent signatures. Scanning for Squads-owned accounts...\n", sigs.len());

    let mut candidates: HashSet<Pubkey> = HashSet::new();

    for entry in &sigs {
        let sig = match entry.signature.parse() {
            Ok(s) => s,
            Err(_) => continue,
        };
        let tx = match client.get_transaction_with_config(
            &sig,
            RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::Json),
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(0),
            },
        ) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("  skip {}: {}", entry.signature, e);
                continue;
            }
        };

        let mut keys: Vec<String> = Vec::new();
        if let EncodedTransaction::Json(ui_tx) = tx.transaction.transaction {
            if let UiMessage::Raw(raw) = ui_tx.message {
                keys.extend(raw.account_keys);
            }
        }
        if let Some(meta) = tx.transaction.meta {
            if let OptionSerializer::Some(loaded) = meta.loaded_addresses {
                keys.extend(loaded.writable);
                keys.extend(loaded.readonly);
            }
        }

        for k in keys {
            if let Ok(pk) = Pubkey::from_str(&k) {
                if pk != vault {
                    candidates.insert(pk);
                }
            }
        }
    }

    println!("Collected {} unique non-vault accounts. Fetching to find Squads-owned ones...", candidates.len());

    let candidate_vec: Vec<Pubkey> = candidates.into_iter().collect();
    let mut multisigs_found: Vec<Pubkey> = Vec::new();

    for chunk in candidate_vec.chunks(100) {
        let accounts = match client.get_multiple_accounts(chunk) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("  get_multiple_accounts error: {}", e);
                continue;
            }
        };
        for (pk, maybe_acct) in chunk.iter().zip(accounts.iter()) {
            let Some(acct) = maybe_acct else { continue };
            if acct.owner != squads_program_id {
                continue;
            }
            if Multisig::try_deserialize(&mut acct.data.as_slice()).is_ok() {
                multisigs_found.push(*pk);
            }
        }
    }

    if multisigs_found.is_empty() {
        println!("\nNo Squads-owned Multisig accounts referenced in recent vault transactions.");
        println!("Try increasing SIG_LIMIT or checking older transactions.");
        return;
    }

    println!("Candidate multisigs: {}\n", multisigs_found.len());

    for ms in &multisigs_found {
        for idx in 0..=VAULT_INDEX_SCAN {
            let (derived, _) = get_vault_pda(ms, idx, None);
            if derived == vault {
                println!("MATCH:");
                println!("  Multisig:    {}", ms);
                println!("  Vault index: {}", idx);
                return;
            }
        }
    }

    println!("None of the candidate multisigs derive to this vault for indices 0..={}", VAULT_INDEX_SCAN);
    println!("Candidates checked:");
    for ms in &multisigs_found {
        println!("  {}", ms);
    }
}
