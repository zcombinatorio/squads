//! Create a vault transaction proposal for multisig members to approve
//!
//! This creates a transaction that requires threshold signatures to execute.
//! Other members can view and approve it using the approve-proposal script.
//!
//! Usage:
//!   cargo run --bin create-proposal -- <multisig_address> transfer <destination> <amount_lamports> [mainnet]
//!
//! Examples:
//!   # Transfer 0.1 SOL from vault to destination
//!   cargo run --bin create-proposal -- BJbRt... transfer DestPubkey... 100000000
//!
//!   # Transfer on mainnet
//!   cargo run --bin create-proposal -- BJbRt... transfer DestPubkey... 100000000 mainnet

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    system_instruction,
    system_program,
    transaction::Transaction,
};
use squads_multisig::anchor_lang::{AccountDeserialize, AnchorSerialize, InstructionData, ToAccountMetas};
use squads_multisig::pda::{get_proposal_pda, get_transaction_pda, get_vault_pda};
use squads_multisig::squads_multisig_program;
use squads_multisig::state::Multisig;
use squads_multisig::vault_transaction::VaultTransactionMessageExt;
use squads_multisig_program::TransactionMessage;
use std::env;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";

fn print_usage() {
    println!("Create a vault transaction proposal for multisig approval");
    println!();
    println!("Usage:");
    println!("  cargo run --bin create-proposal -- <multisig_address> <command> [args...] [mainnet]");
    println!();
    println!("Commands:");
    println!("  transfer <destination> <amount_lamports>");
    println!("      Transfer SOL from the vault to a destination address");
    println!();
    println!("Examples:");
    println!("  # Transfer 0.1 SOL (100,000,000 lamports)");
    println!("  cargo run --bin create-proposal -- BJbRt... transfer DestAddr... 100000000");
    println!();
    println!("  # Transfer on mainnet");
    println!("  cargo run --bin create-proposal -- BJbRt... transfer DestAddr... 100000000 mainnet");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        print_usage();
        return;
    }

    let multisig_pda: Pubkey = args[1].parse().expect("Invalid multisig address");
    let command = &args[2];

    // Parse command and build the instruction
    let (inner_instructions, network, description) = match command.as_str() {
        "transfer" => {
            if args.len() < 5 {
                println!("Error: transfer requires <destination> <amount_lamports>");
                print_usage();
                return;
            }
            let destination: Pubkey = args[3].parse().expect("Invalid destination address");
            let amount: u64 = args[4].parse().expect("Invalid amount");
            let network = args.get(5).map(|s| s.as_str()).unwrap_or("devnet");

            // We'll set the vault PDA as the "from" address later after we derive it
            (
                vec![("transfer", destination, amount)],
                network,
                format!("Transfer {} lamports to {}", amount, destination),
            )
        }
        _ => {
            println!("Error: Unknown command '{}'", command);
            print_usage();
            return;
        }
    };

    let rpc_url = match network {
        "mainnet" => MAINNET_RPC,
        _ => DEVNET_RPC,
    };

    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let creator = read_keypair_file("../member1.json").expect("Failed to read member1.json");

    // Fetch multisig to get current transaction index
    let multisig_account = client
        .get_account(&multisig_pda)
        .expect("Failed to fetch multisig account");
    let multisig = Multisig::try_deserialize(&mut multisig_account.data.as_slice())
        .expect("Failed to deserialize multisig");

    // The new transaction will be at index + 1
    let new_transaction_index = multisig.transaction_index + 1;
    let vault_index: u8 = 0;

    // Derive PDAs
    let (vault_pda, _) = get_vault_pda(&multisig_pda, vault_index, None);
    let (transaction_pda, _) = get_transaction_pda(&multisig_pda, new_transaction_index, None);
    let (proposal_pda, _) = get_proposal_pda(&multisig_pda, new_transaction_index, None);

    println!("=== Create Multisig Proposal ({}) ===\n", network.to_uppercase());
    println!("Multisig: {}", multisig_pda);
    println!("Vault: {}", vault_pda);
    println!("Creator: {}", creator.pubkey());
    println!("Threshold: {} of {}", multisig.threshold, multisig.members.len());
    println!();
    println!("Transaction Index: {}", new_transaction_index);
    println!("Transaction PDA: {}", transaction_pda);
    println!("Proposal PDA: {}", proposal_pda);
    println!();
    println!("Action: {}", description);

    // Build the inner instructions that will execute from the vault
    let instructions: Vec<Instruction> = inner_instructions
        .iter()
        .map(|(cmd, dest, amount)| match *cmd {
            "transfer" => system_instruction::transfer(&vault_pda, dest, *amount),
            _ => panic!("Unknown command"),
        })
        .collect();

    // Compile the transaction message
    let transaction_message = TransactionMessage::try_compile(&vault_pda, &instructions, &[])
        .expect("Failed to compile transaction message");

    let message_bytes = transaction_message
        .try_to_vec()
        .expect("Failed to serialize message");

    // === Instruction 1: Create Vault Transaction ===
    let vault_tx_accounts = squads_multisig_program::accounts::VaultTransactionCreate {
        multisig: multisig_pda,
        transaction: transaction_pda,
        creator: creator.pubkey(),
        rent_payer: creator.pubkey(),
        system_program: system_program::ID,
    };

    let vault_tx_data = squads_multisig_program::instruction::VaultTransactionCreate {
        args: squads_multisig_program::instructions::VaultTransactionCreateArgs {
            vault_index,
            ephemeral_signers: 0,
            transaction_message: message_bytes,
            memo: None,
        },
    };

    let create_vault_tx_ix = Instruction {
        program_id: squads_multisig_program::ID,
        accounts: vault_tx_accounts.to_account_metas(Some(false)),
        data: vault_tx_data.data(),
    };

    // === Instruction 2: Create Proposal ===
    let proposal_accounts = squads_multisig_program::accounts::ProposalCreate {
        multisig: multisig_pda,
        proposal: proposal_pda,
        creator: creator.pubkey(),
        rent_payer: creator.pubkey(),
        system_program: system_program::ID,
    };

    let proposal_data = squads_multisig_program::instruction::ProposalCreate {
        args: squads_multisig_program::instructions::ProposalCreateArgs {
            transaction_index: new_transaction_index,
            draft: false, // Active immediately so members can vote
        },
    };

    let create_proposal_ix = Instruction {
        program_id: squads_multisig_program::ID,
        accounts: proposal_accounts.to_account_metas(Some(false)),
        data: proposal_data.data(),
    };

    // === Instruction 3: Creator auto-approves ===
    let approve_accounts = squads_multisig_program::accounts::ProposalVote {
        multisig: multisig_pda,
        proposal: proposal_pda,
        member: creator.pubkey(),
    };

    let approve_data = squads_multisig_program::instruction::ProposalApprove {
        args: squads_multisig_program::instructions::ProposalVoteArgs { memo: None },
    };

    let approve_ix = Instruction {
        program_id: squads_multisig_program::ID,
        accounts: approve_accounts.to_account_metas(Some(false)),
        data: approve_data.data(),
    };

    println!("\nCreating proposal...");

    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &[create_vault_tx_ix, create_proposal_ix, approve_ix],
        Some(&creator.pubkey()),
        &[&creator],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&transaction) {
        Ok(sig) => {
            println!("\nProposal created successfully!");
            println!("Transaction: {}", sig);
            println!();
            println!("=== Proposal Details ===");
            println!("Proposal Index: {}", new_transaction_index);
            println!("Proposal Address: {}", proposal_pda);
            println!("Status: Active (awaiting {} more approval(s))", multisig.threshold - 1);
            println!();
            println!("Share this with other members to approve:");
            println!("  cargo run --bin approve-proposal -- {} {} [mainnet]",
                     multisig_pda, new_transaction_index);
            println!();
            println!("After threshold is met, execute with:");
            println!("  cargo run --bin execute-proposal -- {} {} [mainnet]",
                     multisig_pda, new_transaction_index);

            let cluster_param = if network == "mainnet" { "" } else { "?cluster=devnet" };
            println!("\nView on Solana Explorer:");
            println!("https://explorer.solana.com/tx/{}{}", sig, cluster_param);
            println!("\nView on Squads UI:");
            println!("https://v4.squads.so/squads/{}/tx/{}", multisig_pda, new_transaction_index);
        }
        Err(e) => {
            println!("\nFailed to create proposal: {}", e);
        }
    }
}
