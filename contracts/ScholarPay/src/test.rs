#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::Address as _,
    token, Address, Env,
};

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Deploy ScholarPay contract + mock USDC token, initialize the pool.
fn setup() -> (Env, Address, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    // Deploy mock USDC token
    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract(token_admin.clone());
    let token_asset = token::StellarAssetClient::new(&env, &token_id);

    // Deploy ScholarPay contract
    let contract_id = env.register_contract(None, ScholarPayContract);
    let contract = ScholarPayContractClient::new(&env, &contract_id);

    // Addresses
    let admin = Address::generate(&env);
    let student_a = Address::generate(&env);
    let student_b = Address::generate(&env);

    // Mint USDC to students (500 USDC each = 5_000_000_000 stroops)
    token_asset.mint(&student_a, &5_000_000_000i128);
    token_asset.mint(&student_b, &5_000_000_000i128);

    // Initialize the pool
    contract.initialize(&admin, &token_id);

    (env, contract_id, token_id, admin, student_a, student_b)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// TEST 1 — Happy Path
/// Two students deposit into the pool. Admin issues a loan to student A.
/// Student A repays with 5% fee. Pool total grows from the fee yield.
#[test]
fn test_happy_path_deposit_loan_repay() {
    let (env, contract_id, token_id, admin, student_a, student_b) = setup();
    let contract = ScholarPayContractClient::new(&env, &contract_id);
    let token = token::Client::new(&env, &token_id);

    // Both students deposit 100 USDC each → pool = 200 USDC
    let deposit: i128 = 1_000_000_000; // 100 USDC
    contract.deposit(&student_a, &deposit);
    contract.deposit(&student_b, &deposit);

    assert_eq!(contract.get_pool_total(), 2_000_000_000i128);

    // Admin issues 50 USDC loan to student A
    let principal: i128 = 500_000_000; // 50 USDC
    let loan_id = contract.issue_loan(&admin, &student_a, &principal);

    // Pool reduced by principal
    assert_eq!(contract.get_pool_total(), 1_500_000_000i128);

    // Student A repays: 50 USDC + 5% = 52.5 USDC
    contract.repay_loan(&student_a, &loan_id);

    // Pool now = 150 USDC + 52.5 USDC repayment = 202.5 USDC
    let expected_pool = 2_025_000_000i128;
    assert_eq!(contract.get_pool_total(), expected_pool);

    // Loan marked repaid
    let loan = contract.get_loan(&loan_id);
    assert!(loan.repaid);
}

/// TEST 2 — Edge Case: Student with active loan cannot withdraw savings
#[test]
#[should_panic(expected = "Repay your loan before withdrawing savings")]
fn test_cannot_withdraw_with_active_loan() {
    let (env, contract_id, token_id, admin, student_a, _student_b) = setup();
    let contract = ScholarPayContractClient::new(&env, &contract_id);

    // Student A deposits 200 USDC
    contract.deposit(&student_a, &2_000_000_000i128);

    // Admin issues 50 USDC loan
    contract.issue_loan(&admin, &student_a, &500_000_000i128);

    // Student A tries to withdraw savings — must panic
    contract.withdraw(&student_a, &500_000_000i128);
}

/// TEST 3 — State Verification
/// After deposit + loan + repayment, verify all on-chain state is correct:
/// member.active_loan == false, loan.repaid == true, pool total is accurate.
#[test]
fn test_state_after_full_loan_cycle() {
    let (env, contract_id, token_id, admin, student_a, _student_b) = setup();
    let contract = ScholarPayContractClient::new(&env, &contract_id);

    let deposit: i128 = 2_000_000_000; // 200 USDC
    let principal: i128 = 1_000_000_000; // 100 USDC

    contract.deposit(&student_a, &deposit);
    let loan_id = contract.issue_loan(&admin, &student_a, &principal);
    contract.repay_loan(&student_a, &loan_id);

    // Member state: active_loan cleared, deposits recorded
    let member = contract.get_member(&student_a);
    assert!(!member.active_loan, "active_loan should be false after repayment");
    assert_eq!(member.deposited, deposit);
    assert_eq!(member.withdrawn, 0);

    // Loan state: repaid = true, not defaulted
    let loan = contract.get_loan(&loan_id);
    assert!(loan.repaid);
    assert!(!loan.defaulted);
    assert_eq!(loan.principal, principal);
    assert_eq!(loan.repay_amount, 1_050_000_000i128); // 100 USDC + 5%

    // Pool total: started at 200, loaned 100, repaid 105 → 205 USDC
    assert_eq!(contract.get_pool_total(), 2_050_000_000i128);
}

/// TEST 4 — Admin Default Flow
/// Admin marks a loan as defaulted. Borrower's active_loan flag is cleared.
/// Loan is not repaid — pool absorbs the loss.
#[test]
fn test_admin_can_mark_default() {
    let (env, contract_id, token_id, admin, student_a, _student_b) = setup();
    let contract = ScholarPayContractClient::new(&env, &contract_id);

    contract.deposit(&student_a, &2_000_000_000i128);
    let loan_id = contract.issue_loan(&admin, &student_a, &500_000_000i128);

    // Admin marks loan as defaulted
    contract.mark_default(&admin, &loan_id);

    let loan = contract.get_loan(&loan_id);
    assert!(loan.defaulted);
    assert!(!loan.repaid);

    // Member's active_loan flag should be cleared so pool can continue
    let member = contract.get_member(&student_a);
    assert!(!member.active_loan);
}

/// TEST 5 — Withdrawal After Repayment
/// Student can withdraw their savings after repaying a loan in full.
#[test]
fn test_withdrawal_after_loan_repaid() {
    let (env, contract_id, token_id, admin, student_a, _student_b) = setup();
    let contract = ScholarPayContractClient::new(&env, &contract_id);
    let token = token::Client::new(&env, &token_id);

    // Student deposits 200 USDC
    let deposit: i128 = 2_000_000_000;
    contract.deposit(&student_a, &deposit);

    // Track wallet balance after deposit
    let balance_after_deposit = token.balance(&student_a);

    // Admin loans 100 USDC to student A
    let loan_id = contract.issue_loan(&admin, &student_a, &1_000_000_000i128);

    // Student A repays the loan (principal 100 + 5% = 105 USDC)
    contract.repay_loan(&student_a, &loan_id);

    // Student A withdraws their 200 USDC savings
    contract.withdraw(&student_a, &deposit);

    let member = contract.get_member(&student_a);
    assert_eq!(member.withdrawn, deposit);
    assert!(!member.active_loan);

    // Pool should be: 0 deposits left (200 withdrawn) + 5 USDC fee yield = 5 USDC
    // 200 deposited - 100 loaned = 100 in pool, +105 repaid = 205, -200 withdrawn = 5
    assert_eq!(contract.get_pool_total(), 50_000_000i128); // 5 USDC
}