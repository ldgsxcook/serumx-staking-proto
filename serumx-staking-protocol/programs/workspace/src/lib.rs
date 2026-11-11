use anchor_lang::prelude::*;
use anchor_lang::solana_program::hash::hashv;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("4eYm6iZyVWnUkMbRZPkH6RkLBBMPV8EKP9mK8jkBKqr3");

pub const SECONDS_PER_YEAR: u128 = 31_536_000;
pub const BPS_DENOMINATOR: u128 = 10_000;
pub const MAX_APR_BPS: u16 = 5_000;
pub const MAX_PROOF_LEN: usize = 32;
pub const LEAF_PREFIX: u8 = 0;
pub const NODE_PREFIX: u8 = 1;

#[program]
pub mod workspace {
    use super::*;

    // mint: Pubkey, SerumX SPL token mint used for eligibility and rewards, 64vQ6Km98vEZnz7a1MmgMjsaDYUL7RaLJCDmRiggBAGS
    // indexer: Pubkey, Balance indexer authority that publishes Merkle reward epochs, 9PJ8I...3555
    // apr_bps: u16, Target annual reward rate in basis points, 1000 = 10%
    pub fn initialize_config(
        ctx: Context<InitializeConfig>,
        mint: Pubkey,
        indexer: Pubkey,
        apr_bps: u16,
    ) -> Result<()> {
        require!(apr_bps > 0 && apr_bps <= MAX_APR_BPS, ErrorCode::InvalidApr);
        require!(indexer != Pubkey::default(), ErrorCode::InvalidParameter);
        require!(mint != Pubkey::default(), ErrorCode::InvalidMint);
        let now = Clock::get()?.unix_timestamp;
        let authority_key = ctx.accounts.authority.key();

        let config = &mut ctx.accounts.config;
        config.bump = ctx.bumps.config;
        config.creator = authority_key;
        config.authority = authority_key;
        config.pending_authority = Pubkey::default();
        config.indexer = indexer;
        config.mint = mint;
        config.vault = Pubkey::default();
        config.is_active = true;
        config.is_paused = false;
        config.version = 1;
        config.apr_bps = apr_bps;
        config.rewards_start_ts = now;
        config.last_epoch_end_ts = now;
        config.epoch_count = 0;
        config.cumulative_rewards = 0;
        config.total_funded = 0;
        config.total_claimed = 0;
        config.total_holders = 0;

        emit!(ProtocolInitialized {
            config: config.key(),
            authority: authority_key,
            indexer,
            mint,
            apr_bps,
            rewards_start_ts: now,
        });
        Ok(())
    }

    pub fn initialize_rewards_vault(ctx: Context<InitializeRewardsVault>) -> Result<()> {
        let vault_key = ctx.accounts.vault.key();
        let config = &mut ctx.accounts.config;
        require!(config.vault == Pubkey::default(), ErrorCode::AlreadyInitialized);
        config.vault = vault_key;

        emit!(RewardsVaultInitialized {
            config: config.key(),
            vault: vault_key,
            mint: config.mint,
        });
        Ok(())
    }

    pub fn fund_rewards_vault(ctx: Context<FundRewardsVault>, amount: u64) -> Result<()> {
        let funder_key = ctx.accounts.funder.key();
        let vault_balance = ctx.accounts.vault.amount;
        require!(amount > 0, ErrorCode::InvalidAmount);
        require!(
            ctx.accounts.funder_token.key() != ctx.accounts.vault.key(),
            ErrorCode::DuplicateAccount
        );
        require!(ctx.accounts.funder_token.amount >= amount, ErrorCode::InsufficientFunds);
        let new_total = ctx
            .accounts
            .config
            .total_funded
            .checked_add(amount)
            .ok_or(ErrorCode::MathOverflow)?;
        let balance_after = vault_balance.checked_add(amount).ok_or(ErrorCode::MathOverflow)?;

        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.funder_token.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                    authority: ctx.accounts.funder.to_account_info(),
                },
            ),
            amount,
        )?;

        let config = &mut ctx.accounts.config;
        config.total_funded = new_total;

        emit!(RewardsVaultFunded {
            config: config.key(),
            funder: funder_key,
            amount,
            vault_balance: balance_after,
            total_funded: new_total,
        });
        Ok(())
    }

    pub fn register_holder(ctx: Context<RegisterHolder>) -> Result<()> {
        let config_key = ctx.accounts.config.key();
        let holder_key = ctx.accounts.holder.key();
        let balance = ctx.accounts.holder_token.amount;
        require!(
            ctx.accounts.config.is_active && !ctx.accounts.config.is_paused,
            ErrorCode::ProtocolPaused
        );
        let now = Clock::get()?.unix_timestamp;
        let total_holders = ctx
            .accounts
            .config
            .total_holders
            .checked_add(1)
            .ok_or(ErrorCode::MathOverflow)?;

        let record = &mut ctx.accounts.holder_record;
        record.bump = ctx.bumps.holder_record;
        record.config = config_key;
        record.owner = holder_key;
        record.registered_at = now;
        record.balance_at_registration = balance;
        record.total_claimed = 0;
        record.last_claim_epoch = 0;
        record.last_claim_ts = 0;
        record.claim_count = 0;

        ctx.accounts.config.total_holders = total_holders;

        emit!(HolderRegistered {
            config: config_key,
            holder: holder_key,
            registered_at: now,
            balance_at_registration: balance,
        });
        Ok(())
    }

    pub fn publish_reward_epoch(
        ctx: Context<PublishRewardEpoch>,
        epoch_id: u64,
        end_ts: i64,
        total_eligible_balance: u64,
        epoch_rewards: u64,
        merkle_root: [u8; 32],
    ) -> Result<()> {
        let config = &ctx.accounts.config;
        let config_key = config.key();
        require!(config.is_active && !config.is_paused, ErrorCode::ProtocolPaused);
        let expected_id = config.epoch_count.checked_add(1).ok_or(ErrorCode::MathOverflow)?;
        require!(epoch_id == expected_id, ErrorCode::InvalidEpochId);
        require!(merkle_root != [0u8; 32], ErrorCode::InvalidMerkleRoot);

        let now = Clock::get()?.unix_timestamp;
        let start_ts = config.last_epoch_end_ts;
        require!(end_ts > start_ts && end_ts <= now, ErrorCode::InvalidEpochWindow);
        let duration = end_ts.checked_sub(start_ts).ok_or(ErrorCode::MathOverflow)? as u128;

        let max_rewards = (total_eligible_balance as u128)
            .checked_mul(config.apr_bps as u128)
            .ok_or(ErrorCode::MathOverflow)?
            .checked_mul(duration)
            .ok_or(ErrorCode::MathOverflow)?
            .checked_div(BPS_DENOMINATOR * SECONDS_PER_YEAR)
            .ok_or(ErrorCode::MathOverflow)?;
        require!((epoch_rewards as u128) <= max_rewards, ErrorCode::ExceedsAprCap);
        let cumulative = config
            .cumulative_rewards
            .checked_add(epoch_rewards)
            .ok_or(ErrorCode::MathOverflow)?;
        let apr_bps = config.apr_bps;

        let epoch = &mut ctx.accounts.epoch;
        epoch.bump = ctx.bumps.epoch;
        epoch.config = config_key;
        epoch.epoch_id = epoch_id;
        epoch.start_ts = start_ts;
        epoch.end_ts = end_ts;
        epoch.total_eligible_balance = total_eligible_balance;
        epoch.epoch_rewards = epoch_rewards;
        epoch.cumulative_rewards = cumulative;
        epoch.apr_bps = apr_bps;
        epoch.merkle_root = merkle_root;
        epoch.published_at = now;

        let config = &mut ctx.accounts.config;
        config.epoch_count = epoch_id;
        config.last_epoch_end_ts = end_ts;
        config.cumulative_rewards = cumulative;

        emit!(RewardEpochPublished {
            config: config_key,
            epoch_id,
            start_ts,
            end_ts,
            total_eligible_balance,
            epoch_rewards,
            cumulative_rewards: cumulative,
            merkle_root,
        });
        Ok(())
    }

    pub fn claim_rewards(
        ctx: Context<ClaimRewards>,
        epoch_id: u64,
        cumulative_amount: u64,
        proof: Vec<[u8; 32]>,
    ) -> Result<()> {
        let config = &ctx.accounts.config;
        let epoch = &ctx.accounts.epoch;
        let record = &ctx.accounts.holder_record;
        let config_key = config.key();
        let holder_key = ctx.accounts.holder.key();

        require!(
            ctx.accounts.vault.key() != ctx.accounts.holder_token.key(),
            ErrorCode::DuplicateAccount
        );
        require!(config.is_active && !config.is_paused, ErrorCode::ProtocolPaused);
        require!(record.registered_at < epoch.end_ts, ErrorCode::NotEligibleForEpoch);
        require!(proof.len() <= MAX_PROOF_LEN, ErrorCode::InvalidProof);
        require!(cumulative_amount <= epoch.cumulative_rewards, ErrorCode::InvalidProof);

        let leaf = hashv(&[
            &[LEAF_PREFIX],
            config_key.as_ref(),
            holder_key.as_ref(),
            &cumulative_amount.to_le_bytes(),
        ])
        .to_bytes();
        require!(verify_proof(&proof, epoch.merkle_root, leaf), ErrorCode::InvalidProof);

        let payout = cumulative_amount
            .checked_sub(record.total_claimed)
            .ok_or(ErrorCode::NothingToClaim)?;
        require!(payout > 0, ErrorCode::NothingToClaim);
        let new_config_claimed = config.total_claimed.checked_add(payout).ok_or(ErrorCode::MathOverflow)?;
        require!(new_config_claimed <= config.cumulative_rewards, ErrorCode::ExceedsPublishedRewards);
        require!(ctx.accounts.vault.amount >= payout, ErrorCode::InsufficientVaultBalance);
        let new_holder_claimed = record.total_claimed.checked_add(payout).ok_or(ErrorCode::MathOverflow)?;
        let new_claim_count = record.claim_count.checked_add(1).ok_or(ErrorCode::MathOverflow)?;

        let bump_arr = [ctx.bumps.vault];
        let vault_seeds: &[&[u8]] = &[b"vault", config_key.as_ref(), &bump_arr];
        let signer_seeds: &[&[&[u8]]] = &[vault_seeds];
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.vault.to_account_info(),
                    to: ctx.accounts.holder_token.to_account_info(),
                    authority: ctx.accounts.vault.to_account_info(),
                },
                signer_seeds,
            ),
            payout,
        )?;

        let now = Clock::get()?.unix_timestamp;
        let record = &mut ctx.accounts.holder_record;
        record.total_claimed = new_holder_claimed;
        record.last_claim_epoch = epoch_id;
        record.last_claim_ts = now;
        record.claim_count = new_claim_count;
        ctx.accounts.config.total_claimed = new_config_claimed;

        emit!(RewardsClaimed {
            config: config_key,
            holder: holder_key,
            epoch_id,
            amount: payout,
            holder_total_claimed: new_holder_claimed,
            claimed_at: now,
        });
        Ok(())
    }

    pub fn pause_protocol(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        require!(!config.is_paused, ErrorCode::AlreadyPaused);
        config.is_paused = true;
        emit!(ProtocolPauseChanged { config: config.key(), is_paused: true });
        Ok(())
    }

    pub fn resume_protocol(ctx: Context<AdminAction>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        require!(config.is_paused, ErrorCode::NotPaused);
        config.is_paused = false;
        emit!(ProtocolPauseChanged { config: config.key(), is_paused: false });
        Ok(())
    }

    pub fn update_protocol_config(
        ctx: Context<AdminAction>,
        indexer: Option<Pubkey>,
        apr_bps: Option<u16>,
    ) -> Result<()> {
        if let Some(new_indexer) = indexer {
            require!(new_indexer != Pubkey::default(), ErrorCode::InvalidParameter);
        }
        if let Some(new_apr) = apr_bps {
            require!(new_apr > 0 && new_apr <= MAX_APR_BPS, ErrorCode::InvalidApr);
        }

        let config = &mut ctx.accounts.config;
        if let Some(new_indexer) = indexer {
            config.indexer = new_indexer;
        }
        if let Some(new_apr) = apr_bps {
            config.apr_bps = new_apr;
        }

        emit!(ProtocolConfigUpdated {
            config: config.key(),
            indexer: config.indexer,
            apr_bps: config.apr_bps,
        });
        Ok(())
    }

    pub fn transfer_admin_authority(ctx: Context<AdminAction>, new_authority: Pubkey) -> Result<()> {
        require!(new_authority != Pubkey::default(), ErrorCode::InvalidParameter);
        let config = &mut ctx.accounts.config;
        config.pending_authority = new_authority;
        emit!(AdminTransferStarted {
            config: config.key(),
            current_authority: config.authority,
            pending_authority: new_authority,
        });
        Ok(())
    }

    pub fn accept_admin_authority(ctx: Context<AcceptAdminAuthority>) -> Result<()> {
        let new_key = ctx.accounts.new_authority.key();
        let config = &mut ctx.accounts.config;
        let previous = config.authority;
        config.authority = new_key;
        config.pending_authority = Pubkey::default();
        emit!(AdminTransferAccepted {
            config: config.key(),
            previous_authority: previous,
            new_authority: new_key,
        });
        Ok(())
    }
}

fn verify_proof(proof: &[[u8; 32]], root: [u8; 32], leaf: [u8; 32]) -> bool {
    let mut computed = leaf;
    for node in proof.iter() {
        computed = if computed <= *node {
            hashv(&[&[NODE_PREFIX], &computed, node]).to_bytes()
        } else {
            hashv(&[&[NODE_PREFIX], node, &computed]).to_bytes()
        };
    }
    computed == root
}

#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    #[account(
        init,
        seeds = [b"config", authority.key().as_ref()],
        bump,
        payer = authority,
        space = 8 + Config::LEN
    )]
    pub config: Account<'info, Config>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitializeRewardsVault<'info> {
    #[account(
        mut,
        seeds = [b"config", config.creator.as_ref()],
        bump = config.bump,
        has_one = authority @ ErrorCode::Unauthorized,
        has_one = mint @ ErrorCode::InvalidMint,
    )]
    pub config: Account<'info, Config>,

    #[account(
        init,
        seeds = [b"vault", config.key().as_ref()],
        bump,
        payer = authority,
        token::mint = mint,
        token::authority = vault,
    )]
    pub vault: Account<'info, TokenAccount>,

    pub mint: Account<'info, Mint>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct FundRewardsVault<'info> {
    #[account(
        mut,
        seeds = [b"config", config.creator.as_ref()],
        bump = config.bump,
        constraint = config.vault != Pubkey::default() @ ErrorCode::VaultNotInitialized,
    )]
    pub config: Account<'info, Config>,

    #[account(
        mut,
        seeds = [b"vault", config.key().as_ref()],
        bump,
    )]
    pub vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = funder_token.mint == config.mint @ ErrorCode::InvalidMint,
        constraint = funder_token.owner == funder.key() @ ErrorCode::InvalidOwner,
    )]
    pub funder_token: Account<'info, TokenAccount>,

    pub funder: Signer<'info>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct RegisterHolder<'info> {
    #[account(
        mut,
        seeds = [b"config", config.creator.as_ref()],
        bump = config.bump,
    )]
    pub config: Account<'info, Config>,

    #[account(
        init,
        seeds = [b"holder", config.key().as_ref(), holder.key().as_ref()],
        bump,
        payer = holder,
        space = 8 + HolderRecord::LEN
    )]
    pub holder_record: Account<'info, HolderRecord>,

    #[account(
        constraint = holder_token.mint == config.mint @ ErrorCode::InvalidMint,
        constraint = holder_token.owner == holder.key() @ ErrorCode::InvalidOwner,
    )]
    pub holder_token: Account<'info, TokenAccount>,

    #[account(mut)]
    pub holder: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(epoch_id: u64)]
pub struct PublishRewardEpoch<'info> {
    #[account(
        mut,
        seeds = [b"config", config.creator.as_ref()],
        bump = config.bump,
        constraint = config.indexer == publisher.key() @ ErrorCode::Unauthorized,
    )]
    pub config: Account<'info, Config>,

    #[account(
        init,
        seeds = [b"epoch", config.key().as_ref(), &epoch_id.to_le_bytes()],
        bump,
        payer = publisher,
        space = 8 + RewardEpoch::LEN
    )]
    pub epoch: Account<'info, RewardEpoch>,

    #[account(mut)]
    pub publisher: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(epoch_id: u64)]
pub struct ClaimRewards<'info> {
    #[account(
        mut,
        seeds = [b"config", config.creator.as_ref()],
        bump = config.bump,
    )]
    pub config: Account<'info, Config>,

    #[account(
        seeds = [b"epoch", config.key().as_ref(), &epoch_id.to_le_bytes()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, RewardEpoch>,

    #[account(
        mut,
        seeds = [b"holder", config.key().as_ref(), holder.key().as_ref()],
        bump = holder_record.bump,
        constraint = holder_record.owner == holder.key() @ ErrorCode::Unauthorized,
    )]
    pub holder_record: Account<'info, HolderRecord>,

    #[account(
        mut,
        seeds = [b"vault", config.key().as_ref()],
        bump,
    )]
    pub vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = holder_token.mint == config.mint @ ErrorCode::InvalidMint,
        constraint = holder_token.owner == holder.key() @ ErrorCode::InvalidOwner,
    )]
    pub holder_token: Account<'info, TokenAccount>,

    pub holder: Signer<'info>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct AdminAction<'info> {
    #[account(
        mut,
        seeds = [b"config", config.creator.as_ref()],
        bump = config.bump,
        has_one = authority @ ErrorCode::Unauthorized,
    )]
    pub config: Account<'info, Config>,

    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct AcceptAdminAuthority<'info> {
    #[account(
        mut,
        seeds = [b"config", config.creator.as_ref()],
        bump = config.bump,
        constraint = config.pending_authority == new_authority.key() @ ErrorCode::Unauthorized,
    )]
    pub config: Account<'info, Config>,

    pub new_authority: Signer<'info>,
}

#[account]
pub struct Config {
    pub bump: u8,
    pub creator: Pubkey,
    pub authority: Pubkey,
    pub pending_authority: Pubkey,
    pub indexer: Pubkey,
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub is_active: bool,
    pub is_paused: bool,
    pub version: u8,
    pub apr_bps: u16,
    pub rewards_start_ts: i64,
    pub last_epoch_end_ts: i64,
    pub epoch_count: u64,
    pub cumulative_rewards: u64,
    pub total_funded: u64,
    pub total_claimed: u64,
    pub total_holders: u64,
}

impl Config {
    pub const LEN: usize = 1 + 32 * 6 + 1 + 1 + 1 + 2 + 8 * 7;
}

#[account]
pub struct RewardEpoch {
    pub bump: u8,
    pub config: Pubkey,
    pub epoch_id: u64,
    pub start_ts: i64,
    pub end_ts: i64,
    pub total_eligible_balance: u64,
    pub epoch_rewards: u64,
    pub cumulative_rewards: u64,
    pub apr_bps: u16,
    pub merkle_root: [u8; 32],
    pub published_at: i64,
}

impl RewardEpoch {
    pub const LEN: usize = 1 + 32 + 8 * 6 + 2 + 32 + 8;
}

#[account]
pub struct HolderRecord {
    pub bump: u8,
    pub config: Pubkey,
    pub owner: Pubkey,
    pub registered_at: i64,
    pub balance_at_registration: u64,
    pub total_claimed: u64,
    pub last_claim_epoch: u64,
    pub last_claim_ts: i64,
    pub claim_count: u64,
}

impl HolderRecord {
    pub const LEN: usize = 1 + 32 + 32 + 8 * 6;
}

#[event]
pub struct ProtocolInitialized {
    pub config: Pubkey,
    pub authority: Pubkey,
    pub indexer: Pubkey,
    pub mint: Pubkey,
    pub apr_bps: u16,
    pub rewards_start_ts: i64,
}

#[event]
pub struct RewardsVaultInitialized {
    pub config: Pubkey,
    pub vault: Pubkey,
    pub mint: Pubkey,
}

#[event]
pub struct RewardsVaultFunded {
    pub config: Pubkey,
    pub funder: Pubkey,
    pub amount: u64,
    pub vault_balance: u64,
    pub total_funded: u64,
}

#[event]
pub struct HolderRegistered {
    pub config: Pubkey,
    pub holder: Pubkey,
    pub registered_at: i64,
    pub balance_at_registration: u64,
}

#[event]
pub struct RewardEpochPublished {
    pub config: Pubkey,
    pub epoch_id: u64,
    pub start_ts: i64,
    pub end_ts: i64,
    pub total_eligible_balance: u64,
    pub epoch_rewards: u64,
    pub cumulative_rewards: u64,
    pub merkle_root: [u8; 32],
}

#[event]
pub struct RewardsClaimed {
    pub config: Pubkey,
    pub holder: Pubkey,
    pub epoch_id: u64,
    pub amount: u64,
    pub holder_total_claimed: u64,
    pub claimed_at: i64,
}

#[event]
pub struct ProtocolPauseChanged {
    pub config: Pubkey,
    pub is_paused: bool,
}

#[event]
pub struct ProtocolConfigUpdated {
    pub config: Pubkey,
    pub indexer: Pubkey,
    pub apr_bps: u16,
}

#[event]
pub struct AdminTransferStarted {
    pub config: Pubkey,
    pub current_authority: Pubkey,
    pub pending_authority: Pubkey,
}

#[event]
pub struct AdminTransferAccepted {
    pub config: Pubkey,
    pub previous_authority: Pubkey,
    pub new_authority: Pubkey,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Math overflow occurred")]
    MathOverflow,
    #[msg("Insufficient funds")]
    InsufficientFunds,
    #[msg("Unauthorized access")]
    Unauthorized,
    #[msg("Account is inactive")]
    InactiveAccount,
    #[msg("Config is inactive")]
    ConfigInactive,
    #[msg("Invalid amount")]
    InvalidAmount,
    #[msg("Invalid parameter")]
    InvalidParameter,
    #[msg("Already initialized")]
    AlreadyInitialized,
    #[msg("APR must be between 1 and 5000 basis points")]
    InvalidApr,
    #[msg("Token mint does not match the protocol mint")]
    InvalidMint,
    #[msg("Token account owner mismatch")]
    InvalidOwner,
    #[msg("Protocol is paused")]
    ProtocolPaused,
    #[msg("Protocol is already paused")]
    AlreadyPaused,
    #[msg("Protocol is not paused")]
    NotPaused,
    #[msg("Rewards vault not initialized")]
    VaultNotInitialized,
    #[msg("Epoch id must be the next sequential epoch")]
    InvalidEpochId,
    #[msg("Epoch window is invalid")]
    InvalidEpochWindow,
    #[msg("Merkle root cannot be empty")]
    InvalidMerkleRoot,
    #[msg("Epoch rewards exceed the APR cap for the eligible balance")]
    ExceedsAprCap,
    #[msg("Invalid Merkle proof")]
    InvalidProof,
    #[msg("Nothing to claim")]
    NothingToClaim,
    #[msg("Claim exceeds published rewards")]
    ExceedsPublishedRewards,
    #[msg("Rewards vault has insufficient balance")]
    InsufficientVaultBalance,
    #[msg("Holder was not registered before this epoch ended")]
    NotEligibleForEpoch,
    #[msg("Duplicate account provided")]
    DuplicateAccount,
}
