use crate::{Error, GuildParams, GuildVaultContract, GuildVaultContractClient};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{token, Address, Env, Vec};

/// 100 USDC in stroops.
const CONTRIBUTION: i128 = 1_000_000_000;
/// 100,000 USDC, enough for any test.
const FUND: i128 = 1_000_000_000_000;
const DURATION: u32 = 120;
const GRACE: u32 = 50;
const START: u32 = 1_000;

struct Ctx {
    admin: Address,
    treasury: Address,
    usdc: Address,
    contract: Address,
}

fn params(contribution: i128, duration: u32, grace: u32, rounds: u32, bps: u32) -> GuildParams {
    GuildParams {
        contribution_amount: contribution,
        round_duration_ledgers: duration,
        grace_period_ledgers: grace,
        total_rounds: rounds,
        collateral_bps: bps,
    }
}

/// Deploy a guild with `rounds` seats, bond-only collateral, and test USDC.
fn setup(env: &Env, rounds: u32) -> Ctx {
    setup_bps(env, rounds, 0)
}

/// Deploy a guild with `rounds` seats and the given `collateral_bps`.
fn setup_bps(env: &Env, rounds: u32, bps: u32) -> Ctx {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let treasury = Address::generate(env);
    let issuer = Address::generate(env);
    let usdc = env.register_stellar_asset_contract_v2(issuer).address();
    let contract = env.register(GuildVaultContract, ());
    let client = GuildVaultContractClient::new(env, &contract);
    client.initialize(
        &admin,
        &treasury,
        &usdc,
        &params(CONTRIBUTION, DURATION, GRACE, rounds, bps),
    );
    Ctx {
        admin,
        treasury,
        usdc,
        contract,
    }
}

fn mint(env: &Env, usdc: &Address, to: &Address, amount: i128) {
    token::StellarAssetClient::new(env, usdc).mint(to, &amount);
}

fn balance(env: &Env, usdc: &Address, who: &Address) -> i128 {
    token::Client::new(env, usdc).balance(who)
}

fn members(env: &Env, n: usize) -> Vec<Address> {
    let mut out = Vec::new(env);
    for _ in 0..n {
        out.push_back(Address::generate(env));
    }
    out
}

/// Fund one member with `FUND` and seat them.
fn join_one(env: &Env, usdc: &Address, client: &GuildVaultContractClient, member: &Address) {
    mint(env, usdc, member, FUND);
    client.join_guild(member);
}

/// Fund every member with `FUND` and seat them. Each posts a one-contribution bond.
fn join_all(env: &Env, usdc: &Address, client: &GuildVaultContractClient, list: &Vec<Address>) {
    for member in list.iter() {
        mint(env, usdc, &member, FUND);
        client.join_guild(&member);
    }
}

/// Invariant 1: the contract holds exactly its round vault, the treasury's
/// claim and every member's collateral.
fn assert_balanced(env: &Env, ctx: &Ctx, client: &GuildVaultContractClient) {
    assert_eq!(
        balance(env, &ctx.usdc, &ctx.contract),
        client.get_vault_balance()
            + client.get_treasury_balance()
            + client.get_collateral_balance()
    );
}

/// Run one full round: everyone contributes, then the round settles.
fn run_round(env: &Env, client: &GuildVaultContractClient, list: &Vec<Address>, round: u32) {
    env.ledger().set_sequence_number(START + round * DURATION);
    for member in list.iter() {
        client.deposit_contribution(&member);
    }
    env.ledger()
        .set_sequence_number(START + (round + 1) * DURATION);
    client.distribute_payout();
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

#[test]
fn test_calls_before_initialize_fail() {
    let env = Env::default();
    env.mock_all_auths();
    let contract = env.register(GuildVaultContract, ());
    let client = GuildVaultContractClient::new(&env, &contract);
    let who = Address::generate(&env);

    assert_eq!(client.try_join_guild(&who), Err(Ok(Error::NotInitialized)));
    assert_eq!(
        client.try_deposit_contribution(&who),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(
        client.try_distribute_payout(),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(
        client.try_handle_default(&who),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(
        client.try_withdraw_treasury_fees(),
        Err(Ok(Error::NotInitialized))
    );
    assert_eq!(client.try_get_config(), Err(Ok(Error::NotInitialized)));
    assert_eq!(client.try_get_round(), Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_initialize_stores_config() {
    let env = Env::default();
    let ctx = setup(&env, 4);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);

    let config = client.get_config();
    assert_eq!(config.admin, ctx.admin);
    assert_eq!(config.treasury, ctx.treasury);
    assert_eq!(config.usdc_token, ctx.usdc);
    assert_eq!(config.contribution_amount, CONTRIBUTION);
    assert_eq!(config.round_duration_ledgers, DURATION);
    assert_eq!(config.grace_period_ledgers, GRACE);
    assert_eq!(config.total_rounds, 4);

    let round = client.get_round();
    assert_eq!(round.index, 0);
    assert_eq!(round.start_ledger, 0);
    assert_eq!(round.paid_count, 0);
    assert_eq!(client.get_vault_balance(), 0);
    assert_eq!(client.get_treasury_balance(), 0);
    assert_eq!(client.get_members().len(), 0);
}

#[test]
fn test_initialize_twice_fails() {
    let env = Env::default();
    let ctx = setup(&env, 4);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);

    let result = client.try_initialize(
        &ctx.admin,
        &ctx.treasury,
        &ctx.usdc,
        &params(CONTRIBUTION, DURATION, GRACE, 4, 0),
    );
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_initialize_rejects_admin_equal_treasury() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let issuer = Address::generate(&env);
    let usdc = env.register_stellar_asset_contract_v2(issuer).address();
    let contract = env.register(GuildVaultContract, ());
    let client = GuildVaultContractClient::new(&env, &contract);

    let result = client.try_initialize(
        &admin,
        &admin,
        &usdc,
        &params(CONTRIBUTION, DURATION, GRACE, 4, 0),
    );
    assert_eq!(result, Err(Ok(Error::InvalidParams)));
}

#[test]
fn test_initialize_rejects_bad_bounds() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let issuer = Address::generate(&env);
    let usdc = env.register_stellar_asset_contract_v2(issuer).address();
    let contract = env.register(GuildVaultContract, ());
    let client = GuildVaultContractClient::new(&env, &contract);

    let cases: [(i128, u32, u32, u32); 6] = [
        (0, DURATION, GRACE, 4),              // contribution below minimum
        (CONTRIBUTION, DURATION, GRACE, 1),   // one seat
        (CONTRIBUTION, DURATION, GRACE, 51),  // over MAX_MEMBERS
        (CONTRIBUTION, 119, GRACE, 4),        // round too short
        (CONTRIBUTION, 1_051_201, GRACE, 4),  // round too long
        (CONTRIBUTION, DURATION, 120_961, 4), // grace too long
    ];

    for (amount, duration, grace, rounds) in cases {
        let result = client.try_initialize(
            &admin,
            &treasury,
            &usdc,
            &params(amount, duration, grace, rounds, 0),
        );
        assert_eq!(result, Err(Ok(Error::InvalidParams)));
    }
}

#[test]
fn test_initialize_rejects_account_as_token() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let not_a_token = Address::generate(&env);
    let contract = env.register(GuildVaultContract, ());
    let client = GuildVaultContractClient::new(&env, &contract);

    let result = client.try_initialize(
        &admin,
        &treasury,
        &not_a_token,
        &params(CONTRIBUTION, DURATION, GRACE, 4, 0),
    );
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Joining
// ---------------------------------------------------------------------------

#[test]
fn test_join_assigns_join_order() {
    let env = Env::default();
    let ctx = setup(&env, 3);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 3);

    join_all(&env, &ctx.usdc, &client, &list);

    let stored = client.get_members();
    assert_eq!(stored.len(), 3);
    for i in 0..3 {
        assert_eq!(stored.get(i).unwrap(), list.get(i).unwrap());
    }

    let first = client.get_member(&list.get(0).unwrap()).unwrap();
    assert_eq!(first.reputation_score, 0);
    assert!(!first.has_paid);
    assert!(!first.is_defaulted);
}

#[test]
fn test_join_duplicate_fails() {
    let env = Env::default();
    let ctx = setup(&env, 4);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_one(&env, &ctx.usdc, &client, &list.get(0).unwrap());
    assert_eq!(
        client.try_join_guild(&list.get(0).unwrap()),
        Err(Ok(Error::AlreadyMember))
    );
}

#[test]
fn test_join_beyond_total_rounds_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 3);

    join_one(&env, &ctx.usdc, &client, &list.get(0).unwrap());
    join_one(&env, &ctx.usdc, &client, &list.get(1).unwrap());
    assert_eq!(
        client.try_join_guild(&list.get(2).unwrap()),
        Err(Ok(Error::GuildFull))
    );
}

// ---------------------------------------------------------------------------
// Contributions
// ---------------------------------------------------------------------------

#[test]
fn test_deposit_requires_full_guild() {
    let env = Env::default();
    let ctx = setup(&env, 3);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);
    env.ledger().set_sequence_number(START);

    assert_eq!(
        client.try_deposit_contribution(&list.get(0).unwrap()),
        Err(Ok(Error::GuildNotFull))
    );
}

#[test]
fn test_deposit_moves_exact_usdc() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);
    env.ledger().set_sequence_number(START);

    let member = list.get(0).unwrap();
    let before = balance(&env, &ctx.usdc, &member);
    client.deposit_contribution(&member);

    assert_eq!(balance(&env, &ctx.usdc, &member), before - CONTRIBUTION);
    // Two bonds plus one contribution.
    assert_eq!(balance(&env, &ctx.usdc, &ctx.contract), 3 * CONTRIBUTION);
    assert_eq!(client.get_collateral_balance(), 2 * CONTRIBUTION);
    assert_eq!(client.get_vault_balance(), CONTRIBUTION);
}

#[test]
fn test_deposit_opens_round_zero() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);
    env.ledger().set_sequence_number(START);

    client.deposit_contribution(&list.get(0).unwrap());

    let round = client.get_round();
    assert_eq!(round.index, 0);
    assert_eq!(round.start_ledger, START);
    assert_eq!(round.paid_count, 1);

    let state = client.get_member(&list.get(0).unwrap()).unwrap();
    assert!(state.has_paid);
    assert_eq!(state.reputation_score, 10);
}

#[test]
fn test_deposit_at_deadline_succeeds() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);

    env.ledger().set_sequence_number(START);
    client.deposit_contribution(&list.get(0).unwrap());

    env.ledger().set_sequence_number(START + DURATION);
    client.deposit_contribution(&list.get(1).unwrap());
    assert_eq!(client.get_round().paid_count, 2);
}

#[test]
fn test_deposit_after_deadline_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);

    env.ledger().set_sequence_number(START);
    client.deposit_contribution(&list.get(0).unwrap());

    env.ledger().set_sequence_number(START + DURATION + 1);
    assert_eq!(
        client.try_deposit_contribution(&list.get(1).unwrap()),
        Err(Ok(Error::DepositWindowClosed))
    );

    let state = client.get_member(&list.get(1).unwrap()).unwrap();
    assert!(!state.has_paid);
}

#[test]
fn test_non_member_deposit_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let stranger = Address::generate(&env);

    env.ledger().set_sequence_number(START);
    assert_eq!(
        client.try_deposit_contribution(&stranger),
        Err(Ok(Error::NotMember))
    );
}

#[test]
fn test_deposit_without_balance_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    // Each member can afford the bond and nothing more.
    for member in list.iter() {
        mint(&env, &ctx.usdc, &member, CONTRIBUTION);
        client.join_guild(&member);
    }
    env.ledger().set_sequence_number(START);
    assert!(client
        .try_deposit_contribution(&list.get(0).unwrap())
        .is_err());
    assert_eq!(client.get_vault_balance(), 0);
}

#[test]
fn test_double_deposit_prevention() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);
    env.ledger().set_sequence_number(START);

    let member = list.get(0).unwrap();
    client.deposit_contribution(&member);
    let funded = balance(&env, &ctx.usdc, &member);

    assert_eq!(
        client.try_deposit_contribution(&member),
        Err(Ok(Error::AlreadyPaid))
    );
    assert_eq!(balance(&env, &ctx.usdc, &member), funded);
    assert_eq!(client.get_vault_balance(), CONTRIBUTION);
    assert_eq!(client.get_round().paid_count, 1);
}

/// Contributions large enough to overflow the running vault total are rejected
/// by the token layer before the vault arithmetic is reached, and nothing about
/// the round changes.
#[test]
fn test_overflow_amounts_cannot_corrupt_state() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let issuer = Address::generate(&env);
    let usdc = env.register_stellar_asset_contract_v2(issuer).address();
    let contract = env.register(GuildVaultContract, ());
    let client = GuildVaultContractClient::new(&env, &contract);
    // Three bonds plus two deposits fit in the contract's balance; a third
    // deposit would push it past i128::MAX.
    let huge = i128::MAX / 5;

    client.initialize(
        &admin,
        &treasury,
        &usdc,
        &params(huge, DURATION, GRACE, 3, 0),
    );

    let list = members(&env, 3);
    for member in list.iter() {
        mint(&env, &usdc, &member, 2 * huge);
        client.join_guild(&member);
    }
    env.ledger().set_sequence_number(START);

    client.deposit_contribution(&list.get(0).unwrap());
    client.deposit_contribution(&list.get(1).unwrap());

    assert!(client
        .try_deposit_contribution(&list.get(2).unwrap())
        .is_err());

    // The failed call left no trace: no vault change, member still unpaid.
    assert_eq!(client.get_vault_balance(), 2 * huge);
    assert!(!client.get_member(&list.get(2).unwrap()).unwrap().has_paid);
    assert_eq!(client.get_round().paid_count, 2);
}

// ---------------------------------------------------------------------------
// Payouts
// ---------------------------------------------------------------------------

#[test]
fn test_payout_before_first_deposit_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);

    assert_eq!(client.try_distribute_payout(), Err(Ok(Error::NotActive)));
}

#[test]
fn test_payout_before_deadline_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);
    env.ledger().set_sequence_number(START);
    for member in list.iter() {
        client.deposit_contribution(&member);
    }

    env.ledger().set_sequence_number(START + DURATION - 1);
    assert_eq!(
        client.try_distribute_payout(),
        Err(Ok(Error::RoundNotReady))
    );
}

#[test]
fn test_payout_requires_all_paid_inside_grace() {
    let env = Env::default();
    let ctx = setup(&env, 4);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 4);

    join_all(&env, &ctx.usdc, &client, &list);
    env.ledger().set_sequence_number(START);
    // Three of four members pay; the fourth is inside its grace period.
    for i in 0..3 {
        client.deposit_contribution(&list.get(i).unwrap());
    }

    env.ledger().set_sequence_number(START + DURATION + 1);
    assert_eq!(client.try_distribute_payout(), Err(Ok(Error::NotAllPaid)));
    assert_eq!(client.get_round().index, 0);
}

#[test]
fn test_payout_is_permissionless() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);
    env.ledger().set_sequence_number(START);
    for member in list.iter() {
        client.deposit_contribution(&member);
    }

    env.ledger().set_sequence_number(START + DURATION);
    // No authorization is mocked: settlement must not require any.
    env.mock_auths(&[]);
    client.distribute_payout();
    assert_eq!(client.get_round().index, 1);
}

#[test]
fn test_successful_cycle() {
    let env = Env::default();
    let ctx = setup(&env, 4);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 4);

    join_all(&env, &ctx.usdc, &client, &list);

    for round in 0..4 {
        run_round(&env, &client, &list, round);
    }

    // 4 x 100 USDC contributed, 4 x 399 USDC paid out, 4 x 1 USDC withheld.
    // Each member's 100 USDC bond is still held.
    let net_per_payout = 4 * CONTRIBUTION * 9975 / 10_000;
    for member in list.iter() {
        assert_eq!(
            balance(&env, &ctx.usdc, &member),
            FUND - CONTRIBUTION - 4 * CONTRIBUTION + net_per_payout
        );
    }
    assert_eq!(client.get_treasury_balance(), 40_000_000);
    assert_eq!(client.get_vault_balance(), 0);
    assert_eq!(client.get_collateral_balance(), 4 * CONTRIBUTION);
    assert_eq!(
        balance(&env, &ctx.usdc, &ctx.contract),
        40_000_000 + 4 * CONTRIBUTION
    );

    // Bonds come back once the cycle is over.
    for member in list.iter() {
        client.withdraw_collateral(&member);
        assert_eq!(
            balance(&env, &ctx.usdc, &member),
            FUND - 4 * CONTRIBUTION + net_per_payout
        );
    }
    assert_eq!(client.get_collateral_balance(), 0);
    assert_eq!(balance(&env, &ctx.usdc, &ctx.contract), 40_000_000);

    let round = client.get_round();
    assert_eq!(round.index, 4);
    assert_eq!(round.start_ledger, 0);
}

#[test]
fn test_recipient_rotates_in_join_order() {
    let env = Env::default();
    let ctx = setup(&env, 3);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 3);

    join_all(&env, &ctx.usdc, &client, &list);

    let pool = 3 * CONTRIBUTION;
    let expected_net = pool - pool * 25 / 10_000;

    for round in 0..3u32 {
        env.ledger().set_sequence_number(START + round * DURATION);
        for member in list.iter() {
            client.deposit_contribution(&member);
        }
        env.ledger()
            .set_sequence_number(START + (round + 1) * DURATION);

        let recipient = list.get(round).unwrap();
        let before = balance(&env, &ctx.usdc, &recipient);
        client.distribute_payout();
        assert_eq!(balance(&env, &ctx.usdc, &recipient), before + expected_net);
    }
}

#[test]
fn test_grace_period_and_default() {
    let env = Env::default();
    let ctx = setup(&env, 4);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 4);

    join_all(&env, &ctx.usdc, &client, &list);

    let paying = [0usize, 1, 2];
    env.ledger().set_sequence_number(START);
    for i in paying {
        client.deposit_contribution(&list.get(i as u32).unwrap());
    }

    let delinquent = list.get(3).unwrap();

    // Deadline reached, grace still running: settlement is refused.
    env.ledger().set_sequence_number(START + DURATION);
    assert_eq!(client.try_distribute_payout(), Err(Ok(Error::NotAllPaid)));
    assert_eq!(
        client.try_handle_default(&delinquent),
        Err(Ok(Error::GraceNotExpired))
    );

    // Grace expired: the default is recorded, then the round settles. The
    // delinquent's bond covers the missed contribution, so the pot is whole.
    env.ledger()
        .set_sequence_number(START + DURATION + GRACE + 1);
    client.handle_default(&delinquent);

    let state = client.get_member(&delinquent).unwrap();
    assert!(state.is_defaulted);
    assert_eq!(state.reputation_score, 0);

    let recipient = list.get(0).unwrap();
    let before = balance(&env, &ctx.usdc, &recipient);
    client.distribute_payout();

    let pool = 4 * CONTRIBUTION;
    let fee = pool * 25 / 10_000;
    assert_eq!(balance(&env, &ctx.usdc, &recipient), before + pool - fee);
    let state = client.get_member(&delinquent).unwrap();
    assert_eq!(state.bond, 0);
    assert_eq!(state.debt, 0);
    assert_eq!(state.missed_rounds, 1);
    assert_eq!(client.get_treasury_balance(), fee);
    assert_eq!(client.get_round().index, 1);
}

#[test]
fn test_handle_default_before_first_deposit_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);
    assert_eq!(
        client.try_handle_default(&list.get(0).unwrap()),
        Err(Ok(Error::NotActive))
    );
}

#[test]
fn test_handle_default_after_completion_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);
    for round in 0..2 {
        run_round(&env, &client, &list, round);
    }

    assert_eq!(
        client.try_handle_default(&list.get(0).unwrap()),
        Err(Ok(Error::Completed))
    );
}

#[test]
fn test_handle_default_on_paid_member_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);
    env.ledger().set_sequence_number(START);
    client.deposit_contribution(&list.get(0).unwrap());

    env.ledger()
        .set_sequence_number(START + DURATION + GRACE + 1);
    assert_eq!(
        client.try_handle_default(&list.get(0).unwrap()),
        Err(Ok(Error::MemberHasPaid))
    );
}

#[test]
fn test_handle_default_twice_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);
    env.ledger().set_sequence_number(START);
    client.deposit_contribution(&list.get(0).unwrap());

    let delinquent = list.get(1).unwrap();
    env.ledger()
        .set_sequence_number(START + DURATION + GRACE + 1);
    client.handle_default(&delinquent);
    assert_eq!(
        client.try_handle_default(&delinquent),
        Err(Ok(Error::AlreadyDefaulted))
    );
}

// ---------------------------------------------------------------------------
// Treasury
// ---------------------------------------------------------------------------

#[test]
fn test_treasury_withdrawal() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);
    run_round(&env, &client, &list, 0);

    let fee = 2 * CONTRIBUTION * 25 / 10_000;
    assert_eq!(client.get_treasury_balance(), fee);
    assert_eq!(balance(&env, &ctx.usdc, &ctx.treasury), 0);

    client.withdraw_treasury_fees();
    assert_eq!(client.get_treasury_balance(), 0);
    assert_eq!(balance(&env, &ctx.usdc, &ctx.treasury), fee);
    // Only the two bonds remain.
    assert_eq!(balance(&env, &ctx.usdc, &ctx.contract), 2 * CONTRIBUTION);
}

#[test]
fn test_withdraw_twice_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);
    run_round(&env, &client, &list, 0);

    client.withdraw_treasury_fees();
    assert_eq!(
        client.try_withdraw_treasury_fees(),
        Err(Ok(Error::NoFeesToWithdraw))
    );
}

#[test]
fn test_withdraw_unauthorized_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);
    run_round(&env, &client, &list, 0);

    let fee = 2 * CONTRIBUTION * 25 / 10_000;
    env.mock_auths(&[]);
    assert!(client.try_withdraw_treasury_fees().is_err());
    assert_eq!(client.get_treasury_balance(), fee);
}

// ---------------------------------------------------------------------------
// Completion and invariants
// ---------------------------------------------------------------------------

#[test]
fn test_completed_after_final_round() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&env, &ctx.usdc, &client, &list);
    for round in 0..2 {
        run_round(&env, &client, &list, round);
    }

    assert_eq!(client.try_distribute_payout(), Err(Ok(Error::Completed)));
    assert_eq!(
        client.try_deposit_contribution(&list.get(0).unwrap()),
        Err(Ok(Error::Completed))
    );
    assert_eq!(
        client.try_join_guild(&Address::generate(&env)),
        Err(Ok(Error::Completed))
    );
}

#[test]
fn test_invariant_balance_equation() {
    let env = Env::default();
    let ctx = setup(&env, 3);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 3);

    join_all(&env, &ctx.usdc, &client, &list);

    for round in 0..3u32 {
        env.ledger().set_sequence_number(START + round * DURATION);
        for member in list.iter() {
            client.deposit_contribution(&member);
            assert_balanced(&env, &ctx, &client);
        }
        env.ledger()
            .set_sequence_number(START + (round + 1) * DURATION);
        client.distribute_payout();
        assert_balanced(&env, &ctx, &client);
        assert_eq!(client.get_vault_balance(), 0);
    }

    // Fees and bonds remain, and nothing else.
    assert_eq!(
        balance(&env, &ctx.usdc, &ctx.contract),
        client.get_treasury_balance() + 3 * CONTRIBUTION
    );
}

// ---------------------------------------------------------------------------
// Collateral: bond, demotion, escrow, debt
// ---------------------------------------------------------------------------

/// Play one round starting at `*now`: `payers` contribute, then the round
/// settles as early as the rules allow. Leaves `*now` at the settlement ledger.
fn play(
    env: &Env,
    client: &GuildVaultContractClient,
    payers: &[Address],
    everyone: bool,
    now: &mut u32,
) {
    env.ledger().set_sequence_number(*now);
    for member in payers {
        client.deposit_contribution(member);
    }
    *now += if everyone {
        DURATION
    } else {
        DURATION + GRACE + 1
    };
    env.ledger().set_sequence_number(*now);
    client.distribute_payout();
}

/// Every unit of debt is somebody's claim.
fn assert_debts_match_claims(client: &GuildVaultContractClient, list: &Vec<Address>) {
    let (mut debt, mut claims) = (0i128, 0i128);
    for member in list.iter() {
        let state = client.get_member(&member).unwrap();
        debt += state.debt;
        claims += state.shortfall;
    }
    assert_eq!(debt, claims);
}

fn fee(pool: i128) -> i128 {
    pool * 25 / 10_000
}

#[test]
fn test_join_takes_bond() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let member = Address::generate(&env);

    join_one(&env, &ctx.usdc, &client, &member);
    assert_eq!(balance(&env, &ctx.usdc, &member), FUND - CONTRIBUTION);
    assert_eq!(client.get_member(&member).unwrap().bond, CONTRIBUTION);
    assert_eq!(client.get_collateral_balance(), CONTRIBUTION);
    assert_eq!(client.get_config().bond_amount, CONTRIBUTION);
}

#[test]
fn test_join_without_bond_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let member = Address::generate(&env);

    assert!(client.try_join_guild(&member).is_err());
    assert!(client.get_member(&member).is_none());
    assert_eq!(client.get_members().len(), 0);
}

#[test]
fn test_leave_before_start_refunds_bond() {
    let env = Env::default();
    let ctx = setup(&env, 3);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 3);
    join_all(&env, &ctx.usdc, &client, &list);

    let leaver = list.get(1).unwrap();
    client.leave_guild(&leaver);

    assert_eq!(balance(&env, &ctx.usdc, &leaver), FUND);
    assert!(client.get_member(&leaver).is_none());
    let seats = client.get_members();
    assert_eq!(seats.len(), 2);
    assert_eq!(seats.get(0).unwrap(), list.get(0).unwrap());
    assert_eq!(seats.get(1).unwrap(), list.get(2).unwrap());
    assert_eq!(client.get_collateral_balance(), 2 * CONTRIBUTION);

    // The seat is free again.
    let newcomer = Address::generate(&env);
    join_one(&env, &ctx.usdc, &client, &newcomer);
    assert_eq!(client.get_members().get(2).unwrap(), newcomer);
    assert_eq!(client.try_leave_guild(&leaver), Err(Ok(Error::NotMember)));
}

#[test]
fn test_membership_frozen_after_start() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);
    join_all(&env, &ctx.usdc, &client, &list);
    env.ledger().set_sequence_number(START);
    client.deposit_contribution(&list.get(0).unwrap());

    assert_eq!(
        client.try_leave_guild(&list.get(1).unwrap()),
        Err(Ok(Error::AlreadyStarted))
    );
    assert_eq!(
        client.try_join_guild(&Address::generate(&env)),
        Err(Ok(Error::AlreadyStarted))
    );
}

/// A member who misses before receiving is covered by their bond, then moved
/// to the back. A second miss is uncovered: it becomes their debt and the
/// short-paid recipient's claim, and is repaid from the debtor's own pot.
#[test]
fn test_default_before_receiving_is_recovered_from_own_pot() {
    let env = Env::default();
    let ctx = setup(&env, 4);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 4);
    join_all(&env, &ctx.usdc, &client, &list);
    let (a, b, c, d) = (
        list.get(0).unwrap(),
        list.get(1).unwrap(),
        list.get(2).unwrap(),
        list.get(3).unwrap(),
    );
    let full = 4 * CONTRIBUTION;
    let mut now = START;

    // Round 0: B misses. The bond covers it, A gets a full pot, B moves last.
    let a_before = balance(&env, &ctx.usdc, &a);
    play(
        &env,
        &client,
        &[a.clone(), c.clone(), d.clone()],
        false,
        &mut now,
    );
    assert_eq!(
        balance(&env, &ctx.usdc, &a),
        a_before - CONTRIBUTION + full - fee(full)
    );
    let seats = client.get_members();
    assert_eq!(
        [
            seats.get(0).unwrap(),
            seats.get(1).unwrap(),
            seats.get(2).unwrap(),
            seats.get(3).unwrap()
        ],
        [a.clone(), c.clone(), d.clone(), b.clone()]
    );
    let state = client.get_member(&b).unwrap();
    assert_eq!((state.bond, state.debt, state.missed_rounds), (0, 0, 1));
    assert_balanced(&env, &ctx, &client);

    // Round 1: B misses again with no bond left. C is short-paid by 100.
    let c_before = balance(&env, &ctx.usdc, &c);
    let short = 3 * CONTRIBUTION;
    play(
        &env,
        &client,
        &[a.clone(), c.clone(), d.clone()],
        false,
        &mut now,
    );
    assert_eq!(
        balance(&env, &ctx.usdc, &c),
        c_before - CONTRIBUTION + short - fee(short)
    );
    assert_eq!(client.get_member(&b).unwrap().debt, CONTRIBUTION);
    assert_eq!(client.get_member(&c).unwrap().shortfall, CONTRIBUTION);
    assert_debts_match_claims(&client, &list);
    assert_balanced(&env, &ctx, &client);

    // Round 2: everyone pays, D gets a full pot.
    play(
        &env,
        &client,
        &[a.clone(), b.clone(), c.clone(), d.clone()],
        true,
        &mut now,
    );

    // Round 3: B's own payout repays C before B sees anything.
    let b_before = balance(&env, &ctx.usdc, &b);
    let c_before = balance(&env, &ctx.usdc, &c);
    env.ledger().set_sequence_number(now);
    for m in [&a, &b, &c, &d] {
        client.deposit_contribution(m);
    }
    let b_after_deposit = balance(&env, &ctx.usdc, &b);
    now += DURATION;
    env.ledger().set_sequence_number(now);
    client.distribute_payout();
    assert_eq!(b_after_deposit, b_before - CONTRIBUTION);
    assert_eq!(
        balance(&env, &ctx.usdc, &b),
        b_after_deposit + full - fee(full) - CONTRIBUTION
    );
    assert_eq!(
        balance(&env, &ctx.usdc, &c),
        c_before - CONTRIBUTION + CONTRIBUTION
    );
    assert_eq!(client.get_member(&b).unwrap().debt, 0);
    assert_eq!(client.get_member(&c).unwrap().shortfall, 0);
    assert_debts_match_claims(&client, &list);
    assert_balanced(&env, &ctx, &client);
    assert_eq!(client.get_round().index, 4);
}

/// With `collateral_bps = 10_000` an early recipient who stops paying cannot
/// leave anyone short: their escrow and bond pay every remaining round.
#[test]
fn test_full_collateral_covers_default_after_receiving() {
    let env = Env::default();
    let ctx = setup_bps(&env, 4, 10_000);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 4);
    join_all(&env, &ctx.usdc, &client, &list);
    let (a, b, c, d) = (
        list.get(0).unwrap(),
        list.get(1).unwrap(),
        list.get(2).unwrap(),
        list.get(3).unwrap(),
    );
    let full = 4 * CONTRIBUTION;
    let everyone = [a.clone(), b.clone(), c.clone(), d.clone()];
    let mut now = START;

    // Round 0: A owes 3 more rounds; the bond covers one, escrow holds two.
    let a_before = balance(&env, &ctx.usdc, &a);
    play(&env, &client, &everyone, true, &mut now);
    assert_eq!(
        balance(&env, &ctx.usdc, &a),
        a_before - CONTRIBUTION + full - fee(full) - 2 * CONTRIBUTION
    );
    assert_eq!(client.get_member(&a).unwrap().escrow, 2 * CONTRIBUTION);
    assert_balanced(&env, &ctx, &client);

    // Round 1: A pays on time, so one round of escrow comes back.
    let a_before = balance(&env, &ctx.usdc, &a);
    play(&env, &client, &everyone, true, &mut now);
    assert_eq!(
        balance(&env, &ctx.usdc, &a),
        a_before - CONTRIBUTION + CONTRIBUTION
    );
    assert_eq!(client.get_member(&a).unwrap().escrow, CONTRIBUTION);

    // Rounds 2 and 3: A walks away. C and D still receive full pots.
    for recipient in [&c, &d] {
        let before = balance(&env, &ctx.usdc, recipient);
        play(
            &env,
            &client,
            &[b.clone(), c.clone(), d.clone()],
            false,
            &mut now,
        );
        assert_eq!(
            balance(&env, &ctx.usdc, recipient),
            before - CONTRIBUTION + full - fee(full)
        );
        assert_balanced(&env, &ctx, &client);
    }

    let state = client.get_member(&a).unwrap();
    assert_eq!((state.escrow, state.bond, state.debt), (0, 0, 0));
    assert_eq!(state.missed_rounds, 2);
    assert_debts_match_claims(&client, &list);
    assert_eq!(
        client.try_withdraw_collateral(&a),
        Err(Ok(Error::NothingToWithdraw))
    );
    // Honest members get their bonds back, and the contract keeps only fees.
    for m in [&b, &c, &d] {
        client.withdraw_collateral(m);
    }
    assert_eq!(client.get_collateral_balance(), 0);
    assert_eq!(
        balance(&env, &ctx.usdc, &ctx.contract),
        client.get_treasury_balance()
    );
}

/// With bond-only collateral, a recipient who walks away leaves recorded debt.
/// Repaying it sends the USDC straight to the members who were short-paid.
#[test]
fn test_bond_only_default_leaves_debt_that_can_be_repaid() {
    let env = Env::default();
    let ctx = setup(&env, 4);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 4);
    join_all(&env, &ctx.usdc, &client, &list);
    let (a, b, c, d) = (
        list.get(0).unwrap(),
        list.get(1).unwrap(),
        list.get(2).unwrap(),
        list.get(3).unwrap(),
    );
    let mut now = START;

    play(
        &env,
        &client,
        &[a.clone(), b.clone(), c.clone(), d.clone()],
        true,
        &mut now,
    );
    // No escrow is held at 0 bps: A takes the whole pot.
    assert_eq!(client.get_member(&a).unwrap().escrow, 0);
    for _ in 1..4 {
        play(
            &env,
            &client,
            &[b.clone(), c.clone(), d.clone()],
            false,
            &mut now,
        );
    }

    // Round 1 was covered by the bond; rounds 2 and 3 were not.
    assert_eq!(client.get_member(&a).unwrap().debt, 2 * CONTRIBUTION);
    assert_eq!(client.get_member(&b).unwrap().shortfall, 0);
    assert_eq!(client.get_member(&c).unwrap().shortfall, CONTRIBUTION);
    assert_eq!(client.get_member(&d).unwrap().shortfall, CONTRIBUTION);
    assert_debts_match_claims(&client, &list);
    assert_balanced(&env, &ctx, &client);

    let (c_before, d_before) = (balance(&env, &ctx.usdc, &c), balance(&env, &ctx.usdc, &d));
    client.repay_debt(&a);
    assert_eq!(balance(&env, &ctx.usdc, &c), c_before + CONTRIBUTION);
    assert_eq!(balance(&env, &ctx.usdc, &d), d_before + CONTRIBUTION);
    assert_eq!(client.get_member(&a).unwrap().debt, 0);
    assert_debts_match_claims(&client, &list);
    assert_eq!(client.try_repay_debt(&a), Err(Ok(Error::NoDebt)));
    assert_balanced(&env, &ctx, &client);
}

#[test]
fn test_withdraw_collateral_before_completion_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);
    join_all(&env, &ctx.usdc, &client, &list);
    run_round(&env, &client, &list, 0);

    assert_eq!(
        client.try_withdraw_collateral(&list.get(0).unwrap()),
        Err(Ok(Error::NotCompleted))
    );
}

#[test]
fn test_initialize_rejects_collateral_above_100_percent() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);
    let usdc = env
        .register_stellar_asset_contract_v2(Address::generate(&env))
        .address();
    let contract = env.register(GuildVaultContract, ());
    let client = GuildVaultContractClient::new(&env, &contract);

    assert_eq!(
        client.try_initialize(
            &admin,
            &treasury,
            &usdc,
            &params(CONTRIBUTION, DURATION, GRACE, 4, 10_001)
        ),
        Err(Ok(Error::InvalidParams))
    );
    client.initialize(
        &admin,
        &treasury,
        &usdc,
        &params(CONTRIBUTION, DURATION, GRACE, 4, 10_000),
    );
}
