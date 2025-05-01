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

use darkfi::Result;
use darkfi_contract_test_harness::{init_logger, Holder, TestHarness};
use darkfi::zk::{halo2::Field};
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
        BaseBlind,
    },
    pasta::pallas,
    ContractCall,
};
use log::info;
use rand::rngs::OsRng;
use darkfi_serial::{async_trait, AsyncEncodable, SerialDecodable, SerialEncodable};

#[derive(Debug, Clone, SerialEncodable, SerialDecodable)]
pub struct DummyOrder {
    pub withdraw_key: PublicKey,
    pub base_value: u64,
    pub quote_value: u64,
    pub timeout_duration: u64,
}

impl DummyOrder {
    pub fn to_bulla(&self) -> OrderBulla {
        let (withdraw_x, withdraw_y) = self.withdraw_key.xy();
        let bulla = poseidon_hash([
            withdraw_x,
            withdraw_y,
            pallas::Base::from(self.base_value),
            pallas::Base::from(self.quote_value),
            //self.base_token_id.inner(),
            //self.quote_token_id.inner(),
            pallas::Base::from(self.timeout_duration),
        ]);
        OrderBulla(bulla)
    }
}


#[test]
fn exchange_swap() -> Result<()> {
    // the following imitate and exchange receiving liquidity from liquidity providers
    // to performs:
    // (1) Alice mint token A
    // (2) Bob mints Token B
    // (3) Alice mint and order, and send his funds to the exchange
    // (4) Bob mints an order, and sends his funds to the exchange
    // (5) TODO after the exchange finds a match in the order-book, it make makes a full swap
    //     while keeping funds in it's possession.
    // (6) TODO exchange sends B token to Alice
    // (7) TODO exchange sends A token to Bob
    smol::block_on(async {
        init_logger();
        // Holders this test will use
        const HOLDERS: [Holder; 3] = [Holder::Alice, Holder::Bob, Holder::Charlie];

        // Some numbers we want to assert
        const ALICE_INITIAL: u64 = 1000;
        const BOB_INITIAL: u64 = 1000;

        // Block height to verify against
        let current_block_height = 0;

        // Initialize harness
        let mut th = TestHarness::new(&HOLDERS, false).await?;
        // Generate three new blocks mined by Alice
        // for order_match gas fee
        th.generate_block(&Holder::Alice, &HOLDERS).await?;
        th.generate_block(&Holder::Bob, &HOLDERS).await?;

        info!(target: "exchange", "[Alice] ================================");
        info!(target: "exchange", "[Alice] Building token mint tx for Alice");
        info!(target: "exchange", "[Alice] ================================");
        let spend_hook: FuncId = FuncRef {
            contract_id: *EXCHANGE_CONTRACT_ID,
            func_code: ExchangeFunction::OrderMatch as u8,
        }
        .to_func_id();
        //TODO fix, this will make order for OrderMatch look different from POV of bob, and Alice, which should be the case.
        let order = DummyOrder {
            withdraw_key: th.holders.get(&Holder::Bob).unwrap().keypair.public,
            base_value: 1000,
            quote_value: 1000,
            //base_token_id: bob_token_id,
            //quote_token_id: alice_token_id,
            timeout_duration: 100,
        };
        //let user_data = order.to_bulla();
        let user_data = pallas::Base::ZERO;
        // (1) Alice mint token A
        let alice_token_blind = BaseBlind::random(&mut OsRng);
        let (mint_tx, mint_params, mint_auth_params, fee_params) = th
            .token_mint(
                ALICE_INITIAL,
                &Holder::Alice,
                &Holder::Alice,
                alice_token_blind,
                Some(spend_hook),
                Some(user_data),
                current_block_height,
            )
            .await?;

        for holder in &HOLDERS {
            info!(target: "exchange", "[{holder:?}] ==============================");
            info!(target: "exchange", "[{holder:?}] Executing Alice token mint tx");
            info!(target: "exchange", "[{holder:?}] ==============================");
            th.execute_token_mint_tx(
                holder,
                mint_tx.clone(),
                &mint_params,
                &mint_auth_params,
                &fee_params,
                current_block_height,
                true,
            )
            .await?;
        }

        th.assert_trees(&HOLDERS);
        // (2) Bob mints Token B
        info!(target: "exchange", "[Bob] ==============================");
        info!(target: "exchange", "[Bob] Building token mint tx for Bob");
        info!(target: "exchange", "[Bob] ==============================");
        let bob_token_blind = BaseBlind::random(&mut OsRng);
        let (mint_tx, mint_params, mint_auth_params, fee_params) = th
            .token_mint(
                BOB_INITIAL,
                &Holder::Bob,
                &Holder::Bob,
                bob_token_blind,
                Some(spend_hook),
                Some(user_data),
                current_block_height,
            )
            .await?;

        for holder in &HOLDERS {
            info!(target: "exchange", "[{holder:?}] ===========================");
            info!(target: "exchange", "[{holder:?}] Executing Bob token mint tx");
            info!(target: "exchange", "[{holder:?}] ===========================");
            th.execute_token_mint_tx(
                holder,
                mint_tx.clone(),
                &mint_params,
                &mint_auth_params,
                &fee_params,
                current_block_height,
                true,
            )
            .await?;
        }
        th.assert_trees(&HOLDERS);

        let alice_owncoins = th.holders.get(&Holder::Alice).unwrap().unspent_money_coins.clone();
        let bob_owncoins = th.holders.get(&Holder::Bob).unwrap().unspent_money_coins.clone();
        let alice_token_id = alice_owncoins[1].note.token_id;
        let bob_token_id = bob_owncoins[1].note.token_id;

        // alice funds
        let mut alice_funds = alice_owncoins.clone();
        alice_funds.retain(|x| x.note.token_id == alice_token_id);
        // bob funds
        let mut bob_funds = bob_owncoins.clone();
        bob_funds.retain(|x| x.note.token_id == bob_token_id);
        // alice base/quote
        let alice_base_value = alice_funds[0].clone().note.value;
        let alice_quote_value = bob_funds[0].clone().note.value;
        // bob base/quote
        let bob_base_value = alice_quote_value;
        let bob_quote_value = alice_base_value;

        info!(target: "exchange", "==========================");
        info!(target: "exchange", " Alice make order match tx");
        info!(target: "exchange", "==========================");
        // (3) Alice mint and order, and send his funds to the exchange
        let (tx, (xfer_params, order_params, fee_params), _spent_coins) = th
            .order_match(
                alice_base_value,
                alice_quote_value,
                &Holder::Alice,
                &Holder::Charlie,
                &alice_funds,
                alice_token_id,
                bob_token_id,
                100, //timeout duration
                current_block_height,
                spend_hook,
                user_data,
            )
            .await?;
        for holder in &HOLDERS {
            info!(target: "exchange", "[{holder:?}] ==============================");
            info!(target: "exchange", "[{holder:?}] Executing Alice order match tx");
            info!(target: "exchange", "[{holder:?}] ==============================");
            th.execute_order_match_tx(
                holder,
                tx.clone(),
                &xfer_params,
                &order_params,
                &fee_params,
                current_block_height,
                true,
            )
            .await?;
        }
        th.assert_trees(&HOLDERS);

        info!(target: "exchange", "========================");
        info!(target: "exchange", " Bob make order match tx");
        info!(target: "exchange", "========================");
        // (4) Bob mints an order, and sends his funds to the exchange
        let (tx, (xfer_params, order_params, fee_params), _spent_coins) = th
            .order_match(
                bob_base_value,
                bob_quote_value,
                &Holder::Bob,
                &Holder::Charlie,
                &bob_funds,
                bob_token_id,
                alice_token_id,
                100, //timeout duration
                current_block_height,
                spend_hook,
                user_data,
            )
            .await?;
        for holder in &HOLDERS {
            info!(target: "exchange", "[{holder:?}] ============================");
            info!(target: "exchange", "[{holder:?}] Executing Bob order match tx");
            info!(target: "exchange", "[{holder:?}] ============================");
            th.execute_order_match_tx(
                holder,
                tx.clone(),
                &xfer_params,
                &order_params,
                &fee_params,
                current_block_height,
                true,
            )
            .await?;
        }
        th.assert_trees(&HOLDERS);

        let alice_owncoins = th.holders.get(&Holder::Alice).unwrap().unspent_money_coins.clone();
        let mut bob_owncoins = th.holders.get(&Holder::Bob).unwrap().unspent_money_coins.clone();
        let charlie_owncoins =
            th.holders.get(&Holder::Charlie).unwrap().unspent_money_coins.clone();
        assert!(charlie_owncoins.len() == 2);
        assert!(charlie_owncoins[0].note.token_id == alice_token_id);
        assert!(charlie_owncoins[1].note.token_id == bob_token_id);

        let alice_coin_idx = charlie_owncoins.len() - 2;
        let bob_coin_idx = charlie_owncoins.len() - 1;
        let alice_oc = charlie_owncoins[alice_coin_idx].clone();
        let bob_oc = charlie_owncoins[bob_coin_idx].clone();

        assert!(alice_owncoins.len() == 1);
        assert!(bob_owncoins.len() == 1);
        Ok(())
    })
}
