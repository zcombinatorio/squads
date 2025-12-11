//! Create a proposal to transfer mint authority to a new owner
//!
//! Usage:
//!   cargo run --bin transfer-mint-authority-proposal -- <multisig_address> <mint> <new_authority> [mainnet]
//!
//! Example:
//!   cargo run --bin transfer-mint-authority-proposal -- BJbRt... E7xkt... NewAuth... mainnet

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    system_program,
    transaction::Transaction,
};
use spl_token::instruction::{set_authority, AuthorityType};
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

    if args.len() < 4 {
        println!("Create a proposal to transfer mint authority to a new owner");
        println!();
        println!("Usage:");
        println!("  cargo run --bin transfer-mint-authority-proposal -- <multisig_address> <mint> <new_authority> [mainnet]");
        println!();
        println!("Arguments:");
        println!("  multisig_address  - The multisig PDA (current mint authority holder via vault)");
        println!("  mint              - The token mint address");
        println!("  new_authority     - The new mint authority address");
        println!();
        println!("Example:");
        println!("  cargo run --bin transfer-mint-authority-proposal -- BJbRt... E7xkt... NewAuth... mainnet");
        println!();
        println!("WARNING: This will permanently transfer mint authority away from the multisig!");
        return;
    }

    let multisig_pda: Pubkey = args[1].parse().expect("Invalid multisig address");
    let mint: Pubkey = args[2].parse().expect("Invalid mint address");
    let new_authority: Pubkey = args[3].parse().expect("Invalid new authority address");
    let network = args.get(4).map(|s| s.as_str()).unwrap_or("devnet");

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

    println!("=== Create Transfer Mint Authority Proposal ({}) ===\n", network.to_uppercase());
    println!("Multisig: {}", multisig_pda);
    println!("Vault (current mint authority): {}", vault_pda);
    println!("Creator: {}", creator.pubkey());
    println!("Threshold: {} of {}", multisig.threshold, multisig.members.len());
    println!();
    println!("Mint: {}", mint);
    println!("New Mint Authority: {}", new_authority);
    println!();
    println!("WARNING: This will permanently transfer mint authority away from the multisig!");
    println!();
    println!("Transaction Index: {}", new_transaction_index);

    // Create the set_authority instruction to transfer mint authority
    let set_auth_ix = set_authority(
        &spl_token::ID,
        &mint,                        // The mint account
        Some(&new_authority),         // New authority
        AuthorityType::MintTokens,    // Authority type: MintTokens
        &vault_pda,                   // Current authority (vault)
        &[],                          // No additional signers (vault signs via CPI)
    ).expect("Failed to create set_authority instruction");

    // Compile the transaction message
    let transaction_message = TransactionMessage::try_compile(&vault_pda, &[set_auth_ix], &[])
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

    println!("\nCreating transfer authority proposal...");

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
            println!("  cargo run --bin approve-proposal -- {} {} {}",
                     multisig_pda, new_transaction_index, if network == "mainnet" { "mainnet" } else { "" });
            println!();
            println!("After threshold is met, execute with:");
            println!("  cargo run --bin execute-proposal -- {} {} {}",
                     multisig_pda, new_transaction_index, if network == "mainnet" { "mainnet" } else { "" });

            let cluster_param = if network == "mainnet" { "" } else { "?cluster=devnet" };
            println!("\nView on Solana Explorer:");
            println!("https://explorer.solana.com/tx/{}{}", sig, cluster_param);
        }
        Err(e) => {
            println!("\nFailed to create proposal: {}", e);
        }
    }
}
