/* This file is part of DarkFi (https://dark.fi)
 *
 * Copyright (C) 2020-2025 Dyne.org foundation
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

use darkfi_sdk::error::ContractError;

#[derive(Debug, Clone, thiserror::Error)]
pub enum OrderError {
    #[error("Missing inputs in transfer call")]
    OrderMissingInputs,

    #[error("Missing outputs in transfer call")]
    OrderMissingOutputs,

    #[error("Clear input used non-native token")]
    OrderClearInputNonNativeToken,

    #[error("Clear input used unauthorised pubkey")]
    OrderClearInputUnauthorised,

    #[error("Merkle root not found in previous state")]
    OrderMerkleRootNotFound,

    #[error("Duplicate nullifier found")]
    DuplicateNullifier,

    #[error("Duplicate coin found")]
    DuplicateCoin,

    #[error("Value commitment mismatch")]
    ValueMismatch,

    #[error("Token commitment mismatch")]
    TokenMismatch,

    #[error("Invalid number of inputs")]
    InvalidNumberOfInputs,

    #[error("Invalid number of outputs")]
    InvalidNumberOfOutputs,

    #[error("Spend hook is not zero")]
    SpendHookNonZero,

    #[error("Merkle root not found in previous state")]
    SwapMerkleRootNotFound,

    #[error("Token ID does not derive from mint authority")]
    TokenIdDoesNotDeriveFromMint,

    #[error("Token mint is frozen")]
    TokenMintFrozen,

    #[error("Parent call function mismatch")]
    ParentCallFunctionMismatch,

    #[error("Parent call input mismatch")]
    ParentCallInputMismatch,

    #[error("Child call function mismatch")]
    ChildCallFunctionMismatch,

    #[error("Child call input mismatch")]
    ChildCallInputMismatch,

    #[error("Call is not executed on genesis block")]
    GenesisCallNonGenesisBlock,

    #[error("Missing nullifier in set")]
    MissingNullifier,

    #[error("No inputs in fee call")]
    FeeMissingInputs,

    #[error("Insufficient fee paid")]
    InsufficientFee,

    #[error("Coin merkle root not found")]
    CoinMerkleRootNotFound,

    #[error("Roots value data length missmatch")]
    RootsValueDataMismatch,

    #[error("Children indexes length missmatch")]
    ChildrenIndexesLengthMismatch,

    #[error("Short timeout duration")]
    ShortTimeoutDuration,
}

impl From<OrderError> for ContractError {
    fn from(e: OrderError) -> Self {
        match e {
            OrderError::OrderMissingInputs => Self::Custom(1),
            OrderError::OrderMissingOutputs => Self::Custom(2),
            OrderError::OrderClearInputNonNativeToken => Self::Custom(3),
            OrderError::OrderClearInputUnauthorised => Self::Custom(4),
            OrderError::OrderMerkleRootNotFound => Self::Custom(5),
            OrderError::DuplicateNullifier => Self::Custom(6),
            OrderError::DuplicateCoin => Self::Custom(7),
            OrderError::ValueMismatch => Self::Custom(8),
            OrderError::TokenMismatch => Self::Custom(9),
            OrderError::InvalidNumberOfInputs => Self::Custom(10),
            OrderError::InvalidNumberOfOutputs => Self::Custom(11),
            OrderError::SpendHookNonZero => Self::Custom(12),
            OrderError::SwapMerkleRootNotFound => Self::Custom(13),
            OrderError::TokenIdDoesNotDeriveFromMint => Self::Custom(14),
            OrderError::TokenMintFrozen => Self::Custom(15),
            OrderError::ParentCallFunctionMismatch => Self::Custom(16),
            OrderError::ParentCallInputMismatch => Self::Custom(17),
            OrderError::ChildCallFunctionMismatch => Self::Custom(18),
            OrderError::ChildCallInputMismatch => Self::Custom(19),
            OrderError::GenesisCallNonGenesisBlock => Self::Custom(20),
            OrderError::MissingNullifier => Self::Custom(21),
            OrderError::FeeMissingInputs => Self::Custom(22),
            OrderError::InsufficientFee => Self::Custom(23),
            OrderError::CoinMerkleRootNotFound => Self::Custom(24),
            OrderError::RootsValueDataMismatch => Self::Custom(25),
            OrderError::ChildrenIndexesLengthMismatch => Self::Custom(26),
            OrderError::ShortTimeoutDuration => Self::Custom(27),
        }
    }
}
