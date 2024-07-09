use rust_decimal::Decimal;
use serde::Deserialize;

/// Basic flat datastructure used to deserialize transactions
#[derive(Debug, PartialEq, Deserialize)]
pub struct TransactionRaw {
    #[serde(rename = "type")]
    pub transaction_type: String,
    pub client: u16,
    pub tx: u32,
    pub amount: Option<Decimal>,
}

/// Representation of a transaction
#[derive(Debug, PartialEq)]
pub struct Transaction {
    pub client_id: u16,
    pub transaction_id: u32,
    pub info: TransactionInfo,
}

/// Transaction type and, where relevant, the associated amount.
#[derive(Debug, PartialEq)]
pub enum TransactionInfo {
    Deposit(Decimal),
    Withdrawal(Decimal),
    Dispute,
    Resolve,
    Chargeback,
}

impl std::convert::TryFrom<TransactionRaw> for Transaction {
    type Error = (u32, String);

    fn try_from(value: TransactionRaw) -> Result<Transaction, Self::Error> {
        let info = match (value.transaction_type.as_str(), value.amount) {
            // Round on input. The engine only supports 4 DP, so we need to
            // avoid compounding rounding errors on output. E.g. erroneous
            // deposits of 1.00003 + 1.00003 => 2.0000, not 2.0001
            ("deposit", Some(amount)) if amount > Decimal::ZERO => {
                TransactionInfo::Deposit(amount.round_dp(4))
            }
            ("withdrawal", Some(amount)) if amount > Decimal::ZERO => {
                TransactionInfo::Withdrawal(amount.round_dp(4))
            }
            ("dispute", None) => TransactionInfo::Dispute,
            ("resolve", None) => TransactionInfo::Resolve,
            ("chargeback", None) => TransactionInfo::Chargeback,
            _ => {
                return Err((
                    value.tx,
                    format!("Failed to parse raw transaction {:?}", value),
                ));
            }
        };
        Ok(Self {
            client_id: value.client,
            transaction_id: value.tx,
            info,
        })
    }
}

#[cfg(test)]
mod transaction_deserialization {
    use super::*;
    use rust_decimal_macros::dec;

    fn tx_raw(typ: &str, amount: Option<Decimal>) -> TransactionRaw {
        TransactionRaw {
            transaction_type: typ.to_string(),
            client: 1,
            tx: 1,
            amount,
        }
    }

    #[test]
    fn parse_transaction_raw_ok_cases() {
        assert_eq!(
            Transaction::try_from(tx_raw("deposit", Some(dec!(1)))).unwrap(),
            Transaction {
                client_id: 1,
                transaction_id: 1,
                info: TransactionInfo::Deposit(dec!(1)),
            }
        );
        assert_eq!(
            Transaction::try_from(tx_raw("withdrawal", Some(dec!(1)))).unwrap(),
            Transaction {
                client_id: 1,
                transaction_id: 1,
                info: TransactionInfo::Withdrawal(dec!(1)),
            }
        );
        assert_eq!(
            Transaction::try_from(tx_raw("dispute", None)).unwrap(),
            Transaction {
                client_id: 1,
                transaction_id: 1,
                info: TransactionInfo::Dispute,
            }
        );
        assert_eq!(
            Transaction::try_from(tx_raw("resolve", None)).unwrap(),
            Transaction {
                client_id: 1,
                transaction_id: 1,
                info: TransactionInfo::Resolve,
            }
        );
        assert_eq!(
            Transaction::try_from(tx_raw("chargeback", None)).unwrap(),
            Transaction {
                client_id: 1,
                transaction_id: 1,
                info: TransactionInfo::Chargeback,
            }
        );
    }

    #[test]
    fn parse_transaction_raw_error_cases() {
        // Transactions missing amounts
        assert!(Transaction::try_from(tx_raw("deposit", None)).is_err());
        assert!(Transaction::try_from(tx_raw("withdrawal", None)).is_err());
        // Transactions that shouldn't have amounts
        assert!(Transaction::try_from(tx_raw("dispute", Some(dec!(1)))).is_err());
        assert!(Transaction::try_from(tx_raw("resolve", Some(dec!(1)))).is_err());
        assert!(Transaction::try_from(tx_raw("chargeback", Some(dec!(1)))).is_err());
        // Unrecognized transaction type
        assert!(Transaction::try_from(tx_raw("not a real type", None)).is_err());
        assert!(Transaction::try_from(tx_raw("not a real type", Some(dec!(1)))).is_err());

        // Invalid transaction amount
        assert!(Transaction::try_from(tx_raw("deposit", Some(dec!(0)))).is_err());
        assert!(Transaction::try_from(tx_raw("deposit", Some(dec!(-1)))).is_err());
    }
}
