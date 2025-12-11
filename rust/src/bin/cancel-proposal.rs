//! Cancel a multisig proposal
//!
//! Members can vote to cancel an approved proposal before it's executed.
//! Once enough members vote to cancel (reaching threshold), the proposal is cancelled.
//!
//! Usage:
//!   cargo run --bin cancel-proposal -- <multisig_address> <proposal_index> [mainnet]
//!
//! Example:
//!   cargo run --bin cancel-proposal -- BJbRt... 1 mainnet

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
        println!("Usage: cargo run --bin cancel-proposal -- <multisig_address> <proposal_index> [mainnet]");
        println!();
        println!("Example:");
        println!("  cargo run --bin cancel-proposal -- BJbRt... 1 mainnet");
        return;
    }

    let multisig_pda: Pubkey = args[1].parse().expect("Invalid multisig address");
    let proposal_index: u64 = args[2].parse().expect("Invalid proposal index");
    let network = args.get(3).map(|s| s.as_str()).unwrap_or("devnet");

    let rpc_url = match network {
        "mainnet" => MAINNET_RPC,
        _ => DEVNET_RPC,
    };

    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let member = read_keypair_file("../member1.json").expect("Failed to read member1.json");

    // Derive proposal PDA
    let (proposal_pda, _) = get_proposal_pda(&multisig_pda, proposal_index, None);

    // Fetch multisig info
    let multisig_account = client
        .get_account(&multisig_pda)
        .expect("Failed to fetch multisig account");
    let multisig = Multisig::try_deserialize(&mut multisig_account.data.as_slice())
        .expect("Failed to deserialize multisig");

    // Fetch proposal info
    let proposal_account = client
        .get_account(&proposal_pda)
        .expect("Failed to fetch proposal account. Does this proposal exist?");
    let proposal = Proposal::try_deserialize(&mut proposal_account.data.as_slice())
        .expect("Failed to deserialize proposal");

    println!("=== Cancel Proposal ({}) ===\n", network.to_uppercase());
    println!("Multisig: {}", multisig_pda);
    println!("Member: {}", member.pubkey());
    println!();
    println!("Proposal Index: {}", proposal_index);
    println!("Proposal Address: {}", proposal_pda);

    // Check proposal status
    let status_str = match &proposal.status {
        ProposalStatus::Draft { .. } => "Draft",
        ProposalStatus::Active { .. } => "Active",
        ProposalStatus::Rejected { .. } => "Rejected",
        ProposalStatus::Approved { .. } => "Approved",
        ProposalStatus::Executed { .. } => "Executed",
        ProposalStatus::Cancelled { .. } => "Cancelled",
        _ => "Unknown",
    };
    println!("Status: {}", status_str);
    println!();

    // Show current cancel votes
    println!("Current Cancel Votes: {} of {} required", proposal.cancelled.len(), multisig.threshold);
    for canceller in &proposal.cancelled {
        println!("  - {}", canceller);
    }

    // Check if member already voted to cancel
    if proposal.cancelled.contains(&member.pubkey()) {
        println!("\nYou have already voted to cancel this proposal!");
        return;
    }

    // Check if proposal can be cancelled (must be Approved)
    if !matches!(proposal.status, ProposalStatus::Approved { .. }) {
        println!("\nError: Only approved proposals can be cancelled. Current status: {}", status_str);
        return;
    }

    // Check if member is part of multisig
    if multisig.is_member(member.pubkey()).is_none() {
        println!("\nError: {} is not a member of this multisig", member.pubkey());
        return;
    }

    let accounts = squads_multisig_program::accounts::ProposalVote {
        multisig: multisig_pda,
        proposal: proposal_pda,
        member: member.pubkey(),
    };

    let data = squads_multisig_program::instruction::ProposalCancel {
        args: squads_multisig_program::instructions::ProposalVoteArgs { memo: None },
    };

    let instruction = Instruction {
        program_id: squads_multisig_program::ID,
        accounts: accounts.to_account_metas(Some(false)),
        data: data.data(),
    };

    println!("\nVoting to cancel proposal...");

    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&member.pubkey()),
        &[&member],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&transaction) {
        Ok(sig) => {
            let new_cancel_count = proposal.cancelled.len() + 1;
            println!("\nCancel vote recorded!");
            println!("Transaction: {}", sig);
            println!();
            println!("Cancel Votes: {} of {} required", new_cancel_count, multisig.threshold);

            if new_cancel_count >= multisig.threshold as usize {
                println!("\nThreshold reached! The proposal has been cancelled.");
            } else {
                let remaining = multisig.threshold as usize - new_cancel_count;
                println!("\n{} more cancel vote(s) needed to cancel the proposal.", remaining);
            }

            let cluster_param = if network == "mainnet" { "" } else { "?cluster=devnet" };
            println!("\nView on Solana Explorer:");
            println!("https://explorer.solana.com/tx/{}{}", sig, cluster_param);
        }
        Err(e) => {
            println!("\nFailed to vote cancel: {}", e);
        }
    }
}
