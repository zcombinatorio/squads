//! Create a vault transaction proposal that transfers an SPL token from the vault
//!
//! Mirrors create-proposal.rs but for SPL tokens. Builds an idempotent destination
//! ATA creation (paid by the vault on execution) followed by a transfer_checked.
//!
//! Usage:
//!   cargo run --bin create-spl-proposal -- <multisig_address> <mint> <destination_wallet> <amount> [mainnet]
//!
//! Example:
//!   # Send 375 USDC (6 decimals -> 375_000_000) on mainnet
//!   cargo run --bin create-spl-proposal -- Da9hqC... EPjFWdd5... 5eFiSY... 375000000 mainnet

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    system_program,
    transaction::Transaction,
};
use spl_associated_token_account::{
    get_associated_token_address, instruction::create_associated_token_account_idempotent,
};
use spl_token::instruction::transfer_checked;
use spl_token::state::Mint;
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
        println!("Usage: cargo run --bin create-spl-proposal -- <multisig_address> <mint> <destination_wallet> <amount> [mainnet]");
        println!();
        println!("Arguments:");
        println!("  multisig_address    - The multisig PDA");
        println!("  mint                - SPL token mint address");
        println!("  destination_wallet  - Recipient wallet (ATA derived/created idempotently)");
        println!("  amount              - Amount in smallest units (e.g. 375 USDC = 375000000)");
        return;
    }

    let multisig_pda: Pubkey = args[1].parse().expect("Invalid multisig address");
    let mint: Pubkey = args[2].parse().expect("Invalid mint address");
    let destination_wallet: Pubkey = args[3].parse().expect("Invalid destination address");
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

    if multisig.is_member(creator.pubkey()).is_none() {
        eprintln!("Error: signer {} is not a member of multisig {}", creator.pubkey(), multisig_pda);
        std::process::exit(1);
    }

    // Fetch mint to read decimals (used by transfer_checked)
    let mint_account = client.get_account(&mint).expect("Failed to fetch mint account");
    let mint_state = Mint::unpack(&mint_account.data[..Mint::LEN]).expect("Invalid mint account");
    let decimals = mint_state.decimals;

    let new_transaction_index = multisig.transaction_index + 1;
    let vault_index: u8 = 0;

    let (vault_pda, _) = get_vault_pda(&multisig_pda, vault_index, None);
    let (transaction_pda, _) = get_transaction_pda(&multisig_pda, new_transaction_index, None);
    let (proposal_pda, _) = get_proposal_pda(&multisig_pda, new_transaction_index, None);

    let source_ata = get_associated_token_address(&vault_pda, &mint);
    let destination_ata = get_associated_token_address(&destination_wallet, &mint);

    // Verify the vault actually holds this token
    let source_balance = match client.get_token_account_balance(&source_ata) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error: vault has no token account for mint {}", mint);
            eprintln!("  Expected source ATA: {}", source_ata);
            eprintln!("  RPC error: {}", e);
            std::process::exit(1);
        }
    };
    let source_amount: u64 = source_balance.amount.parse().unwrap_or(0);
    if source_amount < amount {
        eprintln!("Error: vault token balance {} < requested {}", source_amount, amount);
        std::process::exit(1);
    }

    println!("=== Create SPL Transfer Proposal ({}) ===\n", network.to_uppercase());
    println!("Multisig: {}", multisig_pda);
    println!("Vault: {}", vault_pda);
    println!("Creator: {}", creator.pubkey());
    println!("Threshold: {} of {}", multisig.threshold, multisig.members.len());
    println!();
    println!("Mint: {} (decimals: {})", mint, decimals);
    println!("Source ATA: {} (balance: {})", source_ata, source_amount);
    println!("Destination wallet: {}", destination_wallet);
    println!("Destination ATA: {}", destination_ata);
    let human = amount as f64 / 10f64.powi(decimals as i32);
    println!("Amount: {} ({} tokens)", amount, human);
    println!();
    println!("Transaction Index: {}", new_transaction_index);
    println!("Note: ATA creation is idempotent; vault pays rent if missing.");

    // Build the inner instructions executed from the vault on execution
    let create_ata_ix = create_associated_token_account_idempotent(
        &vault_pda,
        &destination_wallet,
        &mint,
        &spl_token::ID,
    );

    let transfer_ix = transfer_checked(
        &spl_token::ID,
        &source_ata,
        &mint,
        &destination_ata,
        &vault_pda,
        &[],
        amount,
        decimals,
    )
    .expect("Failed to build transfer_checked");

    let transaction_message =
        TransactionMessage::try_compile(&vault_pda, &[create_ata_ix, transfer_ix], &[])
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
            let remaining = (multisig.threshold as usize).saturating_sub(1);
            println!("Status: Active (awaiting {} more approval(s))", remaining);
            println!();
            println!("Other members can approve with:");
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
            println!("\nView on Squads UI:");
            println!("https://v4.squads.so/squads/{}/tx/{}", multisig_pda, new_transaction_index);
        }
        Err(e) => {
            eprintln!("\nFailed to create proposal: {}", e);
            std::process::exit(1);
        }
    }
}
