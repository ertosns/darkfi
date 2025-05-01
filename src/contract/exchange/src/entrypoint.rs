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
    crypto::{ContractId},
    dark_tree::DarkLeaf,
    error::ContractResult,
    msg,
    wasm, ContractCall,
};
use darkfi_serial::{deserialize, serialize, Encodable};

use crate::{
    error::OrderError, model::OrderMatchUpdate, ExchangeFunction,
    EXCHANGE_CONTRACT_ORDER_MATCH_TREE
};

mod order;
use order::{
    exchange_order_get_metadata, exchange_order_process_instruction, exchange_order_process_update,
};

darkfi_sdk::define_contract!(
    init: init_contract,
    exec: process_instruction,
    apply: process_update,
    metadata: get_metadata
);

/// This entrypoint function runs when the contract is (re)deployed and initialized.
/// We use this function to initialize all the necessary databases and prepare them
/// with initial data if necessary. This is also the place where we bundle the zkas
/// circuits that are to be used with functions provided by the contract.
fn init_contract(cid: ContractId, _ix: &[u8]) -> ContractResult {
    // zkas circuits can simply be embedded in the wasm and set up by using
    // respective db functions. The special `zkas db` operations exist in
    // order to be able to verify the circuits being bundled and enforcing
    // a specific tree inside sled, and also creation of VerifyingKey.
    let order_bincode = include_bytes!("../proof/order.zk.bin");

    // For that, we use `wasm::db::zkas_wasm::db::db_set` and pass in the bincode.
    wasm::db::zkas_db_set(&order_bincode[..])?;


    let tx_hash = wasm::util::get_tx_hash()?;
    // The max outputs for a tx in BTC is 2501
    let call_idx = wasm::util::get_call_index()?;
    let mut roots_value_data = Vec::with_capacity(32 + 1);
    tx_hash.encode(&mut roots_value_data)?;
    call_idx.encode(&mut roots_value_data)?;
    if roots_value_data.len() != 32 + 1 {
        msg!(
            "[exchange::init_contract] Error: Roots value data length is not expected(32 + 1): {}",
            roots_value_data.len()
        );
        return Err(OrderError::RootsValueDataMismatch.into())
    }

    // Set up a database tree to hold the fees paid for each block
    if wasm::db::db_lookup(cid, EXCHANGE_CONTRACT_ORDER_MATCH_TREE).is_err() {
        let fees_db = wasm::db::db_init(cid, EXCHANGE_CONTRACT_ORDER_MATCH_TREE)?;
        // Initialize the first two accumulators
        wasm::db::db_set(fees_db, &serialize(&0_u32), &serialize(&0_u64))?;
        wasm::db::db_set(fees_db, &serialize(&1_u32), &serialize(&0_u64))?;
    }

    Ok(())
}

/// This function is used by the wasm VM's host to fetch the necessary metadata
/// for verifying signatures and zk proofs. The payload given here are all the
/// contract calls in the transaction.
fn get_metadata(cid: ContractId, ix: &[u8]) -> ContractResult {
    let call_idx = wasm::util::get_call_index()? as usize;
    let calls: Vec<DarkLeaf<ContractCall>> = deserialize(ix)?;
    let self_ = &calls[call_idx].data;
    let func = ExchangeFunction::try_from(self_.data[0])?;

    let metadata = match func {
        ExchangeFunction::OrderMatch => exchange_order_get_metadata(cid, call_idx, calls)?,
    };

    wasm::util::set_return_data(&metadata)
}

/// This func-tion verifies a state transition and produces a state update
/// if everything is successful. This step should happen **after** the host
/// has successfully verified the metadata from `get_metadata()`.
fn process_instruction(cid: ContractId, ix: &[u8]) -> ContractResult {
    let call_idx = wasm::util::get_call_index()? as usize;
    let calls: Vec<DarkLeaf<ContractCall>> = deserialize(ix)?;
    let self_ = &calls[call_idx].data;
    let func = ExchangeFunction::try_from(self_.data[0])?;

    let update_data = match func {
        ExchangeFunction::OrderMatch => exchange_order_process_instruction(cid, call_idx, calls)?,
    };

    wasm::util::set_return_data(&update_data)
}

/// This function attempts to write a given state update provided the previous steps
/// of the contract call execution all were successful. It's the last in line, and
/// assumes that the transaction/call was successful. The payload given to the function
/// is the update data retrieved from `process_instruction()`.
fn process_update(cid: ContractId, update_data: &[u8]) -> ContractResult {
    match ExchangeFunction::try_from(update_data[0])? {
        ExchangeFunction::OrderMatch => {
            let update: OrderMatchUpdate = deserialize(&update_data[1..])?;
            Ok(exchange_order_process_update(cid, update)?)
        }
    }
}
