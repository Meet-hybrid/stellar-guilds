use crate::{Error, GuildVaultContract, GuildVaultContractClient};
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

/// Deploy a guild with `rounds` seats and a test USDC asset.
fn setup(env: &Env, rounds: u32) -> Ctx {
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
        &CONTRIBUTION,
        &DURATION,
        &GRACE,
        &rounds,
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

fn join_all(client: &GuildVaultContractClient, list: &Vec<Address>) {
    for member in list.iter() {
        client.join_guild(&member);
    }
}

fn fund_all(env: &Env, usdc: &Address, list: &Vec<Address>) {
    for member in list.iter() {
        mint(env, usdc, &member, FUND);
    }
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
        &CONTRIBUTION,
        &DURATION,
        &GRACE,
        &4,
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

    let result = client.try_initialize(&admin, &admin, &usdc, &CONTRIBUTION, &DURATION, &GRACE, &4);
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
            &admin, &treasury, &usdc, &amount, &duration, &grace, &rounds,
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
        &CONTRIBUTION,
        &DURATION,
        &GRACE,
        &4,
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

    join_all(&client, &list);

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

    client.join_guild(&list.get(0).unwrap());
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

    client.join_guild(&list.get(0).unwrap());
    client.join_guild(&list.get(1).unwrap());
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);
    env.ledger().set_sequence_number(START);

    let member = list.get(0).unwrap();
    let before = balance(&env, &ctx.usdc, &member);
    client.deposit_contribution(&member);

    assert_eq!(balance(&env, &ctx.usdc, &member), before - CONTRIBUTION);
    assert_eq!(balance(&env, &ctx.usdc, &ctx.contract), CONTRIBUTION);
    assert_eq!(client.get_vault_balance(), CONTRIBUTION);
}

#[test]
fn test_deposit_opens_round_zero() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);

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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);

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

    join_all(&client, &list);
    // No minting at all: the transfer must fail.
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);
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
    let huge = i128::MAX / 2;

    client.initialize(&admin, &treasury, &usdc, &huge, &DURATION, &GRACE, &3);

    let list = members(&env, 3);
    join_all(&client, &list);
    for member in list.iter() {
        mint(&env, &usdc, &member, huge);
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);

    for round in 0..4 {
        run_round(&env, &client, &list, round);
    }

    // 4 x 100 USDC contributed, 4 x 399 USDC paid out, 4 x 1 USDC withheld.
    let net_per_payout = 4 * CONTRIBUTION * 9975 / 10_000;
    for member in list.iter() {
        assert_eq!(
            balance(&env, &ctx.usdc, &member),
            FUND - 4 * CONTRIBUTION + net_per_payout
        );
    }
    assert_eq!(client.get_treasury_balance(), 40_000_000);
    assert_eq!(client.get_vault_balance(), 0);
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);

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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);

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

    // Grace expired: the default is recorded, then the round settles pro-rata.
    env.ledger()
        .set_sequence_number(START + DURATION + GRACE + 1);
    client.handle_default(&delinquent);

    let state = client.get_member(&delinquent).unwrap();
    assert!(state.is_defaulted);
    assert_eq!(state.reputation_score, 0);

    let recipient = list.get(0).unwrap();
    let before = balance(&env, &ctx.usdc, &recipient);
    client.distribute_payout();

    let pool = 3 * CONTRIBUTION;
    let fee = pool * 25 / 10_000;
    assert_eq!(balance(&env, &ctx.usdc, &recipient), before + pool - fee);
    assert_eq!(client.get_treasury_balance(), fee);
    assert_eq!(client.get_round().index, 1);
}

#[test]
fn test_handle_default_before_first_deposit_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&client, &list);
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);
    run_round(&env, &client, &list, 0);

    let fee = 2 * CONTRIBUTION * 25 / 10_000;
    assert_eq!(client.get_treasury_balance(), fee);
    assert_eq!(balance(&env, &ctx.usdc, &ctx.treasury), 0);

    client.withdraw_treasury_fees();
    assert_eq!(client.get_treasury_balance(), 0);
    assert_eq!(balance(&env, &ctx.usdc, &ctx.treasury), fee);
    assert_eq!(balance(&env, &ctx.usdc, &ctx.contract), 0);
}

#[test]
fn test_withdraw_twice_fails() {
    let env = Env::default();
    let ctx = setup(&env, 2);
    let client = GuildVaultContractClient::new(&env, &ctx.contract);
    let list = members(&env, 2);

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);
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

    join_all(&client, &list);
    fund_all(&env, &ctx.usdc, &list);

    for round in 0..3u32 {
        env.ledger().set_sequence_number(START + round * DURATION);
        for member in list.iter() {
            client.deposit_contribution(&member);
            // Contract holds exactly what was collected, all of it unallocated.
            assert_eq!(
                balance(&env, &ctx.usdc, &ctx.contract),
                client.get_vault_balance() + client.get_treasury_balance()
            );
        }
        env.ledger()
            .set_sequence_number(START + (round + 1) * DURATION);
        client.distribute_payout();
        assert_eq!(
            balance(&env, &ctx.usdc, &ctx.contract),
            client.get_vault_balance() + client.get_treasury_balance()
        );
        assert_eq!(client.get_vault_balance(), 0);
    }

    // Only fees remain, and they equal the treasury claim.
    assert_eq!(
        balance(&env, &ctx.usdc, &ctx.contract),
        client.get_treasury_balance()
    );
}
