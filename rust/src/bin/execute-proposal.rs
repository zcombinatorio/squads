//! Execute an approved multisig proposal
//!
//! Once a proposal has reached the required threshold of approvals,
//! any member with Execute permission can execute it.
//!
//! Usage:
//!   cargo run --bin execute-proposal -- <multisig_address> <proposal_index> [mainnet]
//!
//! Example:
//!   cargo run --bin execute-proposal -- BJbRt... 1 mainnet

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    transaction::Transaction,
};
use squads_multisig::anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use squads_multisig::pda::{get_proposal_pda, get_transaction_pda, get_vault_pda};
use squads_multisig::squads_multisig_program;
use squads_multisig::state::{Multisig, Proposal, ProposalStatus};
use squads_multisig_program::VaultTransaction;
use std::env;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        println!("Usage: cargo run --bin execute-proposal -- <multisig_address> <proposal_index> [mainnet]");
        println!();
        println!("Example:");
        println!("  cargo run --bin execute-proposal -- BJbRt... 1 mainnet");
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

    // Derive PDAs
    let (transaction_pda, _) = get_transaction_pda(&multisig_pda, proposal_index, None);
    let (proposal_pda, _) = get_proposal_pda(&multisig_pda, proposal_index, None);

    // Fetch multisig
    let multisig_account = client
        .get_account(&multisig_pda)
        .expect("Failed to fetch multisig account");
    let multisig = Multisig::try_deserialize(&mut multisig_account.data.as_slice())
        .expect("Failed to deserialize multisig");

    // Fetch proposal
    let proposal_account = client
        .get_account(&proposal_pda)
        .expect("Failed to fetch proposal account");
    let proposal = Proposal::try_deserialize(&mut proposal_account.data.as_slice())
        .expect("Failed to deserialize proposal");

    // Fetch vault transaction
    let transaction_account = client
        .get_account(&transaction_pda)
        .expect("Failed to fetch transaction account");
    let vault_transaction = VaultTransaction::try_deserialize(&mut transaction_account.data.as_slice())
        .expect("Failed to deserialize vault transaction");

    // Derive vault PDA
    let (vault_pda, _) = get_vault_pda(&multisig_pda, vault_transaction.vault_index, None);

    println!("=== Execute Proposal ({}) ===\n", network.to_uppercase());
    println!("Multisig: {}", multisig_pda);
    println!("Executor: {}", member.pubkey());
    println!();
    println!("Proposal Index: {}", proposal_index);
    println!("Proposal Address: {}", proposal_pda);
    println!("Transaction Address: {}", transaction_pda);
    println!("Vault: {}", vault_pda);

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
    println!("Approvals: {} of {} required", proposal.approved.len(), multisig.threshold);

    // Check if proposal is approved
    if !matches!(proposal.status, ProposalStatus::Approved { .. }) {
        println!("\nError: Proposal is not approved. Current status: {}", status_str);
        if matches!(proposal.status, ProposalStatus::Active { .. }) {
            let remaining = multisig.threshold as usize - proposal.approved.len();
            println!("  {} more approval(s) needed.", remaining);
        }
        return;
    }

    // Build remaining accounts from the transaction message
    let message = &vault_transaction.message;

    // The remaining accounts need to include:
    // 1. AddressLookupTable accounts (none for simple transactions)
    // 2. Static account keys from the message
    // 3. Loaded accounts from address table lookups (none for simple transactions)

    let mut remaining_accounts: Vec<AccountMeta> = Vec::new();

    // Add static accounts from the message
    for (index, pubkey) in message.account_keys.iter().enumerate() {
        let is_signer = message.is_signer_index(index);
        let is_writable = message.is_static_writable_index(index);

        // Vault PDA signs via CPI, so we don't mark it as signer here
        let actual_is_signer = is_signer && pubkey != &vault_pda;

        remaining_accounts.push(AccountMeta {
            pubkey: *pubkey,
            is_signer: actual_is_signer,
            is_writable,
        });
    }

    // Build the execute instruction
    let accounts = squads_multisig_program::accounts::VaultTransactionExecute {
        multisig: multisig_pda,
        proposal: proposal_pda,
        transaction: transaction_pda,
        member: member.pubkey(),
    };

    let mut account_metas = accounts.to_account_metas(Some(false));
    account_metas.extend(remaining_accounts);

    let instruction = Instruction {
        program_id: squads_multisig_program::ID,
        accounts: account_metas,
        data: squads_multisig_program::instruction::VaultTransactionExecute {}.data(),
    };

    println!("\nExecuting proposal...");

    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&member.pubkey()),
        &[&member],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&transaction) {
        Ok(sig) => {
            println!("\nProposal executed successfully!");
            println!("Transaction: {}", sig);

            let cluster_param = if network == "mainnet" { "" } else { "?cluster=devnet" };
            println!("\nView on Solana Explorer:");
            println!("https://explorer.solana.com/tx/{}{}", sig, cluster_param);
        }
        Err(e) => {
            println!("\nFailed to execute proposal: {}", e);
            println!("\nThis may happen if:");
            println!("  - The vault doesn't have enough funds");
            println!("  - The time lock hasn't passed (if set)");
            println!("  - The inner transaction failed");
        }
    }
}
