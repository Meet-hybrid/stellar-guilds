#![no_std]

//! # Stellar Guilds — guild vault
//!
//! A trustless rotating savings and credit association (ROSCA) on Soroban.
//!
//! Members contribute the same USDC amount every round, and the whole pot
//! rotates to one member per round in seat order. A 0.25% fee is withheld from
//! each payout and accrues to the treasury.
//!
//! ## Why a default can never be free
//!
//! A ROSCA extends credit: early recipients receive the pot before they have
//! paid for it. The vault secures that credit in three layers.
//!
//! 1. **Bond.** Every member locks one contribution when joining.
//! 2. **Your own pot is your collateral.** A member who misses a round before
//!    receiving has the gap covered from their bond, is moved to the back of
//!    the queue, and any uncovered amount is deducted from their own payout and
//!    paid to whoever was short-changed.
//! 3. **Escrow after receiving.** A recipient's payout is reduced by
//!    `collateral_bps` of their remaining dues (minus their bond). Every round
//!    they keep paying releases the excess back to them. A missed round is paid
//!    from that escrow, then from the bond.
//!
//! With `collateral_bps = 10_000` every default is covered in full. Lower
//! values trade safety for credit; any residue is recorded as debt against the
//! defaulter and as a claim for the member who was short-paid.
//!
//! ## Round lifecycle
//!
//! 1. `initialize` writes the configuration. The guild is empty.
//! 2. `join_guild` takes a seat and the bond; `leave_guild` refunds it. Both
//!    close at the first contribution.
//! 3. The first `deposit_contribution` opens round 0 (the guild must be full).
//! 4. Members contribute until `start_ledger + round_duration_ledgers`.
//! 5. Anyone may call `distribute_payout` after the deadline: immediately when
//!    every member paid, otherwise once the grace period has also expired.
//! 6. After the final round, `withdraw_collateral` returns each bond.
//!
//! ## Trust assumptions
//!
//! * The contract is immutable: there is no upgrade entrypoint.
//! * The admin can never move principal; the only privileged path is
//!   `withdraw_treasury_fees`, which pays the configured treasury address.
//! * Settlement is permissionless; no keeper role exists.
//! * Seat order changes only by the demotion rule above, never by a caller.

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

/// Emitted when a member takes a seat and posts the bond.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Join {
    #[topic]
    pub member: Address,
    pub index: u32,
    pub bond: i128,
}

/// Emitted when a member leaves before the guild starts.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Leave {
    #[topic]
    pub member: Address,
    pub refund: i128,
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
    /// Contributions plus collateral used to cover missed ones.
    pub pool: i128,
    pub fee: i128,
    /// USDC actually transferred to the recipient.
    pub paid: i128,
    /// Recipient's own debt deducted and passed to short-paid members.
    pub recovered: i128,
    /// Held back as escrow against the recipient's remaining dues.
    pub held: i128,
}

/// Emitted when a member misses a round.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Defaulted {
    #[topic]
    pub member: Address,
    pub round: u32,
    /// Paid into the pool from the member's escrow and bond.
    pub covered: i128,
    /// Left uncovered and recorded as debt.
    pub uncovered: i128,
    /// Whether the member was moved to the back of the queue.
    pub demoted: bool,
}

/// Emitted when held collateral returns to a member.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollateralReleased {
    #[topic]
    pub member: Address,
    pub amount: i128,
}

/// Emitted when a debt is settled and passed to a short-paid member.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebtSettled {
    #[topic]
    pub debtor: Address,
    #[topic]
    pub creditor: Address,
    pub amount: i128,
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
    pub fn initialize(
        env: Env,
        admin: Address,
        treasury: Address,
        usdc_token: Address,
        params: GuildParams,
    ) -> Result<(), Error> {
        admin.require_auth();

        if env.storage().instance().has(&DataKey::Config) {
            return Err(Error::AlreadyInitialized);
        }

        if params.contribution_amount < MIN_CONTRIBUTION
            || !(2..=MAX_MEMBERS).contains(&params.total_rounds)
            || !(MIN_ROUND_LEDGERS..=MAX_ROUND_LEDGERS).contains(&params.round_duration_ledgers)
            || params.grace_period_ledgers > MAX_GRACE_LEDGERS
            || params.collateral_bps > MAX_COLLATERAL_BPS
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
            contribution_amount: params.contribution_amount,
            round_duration_ledgers: params.round_duration_ledgers,
            grace_period_ledgers: params.grace_period_ledgers,
            total_rounds: params.total_rounds,
            collateral_bps: params.collateral_bps,
            bond_amount: params.contribution_amount,
        };

        let instance = env.storage().instance();
        instance.set(&DataKey::Config, &config);
        instance.set(
            &DataKey::CurrentRound,
            &RoundState {
                index: 0,
                start_ledger: 0,
                paid_count: 0,
            },
        );
        instance.set(&DataKey::VaultBalance, &0i128);
        instance.set(&DataKey::TreasuryBalance, &0i128);
        instance.set(&DataKey::CollateralBalance, &0i128);
        store_members(&env, &Vec::<Address>::new(&env));
        bump_instance(&env);

        Init { config }.publish(&env);
        Ok(())
    }

    /// Take a seat and post the bond. Open only until the first contribution.
    pub fn join_guild(env: Env, member: Address) -> Result<(), Error> {
        member.require_auth();

        let config = load_config(&env)?;
        let round = load_round(&env)?;

        if round.index >= config.total_rounds {
            return Err(Error::Completed);
        }
        if round.start_ledger != 0 {
            return Err(Error::AlreadyStarted);
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

        token::Client::new(&env, &config.usdc_token).transfer(
            &member,
            env.current_contract_address(),
            &config.bond_amount,
        );
        add_balance(&env, &DataKey::CollateralBalance, config.bond_amount)?;

        let index = members.len();
        members.push_back(member.clone());
        store_members(&env, &members);
        store_member(
            &env,
            &member,
            &MemberState {
                address: member.clone(),
                has_paid: false,
                reputation_score: REPUTATION_START,
                is_defaulted: false,
                missed_rounds: 0,
                received: false,
                bond: config.bond_amount,
                escrow: 0,
                debt: 0,
                shortfall: 0,
            },
        );
        bump_instance(&env);

        Join {
            member,
            index,
            bond: config.bond_amount,
        }
        .publish(&env);
        Ok(())
    }

    /// Give up a seat and recover the bond. Only before the guild starts.
    pub fn leave_guild(env: Env, member: Address) -> Result<(), Error> {
        member.require_auth();

        let config = load_config(&env)?;
        let round = load_round(&env)?;
        if round.start_ledger != 0 || round.index > 0 {
            return Err(Error::AlreadyStarted);
        }
        let state = load_member(&env, &member)?;

        let members = load_members(&env);
        let mut remaining = Vec::new(&env);
        for address in members.iter() {
            if address != member {
                remaining.push_back(address);
            }
        }
        store_members(&env, &remaining);
        env.storage()
            .persistent()
            .remove(&DataKey::MemberState(member.clone()));

        sub_balance(&env, &DataKey::CollateralBalance, state.bond)?;
        token::Client::new(&env, &config.usdc_token).transfer(
            &env.current_contract_address(),
            &member,
            &state.bond,
        );
        bump_instance(&env);

        Leave {
            member,
            refund: state.bond,
        }
        .publish(&env);
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
        } else if now > round.start_ledger + config.round_duration_ledgers {
            return Err(Error::DepositWindowClosed);
        }

        token::Client::new(&env, &config.usdc_token).transfer(
            &member,
            env.current_contract_address(),
            &config.contribution_amount,
        );

        state.has_paid = true;
        state.reputation_score = state
            .reputation_score
            .saturating_add(REPUTATION_PER_DEPOSIT)
            .min(REPUTATION_MAX);
        store_member(&env, &member, &state);
        add_balance(&env, &DataKey::VaultBalance, config.contribution_amount)?;

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
    /// Missed contributions are covered from the defaulter's escrow and bond;
    /// whatever remains uncovered becomes their debt and the recipient's claim.
    /// The recipient's own debt is deducted and passed to short-paid members,
    /// and `collateral_bps` of their remaining dues is held back as escrow.
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
        let all_paid = round.paid_count == config.total_rounds;
        if !all_paid && now <= deadline + config.grace_period_ledgers {
            return Err(Error::NotAllPaid);
        }

        let r = round.index;
        let c = config.contribution_amount;
        let token_client = token::Client::new(&env, &config.usdc_token);
        let contract = env.current_contract_address();
        let members = load_members(&env);
        let recipient = members.get(r).ok_or(Error::NotMember)?;
        // Dues each member still owes once this round is settled.
        let dues_after = dues(c, config.total_rounds - 1 - r, config.collateral_bps)?;

        let mut covered_total: i128 = 0;
        let mut uncovered_for_recipient: i128 = 0;
        let mut kept = Vec::new(&env);
        let mut demoted = Vec::new(&env);

        for (i, address) in members.iter().enumerate() {
            let i = i as u32;
            let mut state = load_member(&env, &address)?;
            let mut is_demoted = false;

            if state.has_paid {
                // An earlier recipient who keeps paying gets back the part of
                // their escrow their remaining dues no longer need.
                if state.received {
                    let keep = (dues_after - state.bond).max(0);
                    let release = state.escrow - keep;
                    if release > 0 {
                        state.escrow = keep;
                        sub_balance(&env, &DataKey::CollateralBalance, release)?;
                        token_client.transfer(&contract, &address, &release);
                        CollateralReleased {
                            member: address.clone(),
                            amount: release,
                        }
                        .publish(&env);
                    }
                }
            } else {
                let from_escrow = state.escrow.min(c);
                let from_bond = state.bond.min(c - from_escrow);
                let covered = from_escrow + from_bond;
                let uncovered = c - covered;
                state.escrow -= from_escrow;
                state.bond -= from_bond;
                sub_balance(&env, &DataKey::CollateralBalance, covered)?;
                covered_total += covered;

                if i != r && uncovered > 0 {
                    state.debt += uncovered;
                    uncovered_for_recipient += uncovered;
                }
                // A member who has not yet received loses their place: their
                // own pot is then the last one paid, so it backs their debt.
                is_demoted = i > r && !state.received;

                state.missed_rounds += 1;
                state.is_defaulted = true;
                state.reputation_score = 0;
                Defaulted {
                    member: address.clone(),
                    round: r,
                    covered,
                    uncovered,
                    demoted: is_demoted,
                }
                .publish(&env);
            }

            state.has_paid = false;
            store_member(&env, &address, &state);
            if is_demoted {
                demoted.push_back(address);
            } else {
                kept.push_back(address);
            }
        }

        let contributions = c
            .checked_mul(round.paid_count as i128)
            .ok_or(Error::MathOverflow)?;
        sub_balance(&env, &DataKey::VaultBalance, contributions)?;
        let pool = contributions + covered_total;
        let fee = pool.checked_mul(FEE_BPS).ok_or(Error::MathOverflow)? / BPS_DENOMINATOR;
        add_balance(&env, &DataKey::TreasuryBalance, fee)?;
        let net = pool - fee;

        let mut rec = load_member(&env, &recipient)?;
        let owed = rec.debt.min(net);
        rec.debt -= owed;
        let unclaimed = pay_creditors(&env, &token_client, &contract, &recipient, owed)?;
        let recovered = owed - unclaimed;

        let held = (dues_after - rec.bond).max(0).min(net - recovered);
        rec.escrow += held;
        add_balance(&env, &DataKey::CollateralBalance, held)?;
        rec.received = true;
        rec.shortfall += uncovered_for_recipient;
        store_member(&env, &recipient, &rec);

        let paid = net - recovered - held;
        if paid > 0 {
            token_client.transfer(&contract, &recipient, &paid);
        }

        for address in demoted.iter() {
            kept.push_back(address);
        }
        store_members(&env, &kept);

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
            round: r,
            pool,
            fee,
            paid,
            recovered,
            held,
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

    /// Flag a member who missed the round before settlement. Permissionless,
    /// only once the grace period has expired. Settlement covers the missed
    /// contribution either way; this only makes the default visible earlier.
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
        Ok(())
    }

    /// Pay off recorded debt in full. The USDC goes straight to the members
    /// who were short-paid, earliest seat first.
    pub fn repay_debt(env: Env, member: Address) -> Result<(), Error> {
        member.require_auth();

        let config = load_config(&env)?;
        let mut state = load_member(&env, &member)?;
        if state.debt <= 0 {
            return Err(Error::NoDebt);
        }

        let token_client = token::Client::new(&env, &config.usdc_token);
        let contract = env.current_contract_address();
        token_client.transfer(&member, &contract, &state.debt);
        let unclaimed = pay_creditors(&env, &token_client, &contract, &member, state.debt)?;
        if unclaimed > 0 {
            token_client.transfer(&contract, &member, &unclaimed);
        }
        state.debt = 0;
        store_member(&env, &member, &state);
        bump_instance(&env);
        Ok(())
    }

    /// Return a member's remaining bond and escrow once the cycle is over.
    pub fn withdraw_collateral(env: Env, member: Address) -> Result<(), Error> {
        member.require_auth();

        let config = load_config(&env)?;
        let round = load_round(&env)?;
        if round.index < config.total_rounds {
            return Err(Error::NotCompleted);
        }

        let mut state = load_member(&env, &member)?;
        let amount = state.bond + state.escrow;
        if amount <= 0 {
            return Err(Error::NothingToWithdraw);
        }
        state.bond = 0;
        state.escrow = 0;
        sub_balance(&env, &DataKey::CollateralBalance, amount)?;

        let token_client = token::Client::new(&env, &config.usdc_token);
        let contract = env.current_contract_address();
        // Outstanding debt is settled from collateral before anything returns.
        let owed = state.debt.min(amount);
        state.debt -= owed;
        let unclaimed = pay_creditors(&env, &token_client, &contract, &member, owed)?;
        let refund = amount - owed + unclaimed;
        store_member(&env, &member, &state);
        if refund > 0 {
            token_client.transfer(&contract, &member, &refund);
        }
        bump_instance(&env);

        CollateralReleased {
            member,
            amount: refund,
        }
        .publish(&env);
        Ok(())
    }

    /// Pay every accrued protocol fee to the treasury. Treasury auth required.
    pub fn withdraw_treasury_fees(env: Env) -> Result<(), Error> {
        let config = load_config(&env)?;
        config.treasury.require_auth();

        let treasury_balance = get_balance(&env, &DataKey::TreasuryBalance);
        if treasury_balance <= 0 {
            return Err(Error::NoFeesToWithdraw);
        }

        env.storage()
            .instance()
            .set(&DataKey::TreasuryBalance, &0i128);
        token::Client::new(&env, &config.usdc_token).transfer(
            &env.current_contract_address(),
            &config.treasury,
            &treasury_balance,
        );
        bump_instance(&env);

        FeesWithdrawn {
            treasury: config.treasury,
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

    /// Member list in seat order, which is also payout order.
    pub fn get_members(env: Env) -> Vec<Address> {
        load_members(&env)
    }

    /// USDC collected for the round in progress.
    pub fn get_vault_balance(env: Env) -> i128 {
        get_balance(&env, &DataKey::VaultBalance)
    }

    /// USDC owed to the treasury.
    pub fn get_treasury_balance(env: Env) -> i128 {
        get_balance(&env, &DataKey::TreasuryBalance)
    }

    /// USDC held as bonds and escrow across all members.
    pub fn get_collateral_balance(env: Env) -> i128 {
        get_balance(&env, &DataKey::CollateralBalance)
    }
}

// ---------------------------------------------------------------------------
// Accounting helpers
// ---------------------------------------------------------------------------

/// `collateral_bps` of `rounds` contributions.
fn dues(contribution: i128, rounds: u32, collateral_bps: u32) -> Result<i128, Error> {
    contribution
        .checked_mul(rounds as i128)
        .and_then(|v| v.checked_mul(collateral_bps as i128))
        .map(|v| v / BPS_DENOMINATOR)
        .ok_or(Error::MathOverflow)
}

/// Pass `amount` of `debtor`'s debt to short-paid members, earliest seat
/// first. Returns whatever no claim was left to absorb.
fn pay_creditors(
    env: &Env,
    token_client: &token::Client,
    contract: &Address,
    debtor: &Address,
    amount: i128,
) -> Result<i128, Error> {
    let mut left = amount;
    if left <= 0 {
        return Ok(0);
    }
    for address in load_members(env).iter() {
        if left == 0 {
            break;
        }
        let mut state = load_member(env, &address)?;
        let share = state.shortfall.min(left);
        if share <= 0 {
            continue;
        }
        state.shortfall -= share;
        left -= share;
        store_member(env, &address, &state);
        token_client.transfer(contract, &address, &share);
        DebtSettled {
            debtor: debtor.clone(),
            creditor: address,
            amount: share,
        }
        .publish(env);
    }
    Ok(left)
}

fn get_balance(env: &Env, key: &DataKey) -> i128 {
    env.storage().instance().get(key).unwrap_or(0)
}

fn add_balance(env: &Env, key: &DataKey, amount: i128) -> Result<(), Error> {
    let value = get_balance(env, key)
        .checked_add(amount)
        .ok_or(Error::MathOverflow)?;
    env.storage().instance().set(key, &value);
    Ok(())
}

fn sub_balance(env: &Env, key: &DataKey, amount: i128) -> Result<(), Error> {
    let value = get_balance(env, key)
        .checked_sub(amount)
        .ok_or(Error::MathOverflow)?;
    env.storage().instance().set(key, &value);
    Ok(())
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
