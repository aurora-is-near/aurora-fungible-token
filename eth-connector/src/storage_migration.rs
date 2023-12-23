use near_contract_standards::fungible_token::{metadata::FungibleTokenMetadata, FungibleToken};
use near_sdk::{
    borsh::{self, BorshDeserialize, BorshSerialize},
    collections::{LazyOption, LookupSet, UnorderedMap},
    AccountId,
};

use crate::{connector_impl::EthConnector, fee::FeeStorage, EthConnectorContract, StorageKey};

#[derive(BorshDeserialize, BorshSerialize)]
pub struct EthConnectorContractV0 {
    pub connector: EthConnector,
    pub ft: FungibleToken,
    pub metadata: LazyOption<FungibleTokenMetadata>,
    pub used_proofs: LookupSet<String>,
    pub known_engine_accounts: LookupSet<AccountId>,
}

impl From<EthConnectorContractV0> for EthConnectorContract {
    fn from(val: EthConnectorContractV0) -> Self {
        Self {
            connector: val.connector,
            ft: val.ft,
            metadata: val.metadata,
            used_proofs: val.used_proofs,
            known_engine_accounts: val.known_engine_accounts,
            fee: FeeStorage {
                deposit_fee_per_silo: UnorderedMap::new(StorageKey::DespositFeePerSilo),
                withdraw_fee_per_silo: UnorderedMap::new(StorageKey::WithdrawFeePerSilo),
                fee_owner: None,
            },
        }
    }
}
