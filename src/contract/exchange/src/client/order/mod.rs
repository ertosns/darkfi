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

use darkfi::{zk::ProvingKey, zkas::ZkBinary, ClientFailed, Result};
use darkfi_sdk::{
    crypto::{pasta_prelude::*, Blind, FuncId, Keypair, PublicKey},
    pasta::pallas,
};
use log::debug;
use rand::rngs::OsRng;

pub use darkfi_money_contract::{
    client::{
        transfer_v1::{
            TransferCallBuilder, TransferCallClearInput, TransferCallInput, TransferCallOutput,
            TransferCallSecrets,
        },
        OwnCoin,
    },
    error::MoneyError,
    model::{MoneyTransferParamsV1, TokenId},
};

use crate::{error::OrderError, model::OrderMatchParams, MINIMAL_TIMEOUT_DURATION};

mod builder;
pub use builder::{OrderCallBuilder, OrderCallInput, OrderCallOutput, OrderCallSecrets};

pub(crate) mod proof;

/// Make an anonymous order call.
///
/// * `keypair`: Caller's keypair
/// * `withdraw`: Withdraw's public key
/// * `base_value`: Base amount that we ask.
/// * `quote_value`: Quote amount that we bid.
/// * `base_token_id`: Token ID of the ask value.
/// * `quote_token_id`: Token ID of the bid value.
/// * `timeout_duration`: timeout period during which the order can be executed, if set to 0, then no timeout limit.
/// * `transfer_call_secrets`: previous transfer call secret values.
/// * `transfer_inputs`: previous transfer call input values
/// * `transfer_outputs`: previous transfer call output values
/// * `output_spend_hook: Optional contract spend hook to use in
///   the output, not applicable to the change
/// * `output_user_data: Optional user data to use in the output,
///   not applicable to the change
/// * `order_zkbin`: `Order` zkas circuit ZkBinary
/// * `order_pk`: Proving key for the `Order` zk circuit
///
/// Returns a tuple of:
///
/// * The actual call data
/// * Secret values such as blinds
#[allow(clippy::too_many_arguments)]
pub fn make_order_call(
    _keypair: Keypair,
    withdraw_key: PublicKey,
    base_value: u64,
    quote_value: u64,
    base_token_id: TokenId,
    quote_token_id: TokenId,
    timeout_duration: u64,
    transfer_call_secrets: TransferCallSecrets,
    transfer_inputs: Vec<darkfi_money_contract::model::Input>,
    transfer_outputs: Vec<darkfi_money_contract::model::Output>,
    output_spend_hook: FuncId,
    output_user_data: pallas::Base,
    order_zkbin: ZkBinary,
    order_pk: ProvingKey,
) -> Result<(OrderMatchParams, OrderCallSecrets)> {
    //TODO use keypair for returning funds in case of change.
    debug!(target: "contract::exchange::client::order", "Building Exchange::OrderMatch contract call");
    if base_value == 0 {
        return Err(ClientFailed::InvalidAmount(base_value).into())
    }

    if quote_value == 0 {
        return Err(ClientFailed::InvalidAmount(quote_value).into())
    }

    if base_token_id.inner() == pallas::Base::ZERO {
        return Err(ClientFailed::InvalidTokenId(base_token_id.to_string()).into())
    }

    if quote_token_id.inner() == pallas::Base::ZERO {
        return Err(ClientFailed::InvalidTokenId(quote_token_id.to_string()).into())
    }

    if transfer_outputs.is_empty() {
        return Err(ClientFailed::VerifyError(OrderError::OrderMissingInputs.to_string()).into())
    }

    if timeout_duration < MINIMAL_TIMEOUT_DURATION {
        return Err(ClientFailed::VerifyError(OrderError::ShortTimeoutDuration.to_string()).into())
    }

    let mut inputs = vec![];
    let mut outputs = vec![];
    inputs.push(OrderCallInput {
        transfer_secrets: transfer_call_secrets,
        transfer_inputs: transfer_inputs.clone(),
        transfer_outputs: transfer_outputs.clone(),
    });

    outputs.push(OrderCallOutput {
        withdraw_key,
        base_value,
        quote_value,
        base_token_id,
        quote_token_id,
        timeout_duration,
        spend_hook: output_spend_hook,
        user_data: output_user_data,
        bulla_blind: Blind::random(&mut OsRng),
    });

    let order_builder = OrderCallBuilder { inputs, outputs, order_zkbin, order_pk };

    //TODO keypair will be reused for change coin.
    let (params, secrets) = order_builder.build()?;

    Ok((params, secrets))
}
