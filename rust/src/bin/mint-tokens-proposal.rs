//! Create a proposal to mint tokens from a mint the multisig controls
//!
//! Usage:
//!   cargo run --bin mint-tokens-proposal -- <multisig_address> <mint> <destination_wallet> <amount> [mainnet]
//!
//! Example:
//!   # Mint 10,000 tokens (with 9 decimals = 10000 * 10^9 = 10_000_000_000_000)
//!   cargo run --bin mint-tokens-proposal -- BJbRt... E7xkt... DestWallet... 10000000000000 mainnet
//!
//! This script now derives the destination ATA from <destination_wallet> and adds an
//! idempotent ATA creation instruction before minting, so the ATA can be absent.

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    system_program,
    transaction::Transaction,
};
use spl_associated_token_account::{
    get_associated_token_address, instruction::create_associated_token_account_idempotent,
};
use spl_token::instruction::mint_to;
use squads_multisig::anchor_lang::{AccountDeserialize, AnchorSerialize, InstructionData, ToAccountMetas};
use squads_multisig::pda::{get_proposal_pda, get_transaction_pda, get_vault_pda};
use squads_multisig::squads_multisig_program;
use squads_multisig::state::Multisig;
use squads_multisig::vault_transaction::VaultTransactionMessageExt;
use squads_multisig_program::TransactionMessage;
use std::env;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 5 {
        println!("Create a proposal to mint tokens from a mint the multisig controls");
        println!();
        println!("Usage:");
        println!("  cargo run --bin mint-tokens-proposal -- <multisig_address> <mint> <destination_wallet> <amount> [mainnet]");
        println!();
        println!("Arguments:");
        println!("  multisig_address   - The multisig PDA");
        println!("  mint               - The token mint address");
        println!("  destination_wallet - Recipient wallet pubkey (ATA will be derived/created idempotently)");
        println!("  amount             - Amount in smallest units (e.g., for 9 decimals: 10000 tokens = 10000000000000)");
        println!();
        println!("Example:");
        println!("  cargo run --bin mint-tokens-proposal -- BJbRt... E7xkt... DestWallet... 10000000000000 mainnet");
        return;
    }

    let multisig_pda: Pubkey = args[1].parse().expect("Invalid multisig address");
    let mint: Pubkey = args[2].parse().expect("Invalid mint address");
    let destination_wallet: Pubkey = args[3].parse().expect("Invalid destination wallet address");
    let amount: u64 = args[4].parse().expect("Invalid amount");
    let network = args.get(5).map(|s| s.as_str()).unwrap_or("devnet");

    let rpc_url = match network {
        "mainnet" => MAINNET_RPC,
        _ => DEVNET_RPC,
    };

    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let creator = read_keypair_file("../member1.json").expect("Failed to read member1.json");

    // Fetch multisig
    let multisig_account = client
        .get_account(&multisig_pda)
        .expect("Failed to fetch multisig account");
    let multisig = Multisig::try_deserialize(&mut multisig_account.data.as_slice())
        .expect("Failed to deserialize multisig");

    let new_transaction_index = multisig.transaction_index + 1;
    let vault_index: u8 = 0;

    // Derive PDAs
    let (vault_pda, _) = get_vault_pda(&multisig_pda, vault_index, None);
    let (transaction_pda, _) = get_transaction_pda(&multisig_pda, new_transaction_index, None);
    let (proposal_pda, _) = get_proposal_pda(&multisig_pda, new_transaction_index, None);

    let destination_ata = get_associated_token_address(&destination_wallet, &mint);

    println!("=== Create Mint Tokens Proposal ({}) ===\n", network.to_uppercase());
    println!("Multisig: {}", multisig_pda);
    println!("Vault (mint authority / tx payer on execute): {}", vault_pda);
    println!("Creator: {}", creator.pubkey());
    println!("Threshold: {} of {}", multisig.threshold, multisig.members.len());
    println!();
    println!("Mint: {}", mint);
    println!("Destination Wallet: {}", destination_wallet);
    println!("Destination ATA: {}", destination_ata);
    println!("Amount: {} (smallest units)", amount);
    println!();
    println!("Transaction Index: {}", new_transaction_index);
    println!("Note: ATA creation is included and idempotent.");
    println!("Note: Vault must have enough SOL to pay ATA rent if missing.");

    // Create ATA idempotently (payer is the vault during proposal execution)
    let create_ata_ix = create_associated_token_account_idempotent(
        &vault_pda,
        &destination_wallet,
        &mint,
        &spl_token::ID,
    );

    // Create the mint_to instruction. The vault PDA is the mint authority and signs via Squads CPI.
    let mint_ix = mint_to(
        &spl_token::ID,
        &mint,
        &destination_ata,
        &vault_pda,
        &[],
        amount,
    )
    .expect("Failed to create mint_to instruction");

    // Compile the transaction message
    let transaction_message = TransactionMessage::try_compile(&vault_pda, &[create_ata_ix, mint_ix], &[])
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
            draft: false,
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

    println!("\nCreating mint proposal...");

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
            println!(
                "  cargo run --bin approve-proposal -- {} {} {}",
                multisig_pda,
                new_transaction_index,
                if network == "mainnet" { "mainnet" } else { "" }
            );
            println!();
            println!("After threshold is met, execute with:");
            println!(
                "  cargo run --bin execute-proposal -- {} {} {}",
                multisig_pda,
                new_transaction_index,
                if network == "mainnet" { "mainnet" } else { "" }
            );

            let cluster_param = if network == "mainnet" { "" } else { "?cluster=devnet" };
            println!("\nView on Solana Explorer:");
            println!("https://explorer.solana.com/tx/{}{}", sig, cluster_param);
        }
        Err(e) => {
            println!("\nFailed to create proposal: {}", e);
        }
    }
}
