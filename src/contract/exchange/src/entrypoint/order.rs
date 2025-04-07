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
        pasta_prelude::*, ContractId, FuncId, FuncRef, MerkleNode, PublicKey, EXCHANGE_CONTRACT_ID,
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
    ExchangeFunction, EXCHANGE_CONTRACT_COINS_TREE, EXCHANGE_CONTRACT_COIN_MERKLE_TREE,
    EXCHANGE_CONTRACT_COIN_ROOTS_TREE, EXCHANGE_CONTRACT_INFO_TREE,
    EXCHANGE_CONTRACT_LATEST_COIN_ROOT, EXCHANGE_CONTRACT_LATEST_NULLIFIER_ROOT,
    EXCHANGE_CONTRACT_NULLIFIERS_TREE, EXCHANGE_CONTRACT_NULLIFIER_ROOTS_TREE,
    EXCHANGE_CONTRACT_ORDER_MATCH_TREE, EXCHANGE_CONTRACT_ZKAS_ORDER_MATCH,
};

use darkfi_money_contract::{MONEY_CONTRACT_ZKAS_BURN_NS_V1, MONEY_CONTRACT_ZKAS_MINT_NS_V1};

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
    let mut signature_pubkeys: Vec<PublicKey> = vec![];
    // Calculate the spend hook
    let spend_hook = match calls[call_idx].parent_index {
        Some(parent_idx) => {
            let parent_call = &calls[parent_idx].data;
            let contract_id = parent_call.contract_id;
            let func_code = parent_call.data[0];

            FuncRef { contract_id, func_code }.to_func_id()
        }
        None => FuncId::none(),
    };

    // Grab Transfer call's Burn, Mint proof commitments from the inputs
    for input in &params.inputs {
        for transfer_input in &input.transfer_inputs {
            let burn_value_coords = transfer_input.value_commit.to_affine().coordinates().unwrap();
            let (sig_x, sig_y) = transfer_input.signature_public.xy();

            zk_public_inputs.push((
                MONEY_CONTRACT_ZKAS_BURN_NS_V1.to_string(),
                vec![
                    transfer_input.nullifier.inner(),
                    *burn_value_coords.x(),
                    *burn_value_coords.y(),
                    transfer_input.token_commit,
                    transfer_input.merkle_root.inner(),
                    transfer_input.user_data_enc,
                    spend_hook.inner(),
                    sig_x,
                    sig_y,
                ],
            ));
            signature_pubkeys.push(transfer_input.signature_public);
        }
        for transfer_output in &input.transfer_outputs {
            let value_coords = transfer_output.value_commit.to_affine().coordinates().unwrap();

            zk_public_inputs.push((
                MONEY_CONTRACT_ZKAS_MINT_NS_V1.to_string(),
                vec![
                    transfer_output.coin.inner(),
                    *value_coords.x(),
                    *value_coords.y(),
                    transfer_output.token_commit,
                ],
            ));
        }
    }

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
    cid: ContractId,
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

    // Access the necessary databases where there is information to
    // validate this state transition.
    let coins_db = wasm::db::db_lookup(cid, EXCHANGE_CONTRACT_COINS_TREE)?;

    // Accumulator for the value commitments. We add inputs to it, and subtract
    // outputs from it. For the commitments to be valid, the accumulator must
    // be in its initial state after performing the arithmetics.
    let mut valcom_total = pallas::Point::identity();

    // Fees can only be paid using the native token, so we'll compare
    // the token commitments with this one:
    let mut new_coins = Vec::with_capacity(params.outputs.len());
    let mut new_nullifiers = Vec::with_capacity(params.inputs.len());

    //TODO make sure timeout duration didn't pass out.
    msg!("[Order] Iterating over anonymous inputs");
    for (i, inputs) in params.inputs.iter().enumerate() {
        for input in &inputs.transfer_inputs {
            //NOTE verification of valid merkle_root for coins, nullifiers happen using
            // money contract id, can't be checked here.

            //TODO when the withdraw contract is implement make sure that the
            // liquidity in exchange aren't spent/withdrawn.

            // Verify the token commitment is the expected one
            for output in &params.outputs {
                if output.base_token_commit != input.token_commit {
                    msg!("[Order] Error: Token commitment mismatch in input {}", i);
                    return Err(OrderError::TokenMismatch.into())
                }
            }

            // Append this new nullifier to seen nullifiers, and accumulate the value commitment
            new_nullifiers.push(input.nullifier);
            valcom_total += input.value_commit;
        }

        for output in &inputs.transfer_outputs {
            if new_coins.contains(&output.coin) ||
                wasm::db::db_contains_key(coins_db, &serialize(&output.coin))?
            {
                msg!("[Order] Error: Duplicate coin found in output {}", i);
                return Err(OrderError::DuplicateCoin.into())
            }

            // Append this new coin to seen coins, and subtract the value commitment
            new_coins.push(output.coin);
        }
    }
    msg!("[Order] Iterating over anonymous outputs");
    for output in params.outputs.iter() {
        valcom_total -= output.base_value_commit;
    }

    //TODO add `OrderAttributes` to  `OrderMatchParams` outputs,
    // and  validate orders aren't duplicates

    let update = OrderMatchUpdate { nullifiers: new_nullifiers, coins: new_coins, orders: vec![] };
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
    // Grab all necessary db handles for where we want to write
    let info_db = wasm::db::db_lookup(cid, EXCHANGE_CONTRACT_INFO_TREE)?;
    let coins_db = wasm::db::db_lookup(cid, EXCHANGE_CONTRACT_COINS_TREE)?;
    let nullifiers_db = wasm::db::db_lookup(cid, EXCHANGE_CONTRACT_NULLIFIERS_TREE)?;
    let coin_roots_db =
        wasm::db::db_lookup(*EXCHANGE_CONTRACT_ID, EXCHANGE_CONTRACT_COIN_ROOTS_TREE)?;
    let nullifier_roots_db = wasm::db::db_lookup(cid, EXCHANGE_CONTRACT_NULLIFIER_ROOTS_TREE)?;

    let orders_db = wasm::db::db_lookup(cid, EXCHANGE_CONTRACT_ORDER_MATCH_TREE)?;

    for order in &update.orders {
        wasm::db::db_set(orders_db, &serialize(order), &[])?;
    }

    wasm::merkle::sparse_merkle_insert_batch(
        info_db,
        nullifiers_db,
        nullifier_roots_db,
        EXCHANGE_CONTRACT_LATEST_NULLIFIER_ROOT,
        &update.nullifiers.iter().map(|n| n.inner()).collect::<Vec<_>>(),
    )?;

    let mut new_coins = Vec::with_capacity(update.coins.len());

    for coin in &update.coins {
        wasm::db::db_set(coins_db, &serialize(coin), &[])?;
        new_coins.push(MerkleNode::from(coin.inner()));
    }

    wasm::merkle::merkle_add(
        info_db,
        coin_roots_db,
        EXCHANGE_CONTRACT_LATEST_COIN_ROOT,
        EXCHANGE_CONTRACT_COIN_MERKLE_TREE,
        &new_coins,
    )?;

    Ok(())
}
