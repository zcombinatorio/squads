use solana_client::rpc_client::RpcClient;
use solana_sdk::address_lookup_table::state::AddressLookupTable;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use squads_multisig::anchor_lang::AccountDeserialize;
use squads_multisig::pda::{get_transaction_pda, get_vault_pda};
use squads_multisig::squads_multisig_program::state::VaultTransaction;
use std::env;
use std::str::FromStr;

const DEVNET_RPC: &str = "https://api.devnet.solana.com";

/// Mainnet goes through Helius (the public endpoint rate-limits the account scans
/// these tools do). The key is read from the environment so it stays out of source:
///   export HELIUS_API_KEY=...        # or
///   export MAINNET_RPC_URL=https://... # full override
fn mainnet_rpc() -> String {
    if let Ok(url) = env::var("MAINNET_RPC_URL") {
        if !url.is_empty() {
            return url;
        }
    }
    match env::var("HELIUS_API_KEY") {
        Ok(k) if !k.is_empty() => format!("https://mainnet.helius-rpc.com/?api-key={}", k),
        _ => {
            eprintln!("Error: set HELIUS_API_KEY (or MAINNET_RPC_URL) for mainnet access.");
            std::process::exit(1);
        }
    }
}

fn pk(s: &str) -> Pubkey { Pubkey::from_str(s).unwrap() }

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        println!("Usage: cargo run --bin inspect-vault-transaction -- <multisig> <index> [devnet]");
        return;
    }
    let multisig: Pubkey = args[1].parse().expect("invalid multisig");
    let index: u64 = args[2].parse().expect("invalid index");
    let network = args.get(3).map(|s| s.as_str()).unwrap_or("mainnet");
    let rpc_url = match network { "devnet" => DEVNET_RPC.to_string(), _ => mainnet_rpc() };
    let client = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());

    let (tx_pda, _) = get_transaction_pda(&multisig, index, None);
    let acct = client.get_account(&tx_pda).expect("vault tx fetch failed");
    let vt = VaultTransaction::try_deserialize(&mut acct.data.as_slice())
        .expect("vault tx deserialize failed");

    let (vault, _) = get_vault_pda(&multisig, vt.vault_index, None);

    println!("=== Vault Transaction #{} ({}) ===", index, network.to_uppercase());
    println!("PDA:          {}", tx_pda);
    println!("Multisig:     {}", multisig);
    println!("Creator:      {}", vt.creator);
    println!("Vault index:  {} -> {}", vt.vault_index, vault);
    println!("Eph signers:  {}", vt.ephemeral_signer_bumps.len());
    println!();

    let m = &vt.message;
    println!("Static account keys ({}):", m.account_keys.len());
    for (i, k) in m.account_keys.iter().enumerate() {
        let role = if i < m.num_writable_signers as usize { "signer/writable" }
            else if i < m.num_signers as usize { "signer/readonly" }
            else if i < (m.num_signers as usize + m.num_writable_non_signers as usize) { "writable" }
            else { "readonly" };
        println!("  [{}] {} ({})", i, k, role);
    }

    let mut all_keys: Vec<Pubkey> = m.account_keys.clone();
    if !m.address_table_lookups.is_empty() {
        println!("\nAddress table lookups ({}):", m.address_table_lookups.len());
        for lut_ref in &m.address_table_lookups {
            println!("  ATL {}", lut_ref.account_key);
            let lut_acct = match client.get_account(&lut_ref.account_key) {
                Ok(a) => a,
                Err(e) => { println!("    fetch failed: {}", e); continue; }
            };
            let lut = match AddressLookupTable::deserialize(&lut_acct.data) {
                Ok(l) => l,
                Err(e) => { println!("    deserialize failed: {}", e); continue; }
            };
            for &i in &lut_ref.writable_indexes {
                let pk = lut.addresses[i as usize];
                println!("    writable [{}] = {}", i, pk);
                all_keys.push(pk);
            }
            for &i in &lut_ref.readonly_indexes {
                let pk = lut.addresses[i as usize];
                println!("    readonly [{}] = {}", i, pk);
                all_keys.push(pk);
            }
        }
    }

    let sys_program = pk("11111111111111111111111111111111");
    let token_program = pk("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
    let ata_program = pk("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

    println!("\nInstructions ({}):", m.instructions.len());
    for (idx, ix) in m.instructions.iter().enumerate() {
        let prog = all_keys.get(ix.program_id_index as usize).copied().unwrap_or_default();
        println!("\n  Ix #{} program: {}", idx, prog);
        let accts: Vec<Pubkey> = ix.account_indexes.iter()
            .map(|&i| all_keys.get(i as usize).copied().unwrap_or_default())
            .collect();
        for (j, a) in accts.iter().enumerate() {
            println!("    a[{}] = {}", j, a);
        }
        let data_hex: String = ix.data.iter().map(|b| format!("{:02x}", b)).collect();
        println!("    data ({} bytes): {}", ix.data.len(), data_hex);

        if prog == sys_program && ix.data.len() == 12 {
            let disc = u32::from_le_bytes(ix.data[0..4].try_into().unwrap());
            if disc == 2 {
                let lamports = u64::from_le_bytes(ix.data[4..12].try_into().unwrap());
                println!("    -> System Transfer: {} lamports ({} SOL) from {} to {}",
                    lamports, lamports as f64 / 1e9, accts.first().copied().unwrap_or_default(), accts.get(1).copied().unwrap_or_default());
            }
        } else if prog == token_program {
            if ix.data.first() == Some(&3) && ix.data.len() == 9 {
                let amount = u64::from_le_bytes(ix.data[1..9].try_into().unwrap());
                println!("    -> SPL Token Transfer: amount={} (raw)  src={}  dst={}  auth={}",
                    amount, accts.first().copied().unwrap_or_default(), accts.get(1).copied().unwrap_or_default(), accts.get(2).copied().unwrap_or_default());
            } else if ix.data.first() == Some(&12) && ix.data.len() == 10 {
                let amount = u64::from_le_bytes(ix.data[1..9].try_into().unwrap());
                let decimals = ix.data[9];
                let human = amount as f64 / 10f64.powi(decimals as i32);
                println!("    -> SPL Token TransferChecked: amount={} ({} @ {} decimals)  src={}  mint={}  dst={}  auth={}",
                    amount, human, decimals,
                    accts.first().copied().unwrap_or_default(),
                    accts.get(1).copied().unwrap_or_default(),
                    accts.get(2).copied().unwrap_or_default(),
                    accts.get(3).copied().unwrap_or_default());
            } else if ix.data.first() == Some(&7) {
                println!("    -> SPL Token MintTo");
            }
        } else if prog == ata_program {
            if ix.data.first() == Some(&1) {
                println!("    -> ATA CreateIdempotent  payer={}  ata={}  owner={}  mint={}",
                    accts.first().copied().unwrap_or_default(),
                    accts.get(1).copied().unwrap_or_default(),
                    accts.get(2).copied().unwrap_or_default(),
                    accts.get(3).copied().unwrap_or_default());
            } else if ix.data.is_empty() {
                println!("    -> ATA Create");
            }
        }
    }
}
