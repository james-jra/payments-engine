use crate::account::{Account, AccountStatement};
use std::collections::HashMap;

/// Trait for accessing Account and transaction state from a state store.
pub trait AccountStore {
    /// Returns a shared reference to the referenced [`Account`].
    fn get_account(&self, client_id: u16) -> Option<&Account>;

    /// Returns a mutable reference to the referenced [`Account`].
    ///
    /// If the [`Account`] with the requested ID is not present, one is
    /// created and a mutable reference returned.
    fn get_account_mut(&mut self, client_id: u16) -> &mut Account;

    /// Generate account statements for all contained accounts.
    fn account_statements(&self) -> impl Iterator<Item = AccountStatement>;
}

/// In-memory implementation of the [`AccountStore`] trait.
pub struct InMemoryStore {
    data: HashMap<u16, Account>,
}

impl InMemoryStore {
    /// Returns a new empty instance of [`InMemoryStore`]
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    #[cfg(test)]
    /// Returns a new instance of [`InMemoryStore`] with the provided accounts.
    pub fn new_with_data(accounts: Vec<Account>) -> Self {
        let data = accounts
            .into_iter()
            .map(|acc| (acc.client, acc))
            .collect::<HashMap<u16, Account>>();
        Self { data }
    }
}

impl AccountStore for InMemoryStore {
    fn get_account(&self, client_id: u16) -> Option<&Account> {
        self.data.get(&client_id)
    }

    fn get_account_mut(&mut self, client_id: u16) -> &mut Account {
        self.data
            .entry(client_id)
            .or_insert_with(|| Account::new(client_id))
    }

    fn account_statements(&self) -> impl Iterator<Item = AccountStatement> {
        self.data.values().map(|account| account.into())
    }
}
