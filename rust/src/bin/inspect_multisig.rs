use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use squads_multisig::anchor_lang::AccountDeserialize;
use squads_multisig::state::Multisig;
use std::env;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Usage: cargo run --bin inspect_multisig -- <multisig_address> [mainnet]");
        println!("Example: cargo run --bin inspect_multisig -- BJbRtXM8wecvRrJNbbpNLfuG8FTSoU6zPYW1NFrMH6Q3 mainnet");
        return;
    }

    let multisig_pda: Pubkey = args[1].parse().expect("Invalid multisig address");
    let network = args.get(2).map(|s| s.as_str()).unwrap_or("devnet");

    let rpc_url = match network {
        "mainnet" => MAINNET_RPC,
        _ => DEVNET_RPC,
    };

    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());

    println!("=== Multisig Info ({}) ===\n", network.to_uppercase());

    match client.get_account(&multisig_pda) {
        Ok(account) => {
            match Multisig::try_deserialize(&mut account.data.as_slice()) {
                Ok(multisig) => {
                    println!("Multisig Address: {}", multisig_pda);
                    println!("Threshold: {} of {}", multisig.threshold, multisig.members.len());
                    println!("Time Lock: {} seconds", multisig.time_lock);

                    // Config authority - check if it's the default (all zeros = None)
                    let config_auth = multisig.config_authority;
                    if config_auth == Pubkey::default() {
                        println!("Config Authority: None (autonomous)");
                    } else {
                        println!("Config Authority: {}", config_auth);
                    }

                    // Rent collector
                    match multisig.rent_collector {
                        Some(rc) => println!("Rent Collector: {}", rc),
                        None => println!("Rent Collector: None"),
                    }

                    println!("\nMembers:");
                    for (i, member) in multisig.members.iter().enumerate() {
                        let perms = member.permissions.mask;
                        let perm_str = format!(
                            "{}{}{}",
                            if perms & 1 != 0 { "Initiate " } else { "" },
                            if perms & 2 != 0 { "Vote " } else { "" },
                            if perms & 4 != 0 { "Execute" } else { "" }
                        );
                        println!("  {}. {} [{}]", i + 1, member.key, perm_str.trim());
                    }

                    println!("\nTransaction Index: {}", multisig.transaction_index);
                    println!("Stale Transaction Index: {}", multisig.stale_transaction_index);
                }
                Err(e) => println!("Failed to deserialize multisig: {}", e),
            }
        }
        Err(e) => println!("Error fetching account: {}", e),
    }
}
