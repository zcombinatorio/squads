//! Vote reject on an active multisig proposal.
//!
//! Usage:
//!   cargo run --bin reject-proposal -- <multisig_address> <proposal_index> [mainnet]

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    transaction::Transaction,
};
use squads_multisig::anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use squads_multisig::pda::get_proposal_pda;
use squads_multisig::squads_multisig_program;
use squads_multisig::state::{Multisig, Proposal, ProposalStatus};
use std::env;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        println!("Usage: cargo run --bin reject-proposal -- <multisig_address> <proposal_index> [mainnet]");
        return;
    }

    let multisig_pda: Pubkey = args[1].parse().expect("Invalid multisig address");
    let proposal_index: u64 = args[2].parse().expect("Invalid proposal index");
    let network = args.get(3).map(|s| s.as_str()).unwrap_or("devnet");
    let rpc_url = match network { "mainnet" => MAINNET_RPC, _ => DEVNET_RPC };

    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let member = read_keypair_file("../member1.json").expect("Failed to read member1.json");

    let (proposal_pda, _) = get_proposal_pda(&multisig_pda, proposal_index, None);

    let multisig_account = client.get_account(&multisig_pda).expect("Failed to fetch multisig");
    let multisig = Multisig::try_deserialize(&mut multisig_account.data.as_slice())
        .expect("Failed to deserialize multisig");

    let proposal_account = client.get_account(&proposal_pda).expect("Failed to fetch proposal");
    let proposal = Proposal::try_deserialize(&mut proposal_account.data.as_slice())
        .expect("Failed to deserialize proposal");

    println!("=== Reject Proposal ({}) ===\n", network.to_uppercase());
    println!("Multisig: {}", multisig_pda);
    println!("Member:   {}", member.pubkey());
    println!("Proposal: {} (#{})", proposal_pda, proposal_index);

    let status_str = match &proposal.status {
        ProposalStatus::Draft { .. } => "Draft",
        ProposalStatus::Active { .. } => "Active",
        ProposalStatus::Rejected { .. } => "Rejected",
        ProposalStatus::Approved { .. } => "Approved",
        ProposalStatus::Executed { .. } => "Executed",
        ProposalStatus::Cancelled { .. } => "Cancelled",
        _ => "Unknown",
    };
    println!("Status:   {}", status_str);
    println!("Approvals: {} | Rejections: {}", proposal.approved.len(), proposal.rejected.len());

    if proposal.rejected.contains(&member.pubkey()) {
        println!("\nYou have already rejected this proposal.");
        return;
    }
    if !matches!(proposal.status, ProposalStatus::Active { .. }) {
        println!("\nError: Proposal must be Active to vote reject. Status: {}", status_str);
        return;
    }
    if multisig.is_member(member.pubkey()).is_none() {
        println!("\nError: {} is not a member of this multisig", member.pubkey());
        return;
    }

    let accounts = squads_multisig_program::accounts::ProposalVote {
        multisig: multisig_pda,
        proposal: proposal_pda,
        member: member.pubkey(),
    };
    let data = squads_multisig_program::instruction::ProposalReject {
        args: squads_multisig_program::instructions::ProposalVoteArgs { memo: None },
    };
    let instruction = Instruction {
        program_id: squads_multisig_program::ID,
        accounts: accounts.to_account_metas(Some(false)),
        data: data.data(),
    };

    println!("\nVoting reject...");
    let recent_blockhash = client.get_latest_blockhash().expect("blockhash");
    let tx = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&member.pubkey()),
        &[&member],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&tx) {
        Ok(sig) => {
            let new_rejected = proposal.rejected.len() + 1;
            let cutoff = (multisig.members.len() as u16) - multisig.threshold + 1;
            println!("\nReject vote recorded!");
            println!("Transaction: {}", sig);
            println!("Rejections now: {} (cutoff to mark Rejected: {})", new_rejected, cutoff);
            if new_rejected >= cutoff as usize {
                println!("Cutoff reached -> proposal status is Rejected.");
            } else {
                println!("{} more reject vote(s) needed to mark Rejected.", cutoff as usize - new_rejected);
            }
            let cluster_param = if network == "mainnet" { "" } else { "?cluster=devnet" };
            println!("https://explorer.solana.com/tx/{}{}", sig, cluster_param);
        }
        Err(e) => println!("\nFailed to vote reject: {}", e),
    }
}
