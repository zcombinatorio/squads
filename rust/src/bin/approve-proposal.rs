//! Approve a multisig proposal
//!
//! Members use this to vote "approve" on an active proposal.
//! Once threshold approvals are reached, the proposal can be executed.
//!
//! Usage:
//!   cargo run --bin approve-proposal -- <multisig_address> <proposal_index> [mainnet]
//!
//! Example:
//!   cargo run --bin approve-proposal -- BJbRt... 1 mainnet

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
        println!("Usage: cargo run --bin approve-proposal -- <multisig_address> <proposal_index> [mainnet]");
        println!();
        println!("Example:");
        println!("  cargo run --bin approve-proposal -- BJbRt... 1 mainnet");
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

    println!("=== Approve Proposal ({}) ===\n", network.to_uppercase());
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

    // Show current votes
    println!("Current Approvals: {} of {} required", proposal.approved.len(), multisig.threshold);
    for approver in &proposal.approved {
        println!("  - {}", approver);
    }

    // Check if member already approved
    if proposal.approved.contains(&member.pubkey()) {
        println!("\nYou have already approved this proposal!");
        return;
    }

    // Check if proposal is active
    if !matches!(proposal.status, ProposalStatus::Active { .. }) {
        println!("\nError: Proposal is not active. Current status: {}", status_str);
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

    let data = squads_multisig_program::instruction::ProposalApprove {
        args: squads_multisig_program::instructions::ProposalVoteArgs { memo: None },
    };

    let instruction = Instruction {
        program_id: squads_multisig_program::ID,
        accounts: accounts.to_account_metas(Some(false)),
        data: data.data(),
    };

    println!("\nApproving proposal...");

    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&member.pubkey()),
        &[&member],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&transaction) {
        Ok(sig) => {
            let new_approval_count = proposal.approved.len() + 1;
            println!("\nProposal approved successfully!");
            println!("Transaction: {}", sig);
            println!();
            println!("Approvals: {} of {} required", new_approval_count, multisig.threshold);

            if new_approval_count >= multisig.threshold as usize {
                println!("\nThreshold reached! The proposal can now be executed:");
                println!("  cargo run --bin execute-proposal -- {} {} {}",
                         multisig_pda, proposal_index, if network == "mainnet" { "mainnet" } else { "" });
            } else {
                let remaining = multisig.threshold as usize - new_approval_count;
                println!("\n{} more approval(s) needed before execution.", remaining);
            }

            let cluster_param = if network == "mainnet" { "" } else { "?cluster=devnet" };
            println!("\nView on Solana Explorer:");
            println!("https://explorer.solana.com/tx/{}{}", sig, cluster_param);
        }
        Err(e) => {
            println!("\nFailed to approve proposal: {}", e);
        }
    }
}
