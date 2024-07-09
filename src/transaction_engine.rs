use crate::account::DepositRecord;
use crate::account_store::AccountStore;
use crate::transaction::{Transaction, TransactionInfo};

/// Enum covering reasons why a transaction was not applied.
/// These may be for expected, valid reasons (e.g. insufficient funds)
/// or indicative of an error. The caller should use
/// [`TransactionNotApplied::is_failure`] to differentiate during handling.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum TransactionNotApplied {
    /// Account is locked so transaction could not be applied.
    AccountLocked,
    /// Transaction with ID has already been applied.
    RepeatTransaction(u32),
    /// Account could not be debited due to insufficient funds.
    InsufficientFunds,
    /// Dispute process failed due to unknwon transaction for this customer
    DisputedTransactionNotFound(u32),
    /// Dispute process failed to progress due to invalid dispute state
    InvalidDisputeState(String),
    /// Unexpected error
    UnexpectedError(String),
}

impl TransactionNotApplied {
    /// Checks whether the current variant of `self` represents a system
    /// failure (`true`) or a valid rejection of a transaction (`false`).
    pub fn is_failure(&self) -> bool {
        match self {
            TransactionNotApplied::AccountLocked => false,
            TransactionNotApplied::InsufficientFunds => false,
            // If we've seen this transaction before, something has gone wrong.
            TransactionNotApplied::RepeatTransaction(_) => true,
            // Either invalid input or a previously lost transaction.
            TransactionNotApplied::DisputedTransactionNotFound(_) => true,
            // Either invalid input or a previously lost dispute-related msg.
            TransactionNotApplied::InvalidDisputeState(_) => true,
            TransactionNotApplied::UnexpectedError(_) => true,
        }
    }
}

impl std::fmt::Display for TransactionNotApplied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransactionNotApplied::AccountLocked => write!(f, "Account Locked"),
            TransactionNotApplied::InsufficientFunds => write!(f, "Insufficient Funds"),
            TransactionNotApplied::RepeatTransaction(id) => write!(f, "Repeat Transaction: {}", id),
            TransactionNotApplied::DisputedTransactionNotFound(id) => {
                write!(f, "Transaction Not Found: {}", id)
            }
            TransactionNotApplied::InvalidDisputeState(err) => {
                write!(f, "Invalid state for disputed transaction: {}", err)
            }
            TransactionNotApplied::UnexpectedError(err) => write!(f, "Unexpected Error: {}", err),
        }
    }
}

/// Transaction Engine, applies transactions to accounts.
pub struct TxEngine<T> {
    state: T,
}

impl<T: AccountStore> TxEngine<T> {
    /// Creates a new instance of Transaction Engine wrapping the provided
    /// account store.
    pub fn new(state: T) -> Self {
        Self { state }
    }

    /// Accesses the underlying account store directly
    pub fn store(&self) -> &T {
        &self.state
    }

    #[cfg(test)]
    fn store_mut(&mut self) -> &mut T {
        &mut self.state
    }

    /// Apply a given transaction to the account store.
    ///
    /// Note: Returns an error for all cases where the requested transaction
    /// was not successfully applied. Some reasons may be valid and require no
    /// additional handling (i.e. not constituting a runtime "error").
    /// See [`TransactionNotApplied`] for more details.
    pub fn handle(
        &mut self,
        Transaction {
            client_id,
            transaction_id,
            info,
        }: &Transaction,
    ) -> Result<(), TransactionNotApplied> {
        let account = self.state.get_account_mut(*client_id);
        if account.locked {
            return Err(TransactionNotApplied::AccountLocked);
        }
        match info {
            TransactionInfo::Deposit(amount) => {
                if account.transactions.contains_key(transaction_id) {
                    return Err(TransactionNotApplied::RepeatTransaction(*transaction_id));
                }
                account.total_funds += *amount;
                account
                    .transactions
                    .insert(*transaction_id, DepositRecord::new(*amount));
            }
            TransactionInfo::Withdrawal(amount) => {
                if account.available_funds() < *amount {
                    return Err(TransactionNotApplied::InsufficientFunds);
                } else {
                    account.total_funds -= *amount;
                }
            }
            TransactionInfo::Dispute => {
                let tx_record = account.transactions.get_mut(transaction_id).ok_or(
                    TransactionNotApplied::DisputedTransactionNotFound(*transaction_id),
                )?;
                if let Err(err) = tx_record.disputed() {
                    return Err(TransactionNotApplied::InvalidDisputeState(err));
                }
                account.active_dispute_total += tx_record.amount;
            }
            TransactionInfo::Resolve => {
                let tx_record = account.transactions.get_mut(transaction_id).ok_or(
                    TransactionNotApplied::DisputedTransactionNotFound(*transaction_id),
                )?;
                if let Err(err) = tx_record.resolved() {
                    return Err(TransactionNotApplied::InvalidDisputeState(err));
                }
                let resolved_amount = tx_record.amount;
                // If this transaction ammount > current disputed funds,
                // then something has gone wrong and we may have failed to
                // hold sufficient funds for any remaining disputes. This
                // doesn't directly affect our ability to resolve _this_
                // dispute, but may indicate past or future bad handling,
                // so drop an error log.
                if account.free_disputed_amount(&resolved_amount) {
                    // TODO log it
                }
            }
            TransactionInfo::Chargeback => {
                let tx_record = account.transactions.get_mut(transaction_id).ok_or(
                    TransactionNotApplied::DisputedTransactionNotFound(*transaction_id),
                )?;
                if let Err(err) = tx_record.refunded() {
                    return Err(TransactionNotApplied::InvalidDisputeState(err));
                }
                let cb_amount = tx_record.amount;
                if account.free_disputed_amount(&cb_amount) {
                    // TODO log it
                }
                account.total_funds -= cb_amount;
                account.locked = true;
            }
        };
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::account::{Account, DisputeStatus};
    use crate::account_store::{AccountStore, InMemoryStore};
    use rust_decimal_macros::dec;

    const CLIENT_ID_DEFAULT: u16 = 123;
    const TX_ID_DEFAULT: u32 = 1;

    fn engine_with_def_account() -> TxEngine<InMemoryStore> {
        let stub_store = InMemoryStore::new_with_data(vec![Account {
            client: CLIENT_ID_DEFAULT,
            locked: false,
            ..Account::default()
        }]);
        TxEngine::new(stub_store)
    }

    /// Utility macro to construct instances of `Transaction`.
    /// Uses CLIENT_ID_DEFAULT and TX_ID_DEFAULT unless they are specified.
    /// Amount is mandatory for deposit and withdrawal types. Target
    /// transaction ID is mandatory for other types.
    macro_rules! txn {
        (Deposit, $amount:expr) => {
            txn!(Deposit, $amount, TX_ID_DEFAULT)
        };
        (Withdrawal, $amount:expr) => {
            txn!(Withdrawal, $amount, TX_ID_DEFAULT)
        };
        ($txn_typ:ident, $txn_id:expr) => {
            txn!($txn_typ, None, $txn_id)
        };
        ($txn_typ:ident, None, $txn_id:expr) => {
            Transaction {
                client_id: CLIENT_ID_DEFAULT,
                transaction_id: $txn_id,
                info: TransactionInfo::$txn_typ,
            }
        };
        ($txn_typ:ident, $amount:expr, $txn_id:expr) => {
            Transaction {
                client_id: CLIENT_ID_DEFAULT,
                transaction_id: $txn_id,
                info: TransactionInfo::$txn_typ(dec!($amount)),
            }
        };
    }

    #[test]
    fn locked_account_transactions_not_applied() {
        let mut engine = engine_with_def_account();
        {
            let acc = engine.store_mut().get_account_mut(123);
            acc.locked = true;
        }
        let resp = engine.handle(&txn!(Deposit, 1)).unwrap_err();
        assert_eq!(resp, TransactionNotApplied::AccountLocked);

        // Nothing changed since not applied.
        let acc = engine.store().get_account(123).unwrap();
        assert_eq!(acc.available_funds(), dec!(0));
        assert_eq!(acc.held_funds(), dec!(0));
        assert!(!acc.transactions.contains_key(&1));
    }

    #[test]
    fn deposit() {
        let mut engine = engine_with_def_account();
        engine.handle(&txn!(Deposit, 1)).unwrap();

        let acc = engine.store().get_account(123).unwrap();
        assert_eq!(acc.available_funds(), dec!(1));
        assert_eq!(acc.held_funds(), dec!(0));
        assert!(acc.transactions.contains_key(&1));
    }

    #[test]
    fn withdrawal() {
        let mut engine = engine_with_def_account();
        engine.handle(&txn!(Deposit, 100)).unwrap();
        engine.handle(&txn!(Withdrawal, 50, 2)).unwrap();

        let acc = engine.store().get_account(123).unwrap();
        assert_eq!(acc.available_funds(), dec!(50));
        assert_eq!(acc.held_funds(), dec!(0));
        assert!(!acc.transactions.contains_key(&2));
    }

    #[test]
    fn withdraw_overdrawn() {
        let mut engine = engine_with_def_account();
        engine.handle(&txn!(Deposit, 100)).unwrap();
        let resp = engine.handle(&txn!(Withdrawal, 150, 2)).unwrap_err();
        assert_eq!(resp, TransactionNotApplied::InsufficientFunds);

        let acc = engine.store().get_account(123).unwrap();
        assert_eq!(acc.available_funds(), dec!(100));
        assert_eq!(acc.held_funds(), dec!(0));
        assert!(!acc.transactions.contains_key(&2));
    }

    #[test]
    fn repeat_transaction_id() {
        let mut engine = engine_with_def_account();
        engine.handle(&txn!(Deposit, 100, 1)).unwrap();
        let resp = engine.handle(&txn!(Deposit, 150, 1)).unwrap_err();
        assert!(matches!(resp, TransactionNotApplied::RepeatTransaction(_)));
    }

    #[test]
    fn dispute_phases_invalid_tx_id() {
        // Checks dispute-phase transactions are rejected if they reference
        // a transaction that:
        // - doesn't exist; or
        // - doesn't belong to that client; or
        // - isn't a deposit
        let mut engine = engine_with_def_account();
        engine.handle(&txn!(Deposit, 100)).unwrap();
        let resp = engine.handle(&txn!(Dispute, 2)).unwrap_err();
        assert!(matches!(
            resp,
            TransactionNotApplied::DisputedTransactionNotFound(_)
        ));
    }

    #[test]
    fn dispute_bad_tx_state() {
        // Dispute gets rejected if referenced transaction is not in a valid
        // state to start the dispute process.
        let mut engine = engine_with_def_account();
        engine.handle(&txn!(Deposit, 100, 1)).unwrap();
        // Start dispute
        engine.handle(&txn!(Dispute, 1)).unwrap();
        // Repeat
        let resp = engine.handle(&txn!(Dispute, 1)).unwrap_err();
        assert!(matches!(
            resp,
            TransactionNotApplied::InvalidDisputeState(_)
        ));
    }

    #[test]
    fn resolve_bad_tx_state() {
        // Resolve gets rejected if referenced transaction is not in a valid
        // state to start the dispute process.
        let mut engine = engine_with_def_account();
        engine.handle(&txn!(Deposit, 100, 1)).unwrap();
        let resp = engine.handle(&txn!(Resolve, 1)).unwrap_err();
        assert!(matches!(
            resp,
            TransactionNotApplied::InvalidDisputeState(_)
        ));
    }

    #[test]
    fn chargeback_bad_tx_state() {
        // Chargeback gets rejected if referenced transaction is not in a valid
        // state to start the dispute process.
        let mut engine = engine_with_def_account();
        engine.handle(&txn!(Deposit, 100, 1)).unwrap();
        let resp = engine.handle(&txn!(Chargeback, 1)).unwrap_err();
        assert!(matches!(
            resp,
            TransactionNotApplied::InvalidDisputeState(_)
        ));
    }

    #[test]
    fn dispute_transitions_success() {
        let mut engine = engine_with_def_account();
        engine.handle(&txn!(Deposit, 100, 1)).unwrap();
        engine.handle(&txn!(Deposit, 50, 2)).unwrap();

        // Initial dispute
        engine.handle(&txn!(Dispute, 1)).unwrap();
        {
            let acc = engine.store().get_account(123).unwrap();
            assert_eq!(acc.available_funds(), dec!(50));
            assert_eq!(acc.held_funds(), dec!(100));
            assert!(acc.transactions.get(&1).unwrap().dispute_status() == DisputeStatus::Disputed);
        }

        // Resolve it
        engine.handle(&txn!(Resolve, 1)).unwrap();
        {
            let acc = engine.store().get_account(123).unwrap();
            assert_eq!(acc.available_funds(), dec!(150));
            assert_eq!(acc.held_funds(), dec!(0));
            assert!(acc.transactions.get(&1).unwrap().dispute_status() == DisputeStatus::Resolved);
        }

        // Re-initialize the dispute.
        engine.handle(&txn!(Dispute, 1)).unwrap();
        {
            let acc = engine.store().get_account(123).unwrap();
            assert_eq!(acc.available_funds(), dec!(50));
            assert_eq!(acc.held_funds(), dec!(100));
            assert!(acc.transactions.get(&1).unwrap().dispute_status() == DisputeStatus::Disputed);
        }

        // Now chargeback.
        engine.handle(&txn!(Chargeback, 1)).unwrap();
        let acc = engine.store().get_account(123).unwrap();
        assert_eq!(acc.available_funds(), dec!(50));
        assert_eq!(acc.held_funds(), dec!(0));
        assert!(acc.transactions.get(&1).unwrap().dispute_status() == DisputeStatus::Refunded);
        assert!(acc.locked);
    }
}
