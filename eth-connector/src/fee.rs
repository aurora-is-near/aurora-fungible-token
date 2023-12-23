use near_sdk::{
    borsh::{self, BorshDeserialize, BorshSerialize},
    collections::UnorderedMap,
    json_types::U128,
    serde::{Deserialize, Serialize},
    AccountId,
};

#[derive(Copy, Debug, Serialize, Deserialize, Clone)]
pub enum FeeType {
    Deposit,
    Withdraw,
}

#[derive(BorshDeserialize, BorshSerialize, Debug, Clone, Serialize, Deserialize, Copy)]
pub struct Fee {
    /// Fee percentage in 6 decimal precision (10% -> 0.1 * 10e6 -> 100_000)
    pub fee_percentage: u32,
    /// The minimum fee amount for transfer
    pub lower_bound: Option<U128>,
    /// The maximum fee amount for transfer
    pub upper_bound: Option<U128>,
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct FeeStorage {
    /// Store the fee for deposit transfers that target silos.
    /// The `None` is used to store the default fee.
    pub deposit_fee_per_silo: UnorderedMap<Option<AccountId>, Fee>,
    /// Store the fee for withdrawal transfers initiated by silos.
    /// The `None` is used to store the default fee.
    pub withdraw_fee_per_silo: UnorderedMap<Option<AccountId>, Fee>,
    /// If set, then fee minted to the `fee_owner` account
    pub fee_owner: Option<AccountId>,
}
