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

use super::{Holder, TestHarness};
use darkfi::{
    tx::{ContractCallLeaf, Transaction, TransactionBuilder},
    Result,
};
use darkfi_exchange_contract::{
    client::order::make_order_call,
    model::{OrderBulla, OrderMatchParams},
    ExchangeFunction, EXCHANGE_CONTRACT_ZKAS_ORDER_MATCH,
};
use darkfi_money_contract::{
    client::{transfer_v1::make_transfer_call, MoneyNote, OwnCoin},
    model::{MoneyFeeParamsV1, MoneyTransferParamsV1, TokenId},
    MoneyFunction, MONEY_CONTRACT_ZKAS_BURN_NS_V1, MONEY_CONTRACT_ZKAS_MINT_NS_V1,
};
use darkfi_sdk::{
    crypto::{
        contract_id::{EXCHANGE_CONTRACT_ID, MONEY_CONTRACT_ID},
        poseidon_hash, FuncId, FuncRef, MerkleNode, PublicKey,
    },
    pasta::pallas,
    ContractCall, dark_tree::DarkTree,
};
use darkfi_serial::{async_trait, AsyncEncodable, SerialDecodable, SerialEncodable};
use log::debug;

impl TestHarness {
    /// Create a `Exchange::OrderMatch` transaction.
    #[allow(clippy::too_many_arguments)]
    pub async fn order_match(
        &mut self,
        base_amount: u64,
        quote_amount: u64,
        lp: &Holder,
        exchange: &Holder,
        owncoins: &[OwnCoin],
        base_token_id: TokenId,
        quote_token_id: TokenId,
        timeout_duration: u64,
        block_height: u32,
        spend_hook: FuncId,
        user_data: pallas::Base,
    ) -> Result<(
        Transaction,
        (MoneyTransferParamsV1, OrderMatchParams, MoneyFeeParamsV1),
        Vec<OwnCoin>,
    )> {
        let wallet = self.holders.get(lp).unwrap();
        let withdraw_keypair = wallet.keypair;
        let withdraw_public_key = withdraw_keypair.public;
        let rcpt = self.holders.get(exchange).unwrap().keypair.public;

        let (order_pk, order_zkbin) =
            self.proving_keys.get(EXCHANGE_CONTRACT_ZKAS_ORDER_MATCH).unwrap();
        let (mint_pk, mint_zkbin) = self.proving_keys.get(MONEY_CONTRACT_ZKAS_MINT_NS_V1).unwrap();
        let (burn_pk, burn_zkbin) = self.proving_keys.get(MONEY_CONTRACT_ZKAS_BURN_NS_V1).unwrap();
        for c in owncoins {
            assert!(c.note.token_id == base_token_id);
        }

        // Create the transfer call
        let (transfer_params, transfer_secrets, mut spent_coins) = make_transfer_call(
            withdraw_keypair,
            rcpt,
            base_amount,
            base_token_id,
            owncoins.to_owned(),
            wallet.money_merkle_tree.clone(),
            Some(spend_hook),
            Some(user_data),
            mint_zkbin.clone(),
            mint_pk.clone(),
            burn_zkbin.clone(),
            burn_pk.clone(),
            false,
        )?;
        // Encode the call
        let mut data = vec![MoneyFunction::TransferV1 as u8];
        transfer_params.encode_async(&mut data).await?;
        let call = ContractCall { contract_id: *MONEY_CONTRACT_ID, data: data.clone() };

        ///////////////////////////////
        // append make order call
        ///////////////////////////////

        //TODO replace charlie with Exchange's OrderBook.
        let (order_params, order_secrets) = make_order_call(
            withdraw_keypair,
            withdraw_public_key,
            base_amount,
            quote_amount,
            base_token_id,
            quote_token_id,
            timeout_duration,
            transfer_secrets.clone(),
            transfer_params.inputs.clone(),
            transfer_params.outputs.clone(),
            spend_hook,
            user_data,
            order_zkbin.clone(),
            order_pk.clone(),
        )?;

        // add order match call
        let mut order_match_data = vec![ExchangeFunction::OrderMatch as u8];
        order_params.encode_async(&mut order_match_data).await?;
        let order_match_call =
            ContractCall { contract_id: *EXCHANGE_CONTRACT_ID, data: order_match_data };
        let contract_call_leaf = ContractCallLeaf { call, proofs: transfer_secrets.clone().proofs };
        let dark_tree = DarkTree::new(contract_call_leaf, vec![], None, None);
        let mut tx_builder = TransactionBuilder::new(
            ContractCallLeaf { call: order_match_call, proofs: order_secrets.proofs },
            vec![dark_tree],
        )?;

        // Now build the actual transaction and sign it with all necessary keys.
        let mut tx = tx_builder.build()?;

        let transfer_sigs = tx.create_sigs(&transfer_secrets.signature_secrets)?;
        tx.signatures = vec![transfer_sigs];
        let order_sigs = tx.create_sigs(&order_secrets.signature_secrets)?;

        tx.signatures.push(order_sigs);
        assert!(tx.signatures.len() == 2);
        assert!(!spent_coins.is_empty());

        // which holder should be charged for fees?lp, or exchange.
        let (fee_call, fee_proofs, fee_secrets, spent_fee_coins, fee_call_params) =
            self.append_fee_call(lp, tx, block_height, &spent_coins).await?;
        // Append the fee call to the transaction
        tx_builder.append(ContractCallLeaf { call: fee_call, proofs: fee_proofs }, vec![])?;
        spent_coins.extend_from_slice(&spent_fee_coins);
        let mut tx = tx_builder.build()?;
        let transfer_sigs = tx.create_sigs(&transfer_secrets.signature_secrets)?;
        tx.signatures = vec![transfer_sigs];
        let order_sigs = tx.create_sigs(&order_secrets.signature_secrets)?;
        tx.signatures.push(order_sigs);
        let fee_sigs = tx.create_sigs(&fee_secrets)?;
        tx.signatures.push(fee_sigs);
        assert!(tx.signatures.len() == 3);
        Ok((tx, (transfer_params, order_params, fee_call_params), spent_coins))
    }

    /// Execute a `Exchange::OrderMatch` transaction for a given [`Holder`].
    ///
    /// Returns any found [`OwnCoin`]s.
    pub async fn execute_order_match_tx(
        &mut self,
        holder: &Holder,
        tx: Transaction,
        transfer_params: &MoneyTransferParamsV1,
        _order_params: &OrderMatchParams,
        fee_params: &MoneyFeeParamsV1,
        block_height: u32,
        append: bool,
    ) -> Result<Vec<OwnCoin>> {
        //TODO use _order_params to verify non-duplicate orders.
        let wallet = self.holders.get_mut(holder).unwrap();
        // Execute the transaction
        wallet.add_transaction("Exchange::OrderMatch", tx, block_height).await?;

        // Iterate over call inputs to mark any spent coins
        let nullifiers =
            transfer_params.inputs.iter().map(|i| i.nullifier.inner()).map(|l| (l, l)).collect();
        wallet.money_null_smt.insert_batch(nullifiers).expect("smt.insert_batch()");
        let mut found_owncoins = vec![];
        if append {
            for input in &transfer_params.inputs {
                if let Some(spent_coin) = wallet
                    .unspent_money_coins
                    .iter()
                    .find(|x| x.nullifier() == input.nullifier)
                    .cloned()
                {
                    debug!("Found spent OwnCoin({}) for {:?}", spent_coin.coin, holder);
                    wallet.unspent_money_coins.retain(|x| x.nullifier() != input.nullifier);
                    wallet.spent_money_coins.push(spent_coin.clone());
                }
            }
            // Iterate over call outputs to find any new OwnCoins
            for output in &transfer_params.outputs {
                wallet.money_merkle_tree.append(MerkleNode::from(output.coin.inner()));

                // Attempt to decrypt the output note to see if this is a coin for the holder.
                let Ok(note) = output.note.decrypt::<MoneyNote>(&wallet.keypair.secret) else {
                    continue
                };

                let owncoin = OwnCoin {
                    coin: output.coin,
                    note: note.clone(),
                    secret: wallet.keypair.secret,
                    leaf_position: wallet.money_merkle_tree.mark().unwrap(),
                };
                debug!("Found new OwnCoin({}) for {:?}", owncoin.coin, holder);
                wallet.unspent_money_coins.push(owncoin.clone());
                found_owncoins.push(owncoin);
            }
        }
        //TODO complete execution order_params, fee_params.
        // Handle fee call
        // Process call input to mark any spent coins

        let nullifier = fee_params.input.nullifier.inner();
        wallet
            .money_null_smt
            .insert_batch(vec![(nullifier, nullifier)])
            .expect("smt.insert_batch()");

        if append {

            if let Some(spent_coin) = wallet
                .unspent_money_coins
                .iter()
                .find(|x| x.nullifier() == fee_params.input.nullifier)
                .cloned()
            {
                debug!("Found spent OwnCoin({}) for {:?}", spent_coin.coin, holder);
                wallet.unspent_money_coins.retain(|x| x.nullifier() != fee_params.input.nullifier);
                wallet.spent_money_coins.push(spent_coin.clone());
            }

            //TODO should this be on a separate merkle tree?
            // Process call output to find any new OwnCoins
            wallet.money_merkle_tree.append(MerkleNode::from(fee_params.output.coin.inner()));

            // Attempt to decrypt the output note to see if this is a coin for the holder.
            if let Ok(note) = fee_params.output.note.decrypt::<MoneyNote>(&wallet.keypair.secret) {
                let owncoin = OwnCoin {
                    coin: fee_params.output.coin,
                    note: note.clone(),
                    secret: wallet.keypair.secret,
                    leaf_position: wallet.money_merkle_tree.mark().unwrap(),
                };

                debug!("Found new OwnCoin({}) for {:?}", owncoin.coin, holder);
                wallet.unspent_money_coins.push(owncoin.clone());
                found_owncoins.push(owncoin);
            };
        }

        //TODO implement exchange, store orders
        // append order to orders database.
        Ok(found_owncoins)
    }
}
