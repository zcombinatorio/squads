//! Close a stale Squads vault transaction and its proposal.
//!
//! If the multisig has no rent collector, this temporarily sets the config
//! authority as rent collector, closes the accounts, then restores None.

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    system_program,
    transaction::Transaction,
};
use squads_multisig::anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use squads_multisig::pda::{get_proposal_pda, get_transaction_pda};
use squads_multisig::squads_multisig_program;
use squads_multisig::state::Multisig;
use std::env;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";

fn send_ix(client: &RpcClient, payer: &solana_sdk::signature::Keypair, ix: Instruction) -> String {
    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get blockhash");
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer],
        recent_blockhash,
    );

    client
        .send_and_confirm_transaction(&tx)
        .expect("Failed to send transaction")
        .to_string()
}

fn set_rent_collector_ix(
    multisig_pda: Pubkey,
    authority: Pubkey,
    rent_collector: Option<Pubkey>,
) -> Instruction {
    let data = squads_multisig_program::instruction::MultisigSetRentCollector {
        args: squads_multisig_program::MultisigSetRentCollectorArgs {
            rent_collector,
            memo: None,
        },
    };

    let accounts = vec![
        AccountMeta::new(multisig_pda, false),
        AccountMeta::new_readonly(authority, true),
        AccountMeta::new(authority, true),
        AccountMeta::new_readonly(system_program::ID, false),
    ];

    Instruction {
        program_id: squads_multisig_program::ID,
        accounts,
        data: data.data(),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        println!("Usage: cargo run --bin close-stale-vault-transaction -- <multisig_address> <proposal_index> [mainnet]");
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
    let authority = read_keypair_file("../member1.json").expect("Failed to read member1.json");

    let multisig_account = client
        .get_account(&multisig_pda)
        .expect("Failed to fetch multisig account");
    let multisig = Multisig::try_deserialize(&mut multisig_account.data.as_slice())
        .expect("Failed to deserialize multisig");
    let original_rent_collector = multisig.rent_collector;
    let close_rent_collector = original_rent_collector.unwrap_or_else(|| authority.pubkey());

    println!("=== Close Stale Vault Transaction ({}) ===\n", network.to_uppercase());
    println!("Multisig: {}", multisig_pda);
    println!("Proposal Index: {}", proposal_index);
    println!("Fee Payer: {}", authority.pubkey());

    if original_rent_collector.is_none() {
        println!("\nTemporarily setting rent collector: {}", close_rent_collector);
        let sig = send_ix(
            &client,
            &authority,
            set_rent_collector_ix(multisig_pda, authority.pubkey(), Some(close_rent_collector)),
        );
        println!("Set rent collector transaction: {}", sig);
    } else {
        println!("Rent collector: {}", close_rent_collector);
    }

    let (proposal_pda, _) = get_proposal_pda(&multisig_pda, proposal_index, None);
    let (transaction_pda, _) = get_transaction_pda(&multisig_pda, proposal_index, None);

    let accounts = squads_multisig_program::accounts::VaultTransactionAccountsClose {
        multisig: multisig_pda,
        proposal: proposal_pda,
        transaction: transaction_pda,
        rent_collector: close_rent_collector,
        system_program: system_program::ID,
    };

    let close_ix = Instruction {
        program_id: squads_multisig_program::ID,
        accounts: accounts.to_account_metas(Some(false)),
        data: squads_multisig_program::instruction::VaultTransactionAccountsClose {}.data(),
    };

    println!("\nClosing proposal {} and transaction {}...", proposal_pda, transaction_pda);
    let close_sig = send_ix(&client, &authority, close_ix);
    println!("Close transaction: {}", close_sig);

    if original_rent_collector.is_none() {
        println!("\nRestoring rent collector: None");
        let sig = send_ix(
            &client,
            &authority,
            set_rent_collector_ix(multisig_pda, authority.pubkey(), None),
        );
        println!("Restore rent collector transaction: {}", sig);
    }

    println!("\nClosed stale vault transaction successfully.");
}
