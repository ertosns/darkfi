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

use darkfi_sdk::{
    crypto::{
        pasta_prelude::*, ContractId, PublicKey,
    },
    dark_tree::DarkLeaf,
    error::{ContractError, ContractResult},
    msg,
    pasta::pallas,
    wasm, ContractCall,
};
use darkfi_serial::{deserialize, serialize, Encodable, WriteExt};

use crate::{
    error::OrderError,
    model::{OrderMatchParams, OrderMatchUpdate},
    ExchangeFunction, EXCHANGE_CONTRACT_ORDER_MATCH_TREE, EXCHANGE_CONTRACT_ZKAS_ORDER_MATCH,
};


/// `get_metadata` function for `Exchange::OrderMatch`
pub(crate) fn exchange_order_get_metadata(
    _cid: ContractId,
    call_idx: usize,
    calls: Vec<DarkLeaf<ContractCall>>,
) -> Result<Vec<u8>, ContractError> {
    let self_ = &calls[call_idx].data;
    let params: OrderMatchParams = deserialize(&self_.data[1..])?;
    // Public inputs for the ZK proofs we have to verify
    let mut zk_public_inputs: Vec<(String, Vec<pallas::Base>)> = vec![];
    // Public keys for the transaction signatures we have to verify
    let signature_pubkeys: Vec<PublicKey> = vec![params.inputs[0].transfer_inputs[0].signature_public];
    // Grab the pedersen commitments from the outputs
    for output in params.outputs {
        let base_output_value_coords = output.base_value_commit.to_affine().coordinates().unwrap();
        let quote_output_value_coords =
            output.quote_value_commit.to_affine().coordinates().unwrap();
        let timeout_duration_coords =
            output.timeout_duration_commit.to_affine().coordinates().unwrap();
        zk_public_inputs.push((
            EXCHANGE_CONTRACT_ZKAS_ORDER_MATCH.to_string(),
            vec![
                output.order_bulla,
                *base_output_value_coords.x(),
                *base_output_value_coords.y(),
                *quote_output_value_coords.x(),
                *quote_output_value_coords.y(),
                output.base_token_commit,
                output.quote_token_commit,
                *timeout_duration_coords.x(),
                *timeout_duration_coords.y(),
            ],
        ));
    }

    // Serialize everything gathered and return it
    let mut metadata = vec![];
    zk_public_inputs.encode(&mut metadata)?;
    signature_pubkeys.encode(&mut metadata)?;

    Ok(metadata)
}

/// `process_instruction` function for `Exchange::OrderMatch`
pub(crate) fn exchange_order_process_instruction(
    _cid: ContractId,
    call_idx: usize,
    calls: Vec<DarkLeaf<ContractCall>>,
) -> Result<Vec<u8>, ContractError> {
    let self_ = &calls[call_idx];
    let params: OrderMatchParams = deserialize(&self_.data.data[1..])?;

    if params.inputs.is_empty() {
        msg!("[Order] Error: No inputs in the call");
        return Err(OrderError::OrderMissingInputs.into())
    }

    if params.outputs.is_empty() {
        msg!("[Order] Error: No outputs in the call");
        return Err(OrderError::OrderMissingOutputs.into())
    }

    //TODO make sure timeout duration didn't pass out.
    //TODO add `OrderAttributes` to  `OrderMatchParams` outputs,
    // and  validate orders aren't duplicates

    let update = OrderMatchUpdate {orders: vec![] };
    let mut update_data = vec![];
    update_data.write_u8(ExchangeFunction::OrderMatch as u8)?;
    update.encode(&mut update_data)?;
    // and return it
    Ok(update_data)
}

/// `process_update` function for `Exchange::OrderMatch`
pub(crate) fn exchange_order_process_update(
    cid: ContractId,
    update: OrderMatchUpdate,
) -> ContractResult {
    let orders_db = wasm::db::db_lookup(cid, EXCHANGE_CONTRACT_ORDER_MATCH_TREE)?;

    for order in &update.orders {
        wasm::db::db_set(orders_db, &serialize(order), &[])?;
    }
    Ok(())
}
