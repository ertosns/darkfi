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

use core::str::FromStr;

use darkfi_money_contract;
pub use darkfi_money_contract::model::{Coin, Nullifier, TokenId, DARK_TOKEN_ID};
use darkfi_sdk::{
    crypto::{
        note::AeadEncryptedNote, pasta_prelude::*, poseidon_hash, BaseBlind, FuncId, PublicKey,
    },
    error::ContractError,
    pasta::pallas,
};
use darkfi_serial::{SerialDecodable, SerialEncodable};

#[cfg(feature = "client")]
use darkfi_serial::async_trait;

/// A `OrderBulla` represented in the state
#[derive(Debug, Copy, Clone, Eq, PartialEq, SerialEncodable, SerialDecodable)]
pub struct OrderBulla(pub pallas::Base);

impl OrderBulla {
    /// Reference the raw inner base field element
    pub fn inner(&self) -> pallas::Base {
        self.0
    }

    /// Create a `OrderBulla` object from given bytes, erroring if the
    /// input bytes are noncanonical.
    pub fn from_bytes(x: [u8; 32]) -> Result<Self, ContractError> {
        match pallas::Base::from_repr(x).into() {
            Some(v) => Ok(Self(v)),
            None => Err(ContractError::IoError(
                "Failed to instantiate OrderBulla from bytes".to_string(),
            )),
        }
    }

    /// Convert the `OrderBulla` type into 32 raw bytes
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_repr()
    }
}

impl std::hash::Hash for OrderBulla {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        state.write(&self.to_bytes());
    }
}

darkfi_sdk::fp_from_bs58!(OrderBulla);
darkfi_sdk::fp_to_bs58!(OrderBulla);
darkfi_sdk::ty_from_fp!(OrderBulla);

#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
// ANCHOR: order-attributes
/// Order Attributes to mint a market order
pub struct OrderAttributes {
    /// Withdraw key for liquidity provider
    pub withdraw_key: PublicKey,
    /// Market order base value
    pub base_value: u64,
    /// Market order quote value
    pub quote_value: u64,
    /// Market order base token id
    pub base_token_id: TokenId,
    /// Market order quote token id
    pub quote_token_id: TokenId,
    /// Timeout duration during which market order is valid
    pub timeout_duration: u64,
    /// Contract call spend hook
    pub spend_hook: FuncId,
    /// Contract call spend hook's user data
    pub user_data: pallas::Base,
    /// Market order bulla blinding factor.
    pub bulla_blind: BaseBlind,
}

impl OrderAttributes {
    /// Convert `OrderAttributes` to a `OrderBulla`
    pub fn to_bulla(&self) -> OrderBulla {
        let (withdraw_x, withdraw_y) = self.withdraw_key.xy();
        let bulla = poseidon_hash([
            withdraw_x,
            withdraw_y,
            pallas::Base::from(self.base_value),
            pallas::Base::from(self.quote_value),
            self.base_token_id.inner(),
            self.quote_token_id.inner(),
            pallas::Base::from(self.timeout_duration),
            self.spend_hook.inner(),
            self.user_data,
            self.bulla_blind.inner(),
        ]);
        OrderBulla(bulla)
    }
}

#[derive(Clone, Debug, PartialEq, SerialEncodable, SerialDecodable)]
// ANCHOR: order-input
/// A contract call's anonymous input
pub struct Input {
    /// Transfer call spent coins's inputs
    pub transfer_inputs: Vec<darkfi_money_contract::model::Input>,
    /// Transfer call outputs
    pub transfer_outputs: Vec<darkfi_money_contract::model::Output>,
}

#[derive(Clone, Debug, PartialEq, SerialEncodable, SerialDecodable)]
// ANCHOR: money-output
/// A contract call's anonymous output
pub struct Output {
    /// commitment for the order bulla
    pub order_bulla: pallas::Base,
    /// commitment for the order base value
    pub base_value_commit: pallas::Point,
    /// commitment for the order quote value
    pub quote_value_commit: pallas::Point,
    /// Commitment for the order base token ID
    pub base_token_commit: pallas::Base,
    /// Commitment for the order quote token ID
    pub quote_token_commit: pallas::Base,
    /// Commitment for the timeout duration
    pub timeout_duration_commit: pallas::Point,
    /// AEAD encrypted note
    pub note: AeadEncryptedNote,
}

#[derive(Clone, Debug, SerialEncodable, SerialDecodable)]
pub struct OrderMatchUpdate {
    /// Minted orders
    pub orders: Vec<OrderAttributes>,
}

#[derive(Clone, Debug, SerialEncodable, SerialDecodable)]
// ANCHOR: match-params
/// Parameters for `Exchange::OrderMatch`
pub struct OrderMatchParams {
    /// Anonymous inputs
    pub inputs: Vec<Input>,
    /// Anonymous outputs
    pub outputs: Vec<Output>,
}
