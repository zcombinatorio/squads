use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use squads_multisig::anchor_lang::AccountDeserialize;
use squads_multisig::pda::get_proposal_pda;
use squads_multisig::state::{Multisig, Proposal, ProposalStatus};
use std::env;

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

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run --bin list-proposals -- <multisig_address> [devnet]");
        return;
    }
    let multisig_pda: Pubkey = args[1].parse().expect("Invalid multisig address");
    let network = args.get(2).map(|s| s.as_str()).unwrap_or("mainnet");
    let rpc_url = match network {
        "devnet" => DEVNET_RPC.to_string(),
        _ => mainnet_rpc(),
    };
    let client = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());

    let multisig_acct = client.get_account(&multisig_pda).expect("multisig fetch failed");
    let multisig = Multisig::try_deserialize(&mut multisig_acct.data.as_slice())
        .expect("multisig deserialize failed");

    println!("=== Proposals for {} ({}) ===", multisig_pda, network.to_uppercase());
    println!("Threshold: {} of {}", multisig.threshold, multisig.members.len());
    println!("Latest tx index: {}\n", multisig.transaction_index);

    if multisig.transaction_index == 0 {
        println!("No proposals.");
        return;
    }

    for idx in 1..=multisig.transaction_index {
        let (proposal_pda, _) = get_proposal_pda(&multisig_pda, idx, None);
        match client.get_account(&proposal_pda) {
            Ok(acct) => match Proposal::try_deserialize(&mut acct.data.as_slice()) {
                Ok(p) => {
                    let status = match &p.status {
                        ProposalStatus::Draft { .. } => "Draft",
                        ProposalStatus::Active { .. } => "Active",
                        ProposalStatus::Rejected { .. } => "Rejected",
                        ProposalStatus::Approved { .. } => "Approved",
                        ProposalStatus::Executed { .. } => "Executed",
                        ProposalStatus::Cancelled { .. } => "Cancelled",
                        _ => "Unknown",
                    };
                    println!("Proposal #{} [{}]", idx, status);
                    println!("  PDA:        {}", proposal_pda);
                    println!("  Approvals:  {} ({})", p.approved.len(), p.approved.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(", "));
                    println!("  Rejections: {} ({})", p.rejected.len(), p.rejected.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(", "));
                    println!("  Cancelled:  {}", p.cancelled.len());
                    if matches!(p.status, ProposalStatus::Active { .. }) {
                        let needed = (multisig.threshold as usize).saturating_sub(p.approved.len());
                        println!("  >>> ACTIVE — needs {} more approval(s)", needed);
                    }
                    println!();
                }
                Err(e) => println!("Proposal #{} ({}): deserialize failed: {}\n", idx, proposal_pda, e),
            },
            Err(_) => println!("Proposal #{} ({}): no account (tx never had a proposal created)\n", idx, proposal_pda),
        }
    }
}
