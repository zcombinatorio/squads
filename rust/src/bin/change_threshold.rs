use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    transaction::Transaction,
};
use squads_multisig::anchor_lang::InstructionData;
use squads_multisig::squads_multisig_program;
use std::env;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        println!("Usage: cargo run --bin change_threshold -- <multisig_address> <new_threshold> [mainnet]");
        println!("Example: cargo run --bin change_threshold -- BJbRtXM8wecvRrJNbbpNLfuG8FTSoU6zPYW1NFrMH6Q3 2 mainnet");
        return;
    }

    let multisig_pda: Pubkey = args[1].parse().expect("Invalid multisig address");
    let new_threshold: u16 = args[2].parse().expect("Invalid threshold (must be a number)");
    let network = args.get(3).map(|s| s.as_str()).unwrap_or("devnet");

    let rpc_url = match network {
        "mainnet" => MAINNET_RPC,
        _ => DEVNET_RPC,
    };

    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let config_authority = read_keypair_file("../member1.json").expect("Failed to read member1.json");

    println!("=== Change Multisig Threshold ({}) ===\n", network.to_uppercase());
    println!("Multisig: {}", multisig_pda);
    println!("Config Authority: {}", config_authority.pubkey());
    println!("New Threshold: {}", new_threshold);

    let instruction_data = squads_multisig_program::instruction::MultisigChangeThreshold {
        args: squads_multisig_program::MultisigChangeThresholdArgs {
            new_threshold,
            memo: None,
        },
    };

    let accounts = vec![
        AccountMeta::new(multisig_pda, false),
        AccountMeta::new_readonly(config_authority.pubkey(), true),
        AccountMeta::new_readonly(squads_multisig_program::ID, false), // rent_payer (None)
        AccountMeta::new_readonly(squads_multisig_program::ID, false), // system_program (None)
    ];

    let instruction = Instruction {
        program_id: squads_multisig_program::ID,
        accounts,
        data: instruction_data.data(),
    };

    println!("\nChanging threshold...");

    let recent_blockhash = client.get_latest_blockhash().expect("Failed to get blockhash");
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&config_authority.pubkey()),
        &[&config_authority],
        recent_blockhash,
    );

    match client.send_and_confirm_transaction(&transaction) {
        Ok(sig) => {
            println!("\nThreshold changed successfully!");
            println!("Transaction: {}", sig);

            let cluster_param = if network == "mainnet" { "" } else { "?cluster=devnet" };
            println!("\nView on Solana Explorer:");
            println!("https://explorer.solana.com/tx/{}{}", sig, cluster_param);
        }
        Err(e) => {
            println!("\nFailed to change threshold: {}", e);
        }
    }
}
