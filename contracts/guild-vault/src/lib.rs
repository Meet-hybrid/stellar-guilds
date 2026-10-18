#![no_std]
// `initialize` has seven parameters by design: the Soroban SDK generates a
// client wrapper that adds `&self`, which trips clippy's 7-argument limit. The
// signature is part of the published interface, so the lint is silenced here
// rather than reshaping the API.
#![allow(clippy::too_many_arguments)]

//! # Stellar Guilds — guild vault
//!
//! A trustless rotating savings and credit association (ROSCA) on Soroban.
//!
//! Members join in a fixed order, contribute the same USDC amount every round,
//! and the whole pot rotates to one member per round in join order. Payout
//! order is therefore fixed by the member list and can never be changed.
//! A 0.25% fee is withheld from each payout and accrues to the treasury.
//!
//! ## Round lifecycle
//!
//! 1. `initialize` writes the configuration. The guild is empty.
//! 2. `join_guild` fills the seats. Joining is open until the first deposit.
//! 3. The first `deposit_contribution` opens round 0 at the current ledger and
//!    freezes the member list (the guild must be full first).
//! 4. Members contribute until `start_ledger + round_duration_ledgers`.
//! 5. Anyone may call `distribute_payout` once the deadline has passed. It
//!    settles immediately when every member paid, and settles pro-rata once the
//!    grace period has also expired. Unpaid members are flagged as defaulted.
//! 6. The next round opens at the settlement ledger.
//!
//! ## Trust assumptions
//!
//! * The contract is immutable: there is no upgrade entrypoint.
//! * The admin can never move principal; the only privileged path is
//!   `withdraw_treasury_fees`, which pays the configured treasury address.
//! * Settlement is permissionless; no keeper role exists.

mod types;

#[cfg(test)]
mod test;

pub use types::*;

use soroban_sdk::{contract, contractevent, contractimpl, token, Address, Env, Vec};

/// Emitted once, when the guild is configured.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Init {
    pub config: GuildConfig,
}

/// Emitted when a member takes a seat.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Join {
    #[topic]
    pub member: Address,
    pub index: u32,
}

/// Emitted on every accepted contribution.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deposit {
    #[topic]
    pub member: Address,
    pub round: u32,
    pub amount: i128,
    pub paid_count: u32,
}

/// Emitted when a round settles.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Payout {
    #[topic]
    pub recipient: Address,
    pub round: u32,
    pub pool: i128,
    pub fee: i128,
    pub net: i128,
}

/// Emitted when a member is flagged as having missed a round.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Defaulted {
    #[topic]
    pub member: Address,
    pub round: u32,
}

/// Emitted when accrued fees are paid out to the treasury.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeesWithdrawn {
    #[topic]
    pub treasury: Address,
    pub amount: i128,
}

/// Emitted after the final round settles.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Completed {
    pub rounds: u32,
}

#[contract]
pub struct GuildVaultContract;

#[contractimpl]
impl GuildVaultContract {
    /// Configure the guild. Callable once.
    ///
    /// * `contribution_amount` — stroops per member per round (USDC has 7 decimals).
    /// * `round_duration_ledgers` — length of the contribution window.
    /// * `grace_period_ledgers` — extra ledgers before pro-rata settlement.
    /// * `total_rounds` — number of seats, and therefore number of payouts.
    pub fn initialize(
        env: Env,
        admin: Address,
        treasury: Address,
        usdc_token: Address,
        contribution_amount: i128,
        round_duration_ledgers: u32,
        grace_period_ledgers: u32,
        total_rounds: u32,
    ) -> Result<(), Error> {
        admin.require_auth();

        if env.storage().instance().has(&DataKey::Config) {
            return Err(Error::AlreadyInitialized);
        }

        if contribution_amount < MIN_CONTRIBUTION
            || !(2..=MAX_MEMBERS).contains(&total_rounds)
            || !(MIN_ROUND_LEDGERS..=MAX_ROUND_LEDGERS).contains(&round_duration_ledgers)
            || grace_period_ledgers > MAX_GRACE_LEDGERS
            || admin == treasury
        {
            return Err(Error::InvalidParams);
        }

        // Rejects an issuer account (`G...`) passed where a token contract is
        // required: the host fails the invocation if the address is not a
        // contract implementing SEP-41 metadata.
        let _decimals = token::Client::new(&env, &usdc_token).decimals();

        let config = GuildConfig {
            admin,
            treasury,
            usdc_token,
            contribution_amount,
            round_duration_ledgers,
            grace_period_ledgers,
            total_rounds,
        };

        env.storage().instance().set(&DataKey::Config, &config);
        env.storage().instance().set(
            &DataKey::CurrentRound,
            &RoundState {
                index: 0,
                start_ledger: 0,
                paid_count: 0,
            },
        );
        env.storage().instance().set(&DataKey::VaultBalance, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::TreasuryBalance, &0i128);
        env.storage()
            .persistent()
            .set(&DataKey::MemberList, &Vec::<Address>::new(&env));
        bump_instance(&env);

        Init {
            config: config.clone(),
        }
        .publish(&env);
        Ok(())
    }

    /// Register a member. Joining is open only until the first contribution.
    pub fn join_guild(env: Env, member: Address) -> Result<(), Error> {
        member.require_auth();

        let config = load_config(&env)?;
        let round = load_round(&env)?;

        if round.index >= config.total_rounds {
            return Err(Error::Completed);
        }
        let mut members = load_members(&env);
        if members.len() >= config.total_rounds {
            return Err(Error::GuildFull);
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::MemberState(member.clone()))
        {
            return Err(Error::AlreadyMember);
        }

        let index = members.len();
        members.push_back(member.clone());
        store_members(&env, &members);

        let state = MemberState {
            address: member.clone(),
            has_paid: false,
            reputation_score: REPUTATION_START,
            is_defaulted: false,
        };
        store_member(&env, &member, &state);
        bump_instance(&env);

        Join { member, index }.publish(&env);
        Ok(())
    }

    /// Contribute the exact round amount. The first contribution opens round 0.
    pub fn deposit_contribution(env: Env, member: Address) -> Result<(), Error> {
        member.require_auth();

        let config = load_config(&env)?;
        let mut round = load_round(&env)?;

        if round.index >= config.total_rounds {
            return Err(Error::Completed);
        }

        let mut state = load_member(&env, &member)?;
        if state.has_paid {
            return Err(Error::AlreadyPaid);
        }

        let now = env.ledger().sequence();

        if round.start_ledger == 0 {
            // Opening the guild: every seat must be filled so the payout
            // rotation covers exactly `total_rounds` distinct members.
            if load_members(&env).len() < config.total_rounds {
                return Err(Error::GuildNotFull);
            }
            round.start_ledger = now;
        } else {
            let deadline = round.start_ledger + config.round_duration_ledgers;
            if now > deadline {
                return Err(Error::DepositWindowClosed);
            }
        }

        let contract = env.current_contract_address();
        token::Client::new(&env, &config.usdc_token).transfer(
            &member,
            &contract,
            &config.contribution_amount,
        );

        state.has_paid = true;
        state.reputation_score = state
            .reputation_score
            .saturating_add(REPUTATION_PER_DEPOSIT)
            .min(REPUTATION_MAX);
        store_member(&env, &member, &state);

        let mut vault: i128 = env
            .storage()
            .instance()
            .get(&DataKey::VaultBalance)
            .unwrap_or(0);
        vault = vault
            .checked_add(config.contribution_amount)
            .ok_or(Error::MathOverflow)?;
        env.storage().instance().set(&DataKey::VaultBalance, &vault);

        round.paid_count += 1;
        env.storage().instance().set(&DataKey::CurrentRound, &round);
        bump_instance(&env);

        Deposit {
            member,
            round: round.index,
            amount: config.contribution_amount,
            paid_count: round.paid_count,
        }
        .publish(&env);
        Ok(())
    }

    /// Settle the round and rotate the pot. Permissionless.
    ///
    /// Pays the recipient `pool - fee` and accrues `fee` to the treasury. When
    /// every member has paid, the pool is the full pot. Otherwise the round may
    /// only settle after the grace period, with the pool being exactly what was
    /// collected; members who did not pay are flagged as defaulted.
    pub fn distribute_payout(env: Env) -> Result<(), Error> {
        let config = load_config(&env)?;
        let mut round = load_round(&env)?;

        if round.index >= config.total_rounds {
            return Err(Error::Completed);
        }
        if round.start_ledger == 0 {
            return Err(Error::NotActive);
        }

        let now = env.ledger().sequence();
        let deadline = round.start_ledger + config.round_duration_ledgers;
        if now < deadline {
            return Err(Error::RoundNotReady);
        }

        let grace_expires = deadline + config.grace_period_ledgers;
        let all_paid = round.paid_count == config.total_rounds;
        if !all_paid && now <= grace_expires {
            return Err(Error::NotAllPaid);
        }

        let members = load_members(&env);
        let paid = round.paid_count as i128;
        let pool = config
            .contribution_amount
            .checked_mul(paid)
            .ok_or(Error::MathOverflow)?;
        let fee = pool.checked_mul(FEE_BPS).ok_or(Error::MathOverflow)? / BPS_DENOMINATOR;
        let net = pool - fee;

        let recipient = members.get(round.index).ok_or(Error::NotMember)?;
        let contract = env.current_contract_address();
        let token_client = token::Client::new(&env, &config.usdc_token);
        token_client.transfer(&contract, &recipient, &net);

        let mut treasury: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TreasuryBalance)
            .unwrap_or(0);
        treasury = treasury.checked_add(fee).ok_or(Error::MathOverflow)?;
        env.storage()
            .instance()
            .set(&DataKey::TreasuryBalance, &treasury);

        let mut vault: i128 = env
            .storage()
            .instance()
            .get(&DataKey::VaultBalance)
            .unwrap_or(0);
        vault = vault.checked_sub(pool).ok_or(Error::MathOverflow)?;
        env.storage().instance().set(&DataKey::VaultBalance, &vault);

        // Reset the round flags and, when settling pro-rata, record who failed
        // to contribute. Bounded by `MAX_MEMBERS`.
        for address in members.iter() {
            let mut member_state = load_member(&env, &address)?;
            if !member_state.has_paid && now > grace_expires {
                member_state.is_defaulted = true;
                member_state.reputation_score = 0;
            }
            member_state.has_paid = false;
            store_member(&env, &address, &member_state);
        }

        let settled_round = round.index;
        round.paid_count = 0;
        round.index += 1;
        round.start_ledger = if round.index < config.total_rounds {
            now
        } else {
            0
        };
        env.storage().instance().set(&DataKey::CurrentRound, &round);
        bump_instance(&env);

        Payout {
            recipient,
            round: settled_round,
            pool,
            fee,
            net,
        }
        .publish(&env);
        if round.index == config.total_rounds {
            Completed {
                rounds: config.total_rounds,
            }
            .publish(&env);
        }
        Ok(())
    }

    /// Flag a member who missed the round. Permissionless, and only meaningful
    /// once the grace period has expired. Settlement records defaults on its
    /// own; this entrypoint exists so the slash can be observed earlier.
    pub fn handle_default(env: Env, delinquent_member: Address) -> Result<(), Error> {
        let config = load_config(&env)?;
        let round = load_round(&env)?;

        if round.index >= config.total_rounds {
            return Err(Error::Completed);
        }
        if round.start_ledger == 0 {
            return Err(Error::NotActive);
        }

        let mut state = load_member(&env, &delinquent_member)?;
        if state.has_paid {
            return Err(Error::MemberHasPaid);
        }
        if state.is_defaulted {
            return Err(Error::AlreadyDefaulted);
        }

        let deadline = round.start_ledger + config.round_duration_ledgers;
        if env.ledger().sequence() <= deadline + config.grace_period_ledgers {
            return Err(Error::GraceNotExpired);
        }

        state.is_defaulted = true;
        state.reputation_score = 0;
        store_member(&env, &delinquent_member, &state);
        bump_instance(&env);

        Defaulted {
            member: delinquent_member,
            round: round.index,
        }
        .publish(&env);
        Ok(())
    }

    /// Pay every accrued protocol fee to the treasury. Treasury auth required.
    pub fn withdraw_treasury_fees(env: Env) -> Result<(), Error> {
        let config = load_config(&env)?;
        config.treasury.require_auth();

        let treasury_balance: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TreasuryBalance)
            .unwrap_or(0);
        if treasury_balance <= 0 {
            return Err(Error::NoFeesToWithdraw);
        }

        env.storage()
            .instance()
            .set(&DataKey::TreasuryBalance, &0i128);
        let contract = env.current_contract_address();
        token::Client::new(&env, &config.usdc_token).transfer(
            &contract,
            &config.treasury,
            &treasury_balance,
        );
        bump_instance(&env);

        FeesWithdrawn {
            treasury: config.treasury.clone(),
            amount: treasury_balance,
        }
        .publish(&env);
        Ok(())
    }

    /// Guild configuration.
    pub fn get_config(env: Env) -> Result<GuildConfig, Error> {
        load_config(&env)
    }

    /// Round cursor.
    pub fn get_round(env: Env) -> Result<RoundState, Error> {
        load_round(&env)
    }

    /// Member state, if the address has joined.
    pub fn get_member(env: Env, who: Address) -> Option<MemberState> {
        env.storage().persistent().get(&DataKey::MemberState(who))
    }

    /// Member list in join order, which is also payout order.
    pub fn get_members(env: Env) -> Vec<Address> {
        load_members(&env)
    }

    /// USDC collected for the round in progress.
    pub fn get_vault_balance(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::VaultBalance)
            .unwrap_or(0)
    }

    /// USDC owed to the treasury.
    pub fn get_treasury_balance(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TreasuryBalance)
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(STORAGE_THRESHOLD, STORAGE_EXTEND_TO);
}

fn load_config(env: &Env) -> Result<GuildConfig, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Config)
        .ok_or(Error::NotInitialized)
}

fn load_round(env: &Env) -> Result<RoundState, Error> {
    env.storage()
        .instance()
        .get(&DataKey::CurrentRound)
        .ok_or(Error::NotInitialized)
}

fn load_members(env: &Env) -> Vec<Address> {
    let key = DataKey::MemberList;
    let members: Vec<Address> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, STORAGE_THRESHOLD, STORAGE_EXTEND_TO);
    }
    members
}

fn store_members(env: &Env, members: &Vec<Address>) {
    let key = DataKey::MemberList;
    env.storage().persistent().set(&key, members);
    env.storage()
        .persistent()
        .extend_ttl(&key, STORAGE_THRESHOLD, STORAGE_EXTEND_TO);
}

fn load_member(env: &Env, who: &Address) -> Result<MemberState, Error> {
    let key = DataKey::MemberState(who.clone());
    let state: MemberState = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(Error::NotMember)?;
    env.storage()
        .persistent()
        .extend_ttl(&key, STORAGE_THRESHOLD, STORAGE_EXTEND_TO);
    Ok(state)
}

fn store_member(env: &Env, who: &Address, state: &MemberState) {
    let key = DataKey::MemberState(who.clone());
    env.storage().persistent().set(&key, state);
    env.storage()
        .persistent()
        .extend_ttl(&key, STORAGE_THRESHOLD, STORAGE_EXTEND_TO);
}
