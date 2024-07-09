use std::error::Error;
use std::io::{Read, Write};

mod account;
mod account_store;
mod transaction;
mod transaction_engine;

use account_store::{AccountStore, InMemoryStore};
use transaction::{Transaction, TransactionRaw};
use transaction_engine::TxEngine;

/// Transactions that were rejected due to account state or invalid input.
/// Transaction ID + description of rejection cause.
pub type RejectedTransactions = Vec<(u32, String)>;
/// Valid transactions that we failed to apply. Store these so they aren't lost.
/// Transaction + description of failure cause.
pub type FailedTransactions = Vec<(Transaction, String)>;

/// Runs the engine to completion, parsing all rows in the input csv and
/// printing the resulting account state for all clients.
pub fn run_with_csv<R: Read, W: Write>(
    reader: R,
    writer: W,
) -> Result<(RejectedTransactions, FailedTransactions), Box<dyn Error>> {
    let mut csv_reader = csv::ReaderBuilder::new()
        .has_headers(true)
        // allow missing fields
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(reader);

    // Rejected transactions. For a system taking inputs from some client
    // service (rather than a static file), we'd send an appropriate response
    // rejecting these transactions.
    let mut rejected_transactions: RejectedTransactions = vec![];
    // Failed transactions. For a real system taking inputs from some client
    // service, we'd also retry and send notifications indicating we could not
    // apply the transaction.
    let mut dead_letter_queue: FailedTransactions = vec![];

    let mut handler = TxEngine::new(InMemoryStore::new());
    for transaction in csv_reader.deserialize::<TransactionRaw>() {
        let transaction_raw = match transaction {
            Ok(tx) => tx,
            // Ideally we'd intervene before here, log the string that
            // couldn't be deserialized, and send a rejection response.
            // For now, just log it and move on.
            Err(_err) => {
                continue;
            }
        };
        // Save the ID so we can use it for logging/failure handling.
        let tx_id = transaction_raw.tx;
        let transaction_parsed = match Transaction::try_from(transaction_raw) {
            Ok(tx) => tx,
            Err(_err) => {
                rejected_transactions.push((tx_id, "Malformed Transaction".into()));
                continue;
            }
        };
        let res = handler.handle(&transaction_parsed);
        if let Err(err) = res {
            if err.is_failure() {
                dead_letter_queue.push((transaction_parsed, err.to_string()));
            } else {
                rejected_transactions.push((tx_id, err.to_string()));
            }
        }
    }

    // Done processing. Write out our results.
    let mut csv_writer = csv::Writer::from_writer(writer);
    for account_statement in handler.store().account_statements() {
        csv_writer.serialize(account_statement)?;
    }
    csv_writer.flush()?;
    Ok((rejected_transactions, dead_letter_queue))
}
