//! Account deposit is information per user about their balances in the exchange.

use std::collections::HashMap;
use std::convert::TryInto;

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::json_types::{ValidAccountId, U128};
use near_sdk::{assert_one_yocto, env, near_bindgen, AccountId, Balance};

use crate::utils::{ext_fungible_token, ERR_NOT_REGISTERED, GAS_FOR_FT_TRANSFER};
use crate::*;

const MAX_ACCOUNT_LENGTH: u128 = 64;
const MIN_ACCOUNT_DEPOSIT_LENGTH: u128 = MAX_ACCOUNT_LENGTH + 16 + 4;

/// Account deposits information and storage cost.
#[derive(BorshSerialize, BorshDeserialize, Default, Clone)]
pub struct AccountDeposit {
    /// Native amount sent to the exchange.
    /// Used for storage now, but in future can be used for trading as well.
    pub amount: Balance,
    /// Amounts of various tokens in this account.
    pub tokens: HashMap<AccountId, Balance>,
}

impl AccountDeposit {
    /// Adds amount to the balance of given token while checking that storage is covered.
    pub fn add(&mut self, token: AccountId, amount: Balance) {
        let prev_amount = *self.tokens.get(&token).unwrap_or(&0);
        self.tokens.insert(token, prev_amount + amount);
        assert!(
            self.storage_usage() <= self.amount,
            "ERR_INSUFFICIENT_STORAGE"
        );
    }

    /// Subtract from balance of given token.
    pub fn sub(&mut self, token: AccountId, amount: Balance) {
        let value = *self
            .tokens
            .get(&token)
            .expect(&format!("ERR_MISSING_TOKEN:{}", token));
        assert!(value >= amount, format!("ERR_NOT_ENOUGH_TOKEN:{}", token));
        self.tokens.insert(token, value - amount);
    }

    /// Returns amount of $NEAR to cover storage used by this data structure.
    pub fn storage_usage(&self) -> Balance {
        (MIN_ACCOUNT_DEPOSIT_LENGTH + self.tokens.len() as u128 * (MAX_ACCOUNT_LENGTH + 16))
            * env::storage_byte_cost()
    }

    /// Returns how much NEAR is available for storage.
    pub fn storage_available(&self) -> Balance {
        self.amount - self.storage_usage()
    }

    /// Returns minimal account deposit storage usage possible.
    pub fn min_storage_usage() -> Balance {
        MIN_ACCOUNT_DEPOSIT_LENGTH * env::storage_byte_cost()
    }

    /// Registers given token and set balance to 0.
    /// Fails if not enough amount to cover new storage usage.
    pub fn register(&mut self, token_id: &AccountId) {
        self.tokens.insert(token_id.clone(), 0);
        assert!(
            self.storage_usage() <= self.amount,
            "ERR_INSUFFICIENT_STORAGE"
        );
    }

    /// Unregisters given token from this users balance. Must be 0 balance.
    pub fn unregister(&mut self, token_id: &AccountId) {
        let amount = self.tokens.remove(token_id).unwrap_or_default();
        assert_eq!(amount, 0, "ERR_UNREGISTER_NON_ZERO_BALANCE");
    }
}

#[near_bindgen]
impl Contract {
    /// Registers given token in the user's account deposit.
    /// Fails if not enough balance on this account to cover storage.
    pub fn register_tokens(&mut self, token_ids: Vec<ValidAccountId>) {
        let sender_id = env::predecessor_account_id();
        let mut deposits = self
            .deposited_amounts
            .get(&sender_id)
            .expect(ERR_NOT_REGISTERED);
        for token_id in token_ids {
            deposits.register(token_id.as_ref());
        }
        self.deposited_amounts.insert(&sender_id, &deposits);
    }

    /// Unregister given token from user's account deposit.
    /// Fails if the balance of any given token is non 0.
    pub fn unregister_tokens(&mut self, token_ids: Vec<ValidAccountId>) {
        let sender_id = env::predecessor_account_id();
        let mut deposits = self
            .deposited_amounts
            .get(&sender_id)
            .expect(ERR_NOT_REGISTERED);
        for token_id in token_ids {
            deposits.unregister(token_id.as_ref());
        }
        self.deposited_amounts.insert(&sender_id, &deposits);
    }

    /// Withdraws given token from the deposits of given user.
    /// Optional unregister will try to remove record of this token from AccountDeposit for given user.
    /// Unregister will fail if the left over balance is non 0.
    #[payable]
    pub fn withdraw(&mut self, token_id: ValidAccountId, amount: U128, unregister: Option<bool>) {
        assert_one_yocto();
        let amount: u128 = amount.into();
        let sender_id = env::predecessor_account_id();
        let mut deposits = self
            .deposited_amounts
            .get(&sender_id)
            .expect(ERR_NOT_REGISTERED);
        deposits.sub(token_id.as_ref().clone(), amount);
        if unregister == Some(true) {
            deposits.unregister(token_id.as_ref());
        }
        self.deposited_amounts.insert(&sender_id, &deposits);
        ext_fungible_token::ft_transfer(
            sender_id.try_into().unwrap(),
            amount.into(),
            None,
            token_id.as_ref(),
            1,
            GAS_FOR_FT_TRANSFER,
        );
    }
}

impl Contract {
    /// Registers account in deposited amounts with given amount of $NEAR.
    /// If account already exists, adds amount to it.
    /// This should be used when it's known that storage is prepaid.
    pub(crate) fn internal_register_account(&mut self, account_id: &AccountId, amount: Balance) {
        let mut deposit_amount = self.deposited_amounts.get(&account_id).unwrap_or_default();
        deposit_amount.amount += amount;
        self.deposited_amounts.insert(&account_id, &deposit_amount);
    }

    /// Record deposit of some number of tokens to this contract.
    /// Fails if account is not registered or if token isn't whitelisted.
    pub(crate) fn internal_deposit(
        &mut self,
        sender_id: &AccountId,
        token_id: &AccountId,
        amount: Balance,
    ) {
        let mut account_deposit = self
            .deposited_amounts
            .get(sender_id)
            .expect(ERR_NOT_REGISTERED);
        assert!(
            self.whitelisted_tokens.contains(token_id)
                || account_deposit.tokens.contains_key(token_id),
            "ERR_TOKEN_NOT_WHITELISTED"
        );
        account_deposit.add(token_id.clone(), amount);
        self.deposited_amounts.insert(sender_id, &account_deposit);
    }

    /// Returns current balance of given token for given user. If there is nothing recorded, returns 0.
    pub(crate) fn internal_get_deposit(
        &self,
        sender_id: &AccountId,
        token_id: &AccountId,
    ) -> Balance {
        self.deposited_amounts
            .get(sender_id)
            .and_then(|d| d.tokens.get(token_id).cloned())
            .unwrap_or_default()
    }
}
