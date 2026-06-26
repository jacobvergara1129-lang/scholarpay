// ScholarPay — Student Savings Pool & Peer Lending on Stellar
// Soroban Smart Contract: Cooperative savings pool with micro-lending

#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short,
    token, Address, Env,
};

// ─── Storage Keys ────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Member(Address),       // Savings balance per student member
    Loan(u64),             // Loan record by ID
    LoanCount,             // Auto-increment loan counter
    PoolTotal,             // Total USDC pooled in the contract
    PoolToken,             // The USDC token address for this pool
    Admin,                 // Pool admin (e.g. school coordinator)
}

// ─── Data Structures ─────────────────────────────────────────────────────────

/// A student's membership record in the savings pool
#[contracttype]
#[derive(Clone)]
pub struct Member {
    pub address: Address,
    pub deposited: i128,   // Cumulative amount deposited (in USDC stroops)
    pub withdrawn: i128,   // Cumulative amount withdrawn
    pub active_loan: bool, // Whether this member has an outstanding loan
}

/// A micro-loan issued from the pool to a student
#[contracttype]
#[derive(Clone)]
pub struct Loan {
    pub loan_id: u64,
    pub borrower: Address,
    pub principal: i128,       // Amount borrowed (in USDC stroops)
    pub repay_amount: i128,    // Principal + flat fee (e.g. 5% flat)
    pub repaid: bool,          // Whether the loan has been fully repaid
    pub defaulted: bool,       // Whether the loan was marked defaulted by admin
}

// ─── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct ScholarPayContract;

#[contractimpl]
impl ScholarPayContract {

    /// Initialize the pool — sets the admin and the USDC token address.
    /// Must be called once after deployment before any other function.
    ///
    /// # Arguments
    /// * `admin` - School coordinator or DAO multisig managing the pool
    /// * `token` - USDC token contract address on Stellar
    pub fn initialize(env: Env, admin: Address, token: Address) {
        admin.require_auth();

        // Prevent re-initialization
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("Already initialized");
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::PoolToken, &token);
        env.storage().instance().set(&DataKey::PoolTotal, &0i128);
    }

    /// A student deposits USDC into the shared savings pool.
    /// Their personal savings balance is tracked separately from the pool total.
    /// Any student can join by making their first deposit.
    ///
    /// # Arguments
    /// * `student`  - The student depositing funds (must authorize)
    /// * `amount`   - Amount to deposit in USDC stroops (min 1 USDC = 10_000_000)
    pub fn deposit(env: Env, student: Address, amount: i128) {
        student.require_auth();

        if amount <= 0 {
            panic!("Deposit amount must be positive");
        }

        let token: Address = env.storage().instance().get(&DataKey::PoolToken).expect("Not initialized");
        let token_client = token::Client::new(&env, &token);

        // Pull USDC from student wallet into pool
        token_client.transfer(&student, &env.current_contract_address(), &amount);

        // Update the student's member record
        let mut member: Member = env
            .storage()
            .persistent()
            .get(&DataKey::Member(student.clone()))
            .unwrap_or(Member {
                address: student.clone(),
                deposited: 0,
                withdrawn: 0,
                active_loan: false,
            });

        member.deposited += amount;
        env.storage().persistent().set(&DataKey::Member(student.clone()), &member);

        // Update global pool total
        let pool_total: i128 = env.storage().instance().get(&DataKey::PoolTotal).unwrap_or(0);
        env.storage().instance().set(&DataKey::PoolTotal, &(pool_total + amount));

        env.events().publish((symbol_short!("deposit"), student), amount);
    }

    /// A student withdraws their own savings from the pool.
    /// They can only withdraw up to their net deposited balance (deposits minus active loan principal).
    /// Students with an active loan cannot withdraw until the loan is repaid.
    ///
    /// # Arguments
    /// * `student` - The student withdrawing (must authorize)
    /// * `amount`  - Amount to withdraw in USDC stroops
    pub fn withdraw(env: Env, student: Address, amount: i128) {
        student.require_auth();

        if amount <= 0 {
            panic!("Withdrawal amount must be positive");
        }

        let mut member: Member = env
            .storage()
            .persistent()
            .get(&DataKey::Member(student.clone()))
            .expect("Not a pool member");

        if member.active_loan {
            panic!("Repay your loan before withdrawing savings");
        }

        let net_balance = member.deposited - member.withdrawn;
        if amount > net_balance {
            panic!("Insufficient savings balance");
        }

        // Ensure the pool has enough liquidity
        let pool_total: i128 = env.storage().instance().get(&DataKey::PoolTotal).unwrap_or(0);
        if amount > pool_total {
            panic!("Pool liquidity insufficient");
        }

        let token: Address = env.storage().instance().get(&DataKey::PoolToken).expect("Not initialized");
        let token_client = token::Client::new(&env, &token);

        // Deduct first (checks-effects-interactions)
        member.withdrawn += amount;
        env.storage().persistent().set(&DataKey::Member(student.clone()), &member);
        env.storage().instance().set(&DataKey::PoolTotal, &(pool_total - amount));

        // Transfer USDC to student
        token_client.transfer(&env.current_contract_address(), &student, &amount);

        env.events().publish((symbol_short!("withdraw"), student), amount);
    }

    /// Issue a micro-loan to a student from the pool.
    /// Only the admin can approve loans to prevent abuse.
    /// The borrower must be a pool member and must not have an active loan.
    /// Repayment amount = principal + 5% flat fee (returned to pool as yield).
    ///
    /// # Arguments
    /// * `admin`     - Pool admin (must authorize)
    /// * `borrower`  - The student receiving the loan
    /// * `principal` - Loan amount in USDC stroops
    pub fn issue_loan(env: Env, admin: Address, borrower: Address, principal: i128) -> u64 {
        admin.require_auth();

        // Verify caller is the registered admin
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).expect("Not initialized");
        if admin != stored_admin {
            panic!("Only admin can issue loans");
        }

        if principal <= 0 {
            panic!("Loan principal must be positive");
        }

        let mut member: Member = env
            .storage()
            .persistent()
            .get(&DataKey::Member(borrower.clone()))
            .expect("Borrower must be a pool member");

        if member.active_loan {
            panic!("Borrower already has an active loan");
        }

        let pool_total: i128 = env.storage().instance().get(&DataKey::PoolTotal).unwrap_or(0);
        if principal > pool_total {
            panic!("Insufficient pool liquidity for this loan");
        }

        let token: Address = env.storage().instance().get(&DataKey::PoolToken).expect("Not initialized");
        let token_client = token::Client::new(&env, &token);

        // 5% flat fee — all yield stays in the pool for other members
        let repay_amount = principal + (principal * 5 / 100);

        // Assign loan ID
        let loan_id: u64 = env.storage().instance().get(&DataKey::LoanCount).unwrap_or(0) + 1;
        env.storage().instance().set(&DataKey::LoanCount, &loan_id);

        // Record the loan
        let loan = Loan {
            loan_id,
            borrower: borrower.clone(),
            principal,
            repay_amount,
            repaid: false,
            defaulted: false,
        };
        env.storage().persistent().set(&DataKey::Loan(loan_id), &loan);

        // Mark member as having an active loan
        member.active_loan = true;
        env.storage().persistent().set(&DataKey::Member(borrower.clone()), &member);

        // Deduct from pool and send to borrower
        env.storage().instance().set(&DataKey::PoolTotal, &(pool_total - principal));
        token_client.transfer(&env.current_contract_address(), &borrower, &principal);

        env.events().publish((symbol_short!("loaned"), borrower), principal);

        loan_id
    }

    /// Repay a loan in full.
    /// The borrower sends back the full repay_amount (principal + 5% fee).
    /// On success, the loan is marked repaid and the member's active_loan flag is cleared.
    ///
    /// # Arguments
    /// * `borrower` - The student repaying (must authorize)
    /// * `loan_id`  - The loan to repay
    pub fn repay_loan(env: Env, borrower: Address, loan_id: u64) {
        borrower.require_auth();

        let mut loan: Loan = env
            .storage()
            .persistent()
            .get(&DataKey::Loan(loan_id))
            .expect("Loan not found");

        if loan.borrower != borrower {
            panic!("Only the borrower can repay this loan");
        }
        if loan.repaid {
            panic!("Loan already repaid");
        }
        if loan.defaulted {
            panic!("Loan has been marked defaulted");
        }

        let token: Address = env.storage().instance().get(&DataKey::PoolToken).expect("Not initialized");
        let token_client = token::Client::new(&env, &token);

        // Pull full repayment (principal + fee) back into pool
        token_client.transfer(&borrower, &env.current_contract_address(), &loan.repay_amount);

        // Pool grows by the repay_amount (principal returns + fee yield)
        let pool_total: i128 = env.storage().instance().get(&DataKey::PoolTotal).unwrap_or(0);
        env.storage().instance().set(&DataKey::PoolTotal, &(pool_total + loan.repay_amount));

        // Clear the borrower's active loan flag
        let mut member: Member = env
            .storage()
            .persistent()
            .get(&DataKey::Member(borrower.clone()))
            .expect("Member not found");
        member.active_loan = false;
        env.storage().persistent().set(&DataKey::Member(borrower.clone()), &member);

        // Mark loan as repaid
        loan.repaid = true;
        env.storage().persistent().set(&DataKey::Loan(loan_id), &loan);

        env.events().publish((symbol_short!("repaid"), borrower), loan.repay_amount);
    }

    /// Admin marks a loan as defaulted (e.g. student dropped out).
    /// Clears the active_loan flag so the pool can continue operating.
    /// The principal loss is socialized across the pool (covered by fee yield over time).
    ///
    /// # Arguments
    /// * `admin`   - Pool admin (must authorize)
    /// * `loan_id` - The loan to mark defaulted
    pub fn mark_default(env: Env, admin: Address, loan_id: u64) {
        admin.require_auth();

        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).expect("Not initialized");
        if admin != stored_admin {
            panic!("Only admin can mark defaults");
        }

        let mut loan: Loan = env
            .storage()
            .persistent()
            .get(&DataKey::Loan(loan_id))
            .expect("Loan not found");

        if loan.repaid {
            panic!("Cannot default a repaid loan");
        }

        loan.defaulted = true;
        env.storage().persistent().set(&DataKey::Loan(loan_id), &loan);

        // Free the borrower's active loan flag
        let mut member: Member = env
            .storage()
            .persistent()
            .get(&DataKey::Member(loan.borrower.clone()))
            .expect("Member not found");
        member.active_loan = false;
        env.storage().persistent().set(&DataKey::Member(loan.borrower.clone()), &member);

        env.events().publish((symbol_short!("default"), loan.loan_id), loan.principal);
    }

    // ─── View Functions ──────────────────────────────────────────────────────

    /// Returns a student's member record (deposits, withdrawals, loan status).
    pub fn get_member(env: Env, student: Address) -> Member {
        env.storage()
            .persistent()
            .get(&DataKey::Member(student))
            .expect("Not a pool member")
    }

    /// Returns the details of a specific loan.
    pub fn get_loan(env: Env, loan_id: u64) -> Loan {
        env.storage()
            .persistent()
            .get(&DataKey::Loan(loan_id))
            .expect("Loan not found")
    }

    /// Returns the total USDC liquidity currently in the pool.
    pub fn get_pool_total(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::PoolTotal).unwrap_or(0)
    }
}