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

//! Contract Client API
//!
//! This module implements the client-side API for this contract's interaction.
//! What we basically do here is implement an API that creates the necessary
//! structures and is able to export them to create a DarkFi transaction
//! object that can be broadcasted to the network when we want to make a
//! payment with some coins in our wallet.
//!
//! Note that this API does not involve any wallet interaction, but only takes
//! the necessary objects provided by the caller. This is intentional, so we
//! are able to abstract away any wallet interfaces to client implementations.

use darkfi_sdk::{
    crypto::{BaseBlind, FuncId, ScalarBlind},
    pasta::pallas,
};
use darkfi_serial::{async_trait, SerialDecodable, SerialEncodable};

use darkfi_money_contract::model::TokenId;

pub mod order;

/// `OrderNote` holds the inner attributes of a `order`.
#[derive(Debug, Clone, Eq, PartialEq, SerialEncodable, SerialDecodable)]
pub struct OrderNote {
    /// Base value of the order
    pub base_value: u64,
    /// Quote value of the order
    pub quote_value: u64,
    /// Base token ID of the order
    pub base_token_id: TokenId,
    /// Quote token ID of the order
    pub quote_token_id: TokenId,
    /// Timeout duration for order execution
    pub timeout_duration: u64,
    /// Spend hook used for protocol-owned liquidity.
    /// Specifies which contract owns this coin.
    pub spend_hook: FuncId,
    /// User data used by protocol when spend hook is enabled
    pub user_data: pallas::Base,
    /// Blinding factor for the order bulla
    pub bulla_blind: BaseBlind,
    /// Base value blinding factor for the base value commit
    pub base_value_blind: ScalarBlind,
    /// Quote value blinding factor for the quote value commit
    pub quote_value_blind: ScalarBlind,
    /// Base token id blinding factor for base token commit
    pub base_token_id_blind: BaseBlind,
    /// Quote token id blinding factor for quote token commit
    pub quote_token_id_blind: BaseBlind,
    /// Timeout duration blinding factor for timeout duration commit
    pub timeout_duration_blind: ScalarBlind,
    /// Attached memo (arbitrary data)
    pub memo: Vec<u8>,
}
