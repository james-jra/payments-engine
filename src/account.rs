use rust_decimal::Decimal;
use serde::Serialize;
use std::cmp::{max, min};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct Account {
    /// Client ID associated with this account.
    pub client: u16,

    /// Account raw funds, may be negative if account is overdrawn.
    pub total_funds: Decimal,

    /// Total of all current disputes.
    ///
    /// Actively disputed funds may exceed total funds in the case where an
    /// account has accrued disputes exceeding its remaining balance. For held
    /// funds, use [`Account::held_funds`] instead.
    pub active_dispute_total: Decimal,

    /// Whether or not the account is frozen.
    pub locked: bool,

    /// Map of all transactions related to this account.
    pub transactions: HashMap<u32, DepositRecord>,
}

impl Account {
    /// Creates a new instance of [`Account`] with zero funds.
    pub fn new(client: u16) -> Self {
        Self {
            client,
            ..Account::default()
        }
    }

    /// Returns the funds available for withdrawal.
    pub fn available_funds(&self) -> Decimal {
        max(self.total_funds - self.active_dispute_total, Decimal::ZERO)
    }

    /// Returns the calculated held funds due to disputes.
    ///
    /// This is the amount of the account's total funds held back to cover
    /// disputed payments.
    pub fn held_funds(&self) -> Decimal {
        min(
            self.active_dispute_total,
            max(self.total_funds, Decimal::ZERO),
        )
    }

    /// Frees the requested disputed amount to be available for use.
    ///
    /// Returns `true` if the requested amount is greater than the current
    /// total disputed funds. This represents an error to be handled by
    /// the caller.
    pub fn free_disputed_amount(&mut self, amount: &Decimal) -> bool {
        let new_disputed = self.active_dispute_total - amount;
        if new_disputed < Decimal::ZERO {
            self.active_dispute_total = Decimal::ZERO;
            true
        } else {
            self.active_dispute_total = new_disputed;
            false
        }
    }
}

/// Serializable summary of an account's state intended for reporting.
///
/// Note: when constructing an [`AccountStatement`] from an [`Account`], all
/// values of funds are rounded to 4 decimal places.
#[derive(Debug, Serialize)]
pub struct AccountStatement {
    client: u16,
    available: Decimal,
    held: Decimal,
    total: Decimal,
    locked: bool,
}

impl std::convert::From<&Account> for AccountStatement {
    fn from(src: &Account) -> Self {
        Self {
            client: src.client,
            available: src.available_funds().round_dp(4),
            held: src.held_funds().round_dp(4),
            total: src.total_funds.round_dp(4),
            locked: src.locked,
        }
    }
}

/// A deposit that was successfully processed for an account.
#[derive(Debug)]
pub struct DepositRecord {
    pub amount: Decimal,
    // Private, so we can enforce transitions via methods instead.
    dispute_status: DisputeStatus,
}

impl DepositRecord {
    pub fn new(amount: Decimal) -> Self {
        Self {
            dispute_status: DisputeStatus::NotDisputed,
            amount,
        }
    }

    #[cfg(test)]
    pub fn dispute_status(&self) -> DisputeStatus {
        self.dispute_status
    }

    pub fn disputed(&mut self) -> Result<(), String> {
        if self.dispute_status == DisputeStatus::Disputed
            || self.dispute_status == DisputeStatus::Refunded
        {
            return Err(format!(
                "Cannot begin dispute from current transaciton state {:?}",
                self.dispute_status
            ));
        }
        self.dispute_status = DisputeStatus::Disputed;
        Ok(())
    }

    pub fn resolved(&mut self) -> Result<(), String> {
        if self.dispute_status != DisputeStatus::Disputed {
            return Err(format!(
                "Cannot resolve dispute from current transaction state {:?}",
                self.dispute_status
            ));
        }
        self.dispute_status = DisputeStatus::Resolved;
        Ok(())
    }

    pub fn refunded(&mut self) -> Result<(), String> {
        if self.dispute_status != DisputeStatus::Disputed {
            return Err(format!(
                "Cannot chargeback from current transaction state {:?}",
                self.dispute_status
            ));
        }
        self.dispute_status = DisputeStatus::Refunded;
        Ok(())
    }
}

/// Dispute status denote's a transaction's position in the dispute process.
///
/// Valid transitions are:
/// NotDisputed -> Disputed
/// Disputed -> {Resolved, Refunded}
/// Resolved -> Disputed
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum DisputeStatus {
    NotDisputed,
    Disputed,
    Resolved,
    Refunded,
}

#[cfg(test)]
mod test {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn hold_funds_for_disputed_transactions() {
        let mut acc = Account::new(1);
        acc.total_funds = dec!(100);
        assert_eq!(acc.available_funds(), dec!(100));
        assert_eq!(acc.held_funds(), dec!(0));

        acc.active_dispute_total = dec!(50);
        assert_eq!(acc.available_funds(), dec!(50));
        assert_eq!(acc.held_funds(), dec!(50));
        assert_eq!(acc.total_funds, dec!(100));

        acc.active_dispute_total = dec!(100);
        assert_eq!(acc.available_funds(), dec!(0));
        assert_eq!(acc.held_funds(), dec!(100));
        assert_eq!(acc.total_funds, dec!(100));

        // Start another dispute pushing the total disputed funds
        // past what's available in total_funds.
        // We should still get sensible values in "available" and "held"
        // compared to the total available funds (i.e. available >= 0
        // and held <= total).
        acc.active_dispute_total = dec!(125);
        assert_eq!(acc.available_funds(), dec!(0));
        assert_eq!(acc.held_funds(), dec!(100));
        assert_eq!(acc.total_funds, dec!(100));

        // Resolve a dispute, bringing the disputed funds back below the
        // total available. Ensure we didn't spontaneously gain some available
        // funds due to the ceiling imposed by total_funds.
        acc.free_disputed_amount(&dec!(50));
        assert_eq!(acc.available_funds(), dec!(25));
        assert_eq!(acc.held_funds(), dec!(75));
        assert_eq!(acc.total_funds, dec!(100));
    }

    #[test]
    fn prevent_negative_dispute_total() {
        // Ensure we never "free" more disputed funds than we're aware of.
        let mut acc = Account::new(1);
        acc.total_funds = dec!(100);
        acc.active_dispute_total = dec!(50);
        assert_eq!(acc.available_funds(), dec!(50));
        assert_eq!(acc.held_funds(), dec!(50));

        // Free most of what's currently disputed
        assert!(!acc.free_disputed_amount(&dec!(45)));
        assert_eq!(acc.available_funds(), dec!(95));
        assert_eq!(acc.held_funds(), dec!(5));
        assert_eq!(acc.total_funds, dec!(100));

        // Then go over - shouldn't happen unless we've miscalculated elsewhere
        // or are trying to free/chargeback an incorrect/missed transaction.
        // Check we notice (boolean true response to free_disputed_amount)
        // and don't magically gain some more available funds.
        assert!(acc.free_disputed_amount(&dec!(10)));
        assert_eq!(acc.available_funds(), dec!(100));
        assert_eq!(acc.held_funds(), dec!(0));
        assert_eq!(acc.total_funds, dec!(100));
    }

    #[test]
    fn dispute_phase_transitions() {
        fn tx_rec(initial: DisputeStatus) -> DepositRecord {
            DepositRecord {
                dispute_status: initial,
                amount: dec!(100),
            }
        }
        assert!(tx_rec(DisputeStatus::NotDisputed).disputed().is_ok());
        assert!(tx_rec(DisputeStatus::Disputed).disputed().is_err());
        assert!(tx_rec(DisputeStatus::Resolved).disputed().is_ok());
        assert!(tx_rec(DisputeStatus::Refunded).disputed().is_err());

        assert!(tx_rec(DisputeStatus::NotDisputed).resolved().is_err());
        assert!(tx_rec(DisputeStatus::Disputed).resolved().is_ok());
        assert!(tx_rec(DisputeStatus::Resolved).resolved().is_err());
        assert!(tx_rec(DisputeStatus::Refunded).resolved().is_err());

        assert!(tx_rec(DisputeStatus::NotDisputed).refunded().is_err());
        assert!(tx_rec(DisputeStatus::Disputed).refunded().is_ok());
        assert!(tx_rec(DisputeStatus::Resolved).refunded().is_err());
        assert!(tx_rec(DisputeStatus::Refunded).refunded().is_err());
    }
}
