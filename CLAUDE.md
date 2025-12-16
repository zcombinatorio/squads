# Squads Multisig - Rust SDK

Create a 3/5 multisig on Solana using Squads Protocol v4.

## Quick Start

### 1. Generate a secure keypair

```bash
# Generate new keypair (keep this safe!)
solana-keygen new -o member1.json

# Or if you have an existing keypair, copy it here
cp ~/.config/solana/id.json member1.json
```

The `member1.json` file must be a 64-byte array (Solana keypair format).

### 2. Fund the keypair

```bash
# Check public key
solana-keygen pubkey member1.json

# Fund with ~0.01 SOL for transaction fees
# On devnet: solana airdrop 1 <pubkey> --url devnet
# On mainnet: send SOL to the pubkey from an exchange/wallet
```

### 3. Configure members

Edit `rust/src/main.rs` and update the member addresses:

```rust
// Lines 36-39 - Replace with your team's wallet addresses
const MEMBER2: &str = "your-wallet-address-2";
const MEMBER3: &str = "your-wallet-address-3";
const MEMBER4: &str = "your-wallet-address-4";
const MEMBER5: &str = "your-wallet-address-5";

// Line 42 - Adjust threshold if needed
const THRESHOLD: u16 = 3;
```

### 4. Deploy

```bash
cd rust

# Test on devnet first
cargo run

# Deploy to mainnet
cargo run -- mainnet
```

## Commands

```bash
# Create multisig (from rust/ directory)
cargo run                    # Devnet (default)
cargo run -- mainnet         # Mainnet

# Change threshold (config authority only)
cargo run --bin change_threshold -- <multisig_address> <new_threshold> [mainnet]

# Add member (config authority only)
cargo run --bin add-member -- <multisig_address> <new_member_address> [mainnet]

# Remove member (config authority only)
cargo run --bin remove-member -- <multisig_address> <member_to_remove> [mainnet]

# Add spending limit (config authority only)
cargo run --bin add-spending-limit -- <multisig_address> <amount> <period> [options] [mainnet]
#   period: one-time, day, week, month
#   options: --mint <addr>, --vault <idx>, --members <addr1,addr2>, --destinations <addr1,addr2>

# Remove spending limit (config authority only)
cargo run --bin remove-spending-limit -- <multisig_address> <spending_limit_address> [mainnet]

# Inspect a specific spending limit
cargo run --bin inspect-spending-limit -- <spending_limit_address> [mainnet]

# List all spending limits for a multisig (requires dedicated RPC for mainnet)
cargo run --bin inspect-spending-limit -- --multisig <multisig_address> [mainnet]

# Use spending limit to transfer (authorized members only, no proposal needed!)
cargo run --bin use-spending-limit -- <spending_limit_address> <destination> <amount> [mainnet]

# Inspect existing multisig
cargo run --bin inspect_multisig -- <multisig_address> [mainnet]

# Create a proposal (requires threshold approval)
cargo run --bin create-proposal -- <multisig_address> transfer <destination> <amount_lamports> [mainnet]

# Approve a proposal (any member)
cargo run --bin approve-proposal -- <multisig_address> <proposal_index> [mainnet]

# Execute a proposal (after threshold met)
cargo run --bin execute-proposal -- <multisig_address> <proposal_index> [mainnet]
```

## What Gets Created

- **Multisig PDA**: The multisig account address
- **Vault PDA**: Where funds are stored (send funds here!)
- **Config Authority**: Member1 can modify settings without proposals

## Config Authority

The config authority (member1) can instantly modify:
- Add/remove members
- Change threshold
- Set time lock
- Add/remove spending limits
- Transfer config authority

**Important**: Config authority CANNOT move funds without threshold approval (unless using spending limits).

Set `config_authority: None` in main.rs to make multisig fully autonomous.

## Spending Limits (Bypass Proposals)

Spending limits allow designated members to transfer funds **without creating proposals**:

1. **Config authority creates a spending limit** with amount, period, authorized members, and optional destination whitelist
2. **Authorized members can transfer directly** using `use-spending-limit` - no proposal/approval needed
3. **Limits reset automatically** based on period (daily, weekly, monthly, or one-time)

Example workflow:
```bash
# Config authority sets up a 1 SOL daily limit for member2
cargo run --bin add-spending-limit -- <multisig> 1000000000 day --members <member2>

# Member2 can now transfer up to 1 SOL/day without proposals
cargo run --bin use-spending-limit -- <spending_limit_addr> <destination> 500000000
```

## Costs

- **Creation**: ~0.003 SOL (rent + fees)
- **Threshold change**: ~0.00001 SOL (fee only)

## Treasury Addresses (verified on-chain)

- **Devnet**: `HM5y4mz3Bt9JY9mr1hkyhnvqxSH4H2u2451j7Hc2dtvK`
- **Mainnet**: `5DH2e3cJmFpyi6mk65EGFediunm4ui6BiKNUNrhWtD1b`

## Rust SDK

- Package: `squads-multisig` v2.1.0
- Compatible with: `solana-sdk` and `solana-client` v1.18
