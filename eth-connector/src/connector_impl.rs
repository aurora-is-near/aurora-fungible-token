use crate::{
    admin_controlled::PAUSE_DEPOSIT,
    connector::{ext_funds_finish, ext_proof_verifier},
    deposit_event::{DepositedEvent, TokenMessageData},
    errors, log, panic_err,
    proof::Proof,
    types::SdkUnwrap,
    AdminControlled, PausedMask,
};
use aurora_engine_types::types::Address;
use near_sdk::{
    borsh::{self, BorshDeserialize, BorshSerialize},
    env, AccountId, Balance, Gas, Promise,
};

/// NEAR Gas for calling `finish_deposit` promise. Used in the `deposit` logic.
pub const GAS_FOR_FINISH_DEPOSIT: Gas = Gas(50 * Gas::ONE_TERA.0);
/// NEAR Gas for calling `verify_log_entry` promise. Used in the `deposit` logic.
// Note: Is 40 TGas always enough?
const GAS_FOR_VERIFY_LOG_ENTRY: Gas = Gas(40 * Gas::ONE_TERA.0);

/// transfer eth-connector call args
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, PartialEq, Eq)]
pub struct TransferCallCallArgs {
    pub receiver_id: AccountId,
    pub amount: Balance,
    pub memo: Option<String>,
    pub msg: String,
}

/// Finish deposit NEAR eth-connector call args
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, PartialEq, Eq)]
pub struct FinishDepositCallArgs {
    pub new_owner_id: AccountId,
    pub amount: Balance,
    pub proof_key: String,
    pub msg: Option<Vec<u8>>,
}

/// withdraw result for eth-connector
#[derive(BorshSerialize, BorshDeserialize, Debug, PartialEq, Eq)]
pub struct WithdrawResult {
    pub amount: Balance,
    pub recipient_id: Address,
    pub eth_custodian_address: Address,
}

/// Connector specific data. It always should contain `prover account` -
#[derive(BorshSerialize, BorshDeserialize)]
pub struct EthConnector {
    /// The account is used in the Deposit flow to verify the incoming proof's log entry.
    pub prover_account: AccountId,
    /// The ETH address is used in the Deposit and Withdraw logic.
    pub eth_custodian_address: Address,
    /// Admin controlled.
    pub paused_mask: PausedMask,
    /// Account with access right for the current contract.
    pub account_with_access_right: AccountId,
    /// Owner's account id.
    pub owner_id: AccountId,
}

impl AdminControlled for EthConnector {
    fn get_paused_flags(&self) -> PausedMask {
        self.paused_mask
    }

    fn set_paused_flags(&mut self, paused: PausedMask) {
        self.paused_mask = paused;
    }

    fn set_access_right(&mut self, account: &AccountId) {
        self.account_with_access_right = account.clone();
    }

    fn get_account_with_access_right(&self) -> AccountId {
        self.account_with_access_right.clone()
    }

    fn is_owner(&self) -> bool {
        self.owner_id == env::predecessor_account_id()
            || env::current_account_id() == env::predecessor_account_id()
    }
}

impl EthConnector {
    pub(crate) fn deposit(&mut self, proof: Proof) -> Promise {
        let current_account_id = env::current_account_id();

        // Check is current flow paused. If it's owner account just skip it.
        self.assert_not_paused(PAUSE_DEPOSIT).sdk_unwrap();

        log!("[Deposit tokens]");

        // Fetch event data from Proof
        let event = DepositedEvent::from_log_entry_data(&proof.log_entry_data).sdk_unwrap();

        log!(
            "Deposit started: from {} to recipient {:?} with amount: {:?}",
            event.sender.encode(),
            event.token_message_data.get_recipient(),
            event.amount,
        );

        log!(
            "Event's address {}, custodian address {}",
            event.eth_custodian_address.encode(),
            self.eth_custodian_address.encode(),
        );

        if event.eth_custodian_address != self.eth_custodian_address {
            panic_err(error::FtDepositError::CustodianAddressMismatch);
        }

        // Verify proof data with cross-contract call to prover account
        log!(
            "Deposit verify_log_entry for prover: {}",
            self.prover_account,
        );

        // Finalize deposit
        let finish_deposit_data = match event.token_message_data {
            // Deposit to NEAR accounts
            TokenMessageData::Near(account_id) => FinishDepositCallArgs {
                new_owner_id: account_id,
                amount: event.amount,
                proof_key: proof.get_key(),
                msg: None,
            },
            // Deposit to Eth accounts
            TokenMessageData::Eth {
                mut receiver_id,
                message,
            } => {
                // The 'EthCustodian' contract produces events with the 'eth-connector'
                // account as the recipient when depositing to the EVM,
                // so here we override the receiver_id for backward compatibility.
                if receiver_id == current_account_id {
                    receiver_id = self.get_account_with_access_right();
                }
                // Transfer to self and then transfer ETH in `ft_on_transfer`
                // address - is NEAR account
                let transfer_data = TransferCallCallArgs {
                    receiver_id,
                    amount: event.amount,
                    memo: None,
                    msg: message.encode(),
                }
                .try_to_vec()
                .map_err(|_| errors::ERR_BORSH_SERIALIZE)
                .sdk_unwrap();

                // Send to self - current account id
                FinishDepositCallArgs {
                    new_owner_id: current_account_id.clone(),
                    amount: event.amount,
                    proof_key: proof.get_key(),
                    msg: Some(transfer_data),
                }
            }
        };

        ext_proof_verifier::ext(self.prover_account.clone())
            .with_static_gas(GAS_FOR_VERIFY_LOG_ENTRY)
            .verify_log_entry(proof.into())
            .then(
                ext_funds_finish::ext(current_account_id)
                    .with_static_gas(GAS_FOR_FINISH_DEPOSIT)
                    .finish_deposit(finish_deposit_data),
            )
    }
}

pub mod error {
    use crate::deposit_event::error::ParseOnTransferMessageError;
    use crate::errors::{
        ERR_BALANCE_OVERFLOW, ERR_NOT_ENOUGH_BALANCE, ERR_NOT_ENOUGH_BALANCE_FOR_FEE,
        ERR_PROOF_EXIST, ERR_SENDER_EQUALS_RECEIVER, ERR_TOTAL_SUPPLY_OVERFLOW,
        ERR_TOTAL_SUPPLY_UNDERFLOW, ERR_WRONG_EVENT_ADDRESS, ERR_ZERO_AMOUNT,
    };
    use aurora_engine_types::types::balance::error::BalanceOverflowError;
    use aurora_engine_types::types::ERR_FAILED_PARSE;

    pub struct ProofUsed;

    impl AsRef<[u8]> for ProofUsed {
        fn as_ref(&self) -> &[u8] {
            ERR_PROOF_EXIST
        }
    }

    #[derive(Debug)]
    pub enum DepositError {
        TotalSupplyOverflow,
        BalanceOverflow,
    }

    impl AsRef<[u8]> for DepositError {
        fn as_ref(&self) -> &[u8] {
            match self {
                Self::TotalSupplyOverflow => ERR_TOTAL_SUPPLY_OVERFLOW.as_bytes(),
                Self::BalanceOverflow => ERR_BALANCE_OVERFLOW,
            }
        }
    }

    impl From<DepositError> for TransferError {
        fn from(err: DepositError) -> Self {
            match err {
                DepositError::BalanceOverflow => Self::BalanceOverflow,
                DepositError::TotalSupplyOverflow => Self::TotalSupplyOverflow,
            }
        }
    }

    pub enum FtDepositError {
        ProofParseFailed,
        CustodianAddressMismatch,
        InsufficientAmountForFee,
    }

    impl AsRef<[u8]> for FtDepositError {
        fn as_ref(&self) -> &[u8] {
            match self {
                Self::ProofParseFailed => ERR_FAILED_PARSE.as_bytes(),
                Self::CustodianAddressMismatch => ERR_WRONG_EVENT_ADDRESS,
                Self::InsufficientAmountForFee => ERR_NOT_ENOUGH_BALANCE_FOR_FEE.as_bytes(),
            }
        }
    }

    #[derive(Debug)]
    pub enum WithdrawError {
        TotalSupplyUnderflow,
        InsufficientFunds,
        BalanceOverflow(BalanceOverflowError),
    }

    impl AsRef<[u8]> for WithdrawError {
        fn as_ref(&self) -> &[u8] {
            match self {
                Self::TotalSupplyUnderflow => ERR_TOTAL_SUPPLY_UNDERFLOW,
                Self::InsufficientFunds => ERR_NOT_ENOUGH_BALANCE,
                Self::BalanceOverflow(e) => e.as_ref(),
            }
        }
    }

    impl From<WithdrawError> for TransferError {
        fn from(err: WithdrawError) -> Self {
            match err {
                WithdrawError::InsufficientFunds => Self::InsufficientFunds,
                WithdrawError::TotalSupplyUnderflow => Self::TotalSupplyUnderflow,
                WithdrawError::BalanceOverflow(_) => Self::BalanceOverflow,
            }
        }
    }

    pub enum FinishDepositError {
        TransferCall(FtTransferCallError),
        ProofUsed,
    }

    impl From<DepositError> for FtTransferCallError {
        fn from(e: DepositError) -> Self {
            Self::Transfer(e.into())
        }
    }

    impl From<ParseOnTransferMessageError> for FtTransferCallError {
        fn from(e: ParseOnTransferMessageError) -> Self {
            Self::MessageParseFailed(e)
        }
    }

    impl AsRef<[u8]> for FinishDepositError {
        fn as_ref(&self) -> &[u8] {
            match self {
                Self::ProofUsed => ERR_PROOF_EXIST,
                Self::TransferCall(e) => e.as_ref(),
            }
        }
    }

    pub enum FtTransferCallError {
        BalanceOverflow(BalanceOverflowError),
        MessageParseFailed(ParseOnTransferMessageError),
        InsufficientAmountForFee,
        Transfer(TransferError),
    }

    impl AsRef<[u8]> for FtTransferCallError {
        fn as_ref(&self) -> &[u8] {
            match self {
                Self::MessageParseFailed(e) => e.as_ref(),
                Self::InsufficientAmountForFee => ERR_NOT_ENOUGH_BALANCE_FOR_FEE.as_bytes(),
                Self::Transfer(e) => e.as_ref(),
                Self::BalanceOverflow(e) => e.as_ref(),
            }
        }
    }

    #[derive(Debug)]
    pub enum TransferError {
        TotalSupplyUnderflow,
        TotalSupplyOverflow,
        InsufficientFunds,
        BalanceOverflow,
        ZeroAmount,
        SelfTransfer,
    }

    impl AsRef<[u8]> for TransferError {
        fn as_ref(&self) -> &[u8] {
            match self {
                Self::TotalSupplyUnderflow => ERR_TOTAL_SUPPLY_UNDERFLOW,
                Self::TotalSupplyOverflow => ERR_TOTAL_SUPPLY_OVERFLOW.as_bytes(),
                Self::InsufficientFunds => ERR_NOT_ENOUGH_BALANCE,
                Self::BalanceOverflow => ERR_BALANCE_OVERFLOW,
                Self::ZeroAmount => ERR_ZERO_AMOUNT,
                Self::SelfTransfer => ERR_SENDER_EQUALS_RECEIVER,
            }
        }
    }
}
