# ScholarPay 🎓

> Student savings pool with peer micro-lending — built on Stellar for university students across Southeast Asia.

---

## Problem

A 19-year-old nursing student in Surabaya, Indonesia needs ₹800,000 IDR (~$50 USD) to cover her semester lab fees before payday. She has no credit history, no collateral, and no access to a bank loan — forcing her to borrow from informal lenders at 20–30% monthly interest, or drop the semester entirely.

## Solution

ScholarPay lets students pool their USDC savings on-chain via a Soroban smart contract. When a pooled member needs emergency funds, a verified admin (school coordinator or student council) approves a micro-loan disbursed directly from the pool. The borrower repays principal + a flat 5% fee — which stays in the pool as yield for all members. No bank, no collateral, no predatory interest.

---

## Stellar Features Used

| Feature | Usage |
|---|---|
| USDC transfers | Deposits, loan disbursements, repayments, withdrawals |
| Soroban smart contracts | Pool accounting, loan lifecycle, default handling |
| Trustlines | Student wallets trust USDC issuer before joining pool |
| Stellar's fast finality | Loan disbursement hits student wallet in <5 seconds |

---

## Target Users

- **Who:** University students (ages 18–25) with irregular income — part-time workers, scholars, stipend recipients
- **Where:** Philippines, Indonesia, Vietnam — students with smartphones but no credit history
- **Why they care:** Emergency cash access without loan sharks; savings grow passively from pool fee yield; trustless and transparent

---

## MVP Core Feature (Demo Flow — under 2 min)