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
use darkfi_sdk::crypto::BaseBlind;
use log::info;
use rand::rngs::OsRng;

#[test]
fn exchange_swap() -> Result<()> {
    // the following imitate and exchange receiving liquidity from liquidity providers
    // to performs:
    // (1) Alice mint token A
    // (2) Bob mints Token B
    // (3) Alice mint and order, and send his funds to the exchange
    // (4) Bob mints an order, and sends his funds to the exchange
    // (5) after the exchange finds a match in the order-book, it make makes a full swap
    //     while keeping funds in it's possession.
    // (6) exchange sends B token to Alice
    // (7) exchange sends A token to Bob
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
        // (1) Alice mint token A
        let alice_token_blind = BaseBlind::random(&mut OsRng);
        let (mint_tx, mint_params, mint_auth_params, fee_params) = th
            .token_mint(
                ALICE_INITIAL,
                &Holder::Alice,
                &Holder::Alice,
                alice_token_blind,
                None,
                None,
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
                None,
                None,
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
        let bob_base_value = alice_quote_value.clone();
        let bob_quote_value = alice_base_value.clone();

        info!(target: "exchange", "==========================");
        info!(target: "exchange", " Alice make order match tx");
        info!(target: "exchange", "==========================");
        // (3) Alice mint and order, and send his funds to the exchange
        let (tx, (xfer_params, order_params, fee_params), _spent_coins) = th
            .order_match(
                alice_base_value.clone(),
                alice_quote_value.clone(),
                &Holder::Alice,
                &Holder::Charlie,
                &alice_funds,
                alice_token_id.clone(),
                bob_token_id.clone(),
                100, //timeout duration
                current_block_height,
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

        info!(target: "money", "[Alice, Bob] ================");
        info!(target: "money", "[Alice, Bob] Building OtcSwap");
        info!(target: "money", "[Alice, Bob] ================");

        let alice_owncoins = th.holders.get(&Holder::Alice).unwrap().unspent_money_coins.clone();
        let mut bob_owncoins = th.holders.get(&Holder::Bob).unwrap().unspent_money_coins.clone();
        let charlie_owncoins =
            th.holders.get(&Holder::Charlie).unwrap().unspent_money_coins.clone();
        assert!(charlie_owncoins.len() == 2);
        assert!(charlie_owncoins[0].note.token_id == alice_token_id);
        assert!(charlie_owncoins[1].note.token_id == bob_token_id);
        th.assert_trees(&HOLDERS);

        let alice_coin_idx = charlie_owncoins.len() - 2;
        let bob_coin_idx = charlie_owncoins.len() - 1;
        let alice_oc = charlie_owncoins[alice_coin_idx].clone();
        let bob_oc = charlie_owncoins[bob_coin_idx].clone();

        assert!(alice_owncoins.len() == 1);
        assert!(bob_owncoins.len() == 1);
        // (5) after the exchange finds a match in the order-book, it make makes a full swap
        // while keeping funds in it's possession.
        let (otc_swap_tx, otc_swap_params, fee_params) = th
            .otc_swap(&Holder::Charlie, &alice_oc, &Holder::Charlie, &bob_oc, current_block_height)
            .await?;

        for holder in &HOLDERS {
            info!(target: "money", "[{holder:?}] ==========================");
            info!(target: "money", "[{holder:?}] Executing AliceBob swap tx");
            info!(target: "money", "[{holder:?}] ==========================");
            th.execute_otc_swap_tx(
                holder,
                otc_swap_tx.clone(),
                &otc_swap_params,
                &fee_params,
                current_block_height,
                true,
            )
            .await?;
        }

        let mut charlie_owncoins =
            th.holders.get(&Holder::Charlie).unwrap().unspent_money_coins.clone();
        assert!(charlie_owncoins.len() == 2);
        assert!(charlie_owncoins[0].note.token_id == bob_token_id);
        assert!(charlie_owncoins[1].note.token_id == alice_token_id);
        th.assert_trees(&HOLDERS);

        info!(target: "money", "[Alice] ============================================================");
        info!(target: "money", "[Alice] charlie now need to send alice's share of the swap to alice ");
        info!(target: "money", "[Alice] ============================================================");
        let alice_token_idx = charlie_owncoins.len() - 2;
        let charlie2alice_token_id = charlie_owncoins[alice_token_idx].note.token_id;
        // (6) exchange sends B token to Alice
        let (tx, (params, fee_params), spent_coins) = th
            .transfer(
                BOB_INITIAL,
                &Holder::Charlie,
                &Holder::Alice,
                &[charlie_owncoins[alice_token_idx].clone()],
                charlie2alice_token_id,
                current_block_height,
                false,
            )
            .await?;

        for coin in spent_coins {
            charlie_owncoins.retain(|x| x != &coin);
        }
        assert!(bob_token_id == charlie2alice_token_id);
        assert!(params.inputs.len() == 1);
        assert!(params.outputs.len() == 1);

        for holder in &HOLDERS {
            th.execute_transfer_tx(
                holder,
                tx.clone(),
                &params,
                &fee_params,
                current_block_height,
                true,
            )
            .await?;
        }
        th.assert_trees(&HOLDERS);

        let alice_owncoins = th.holders.get(&Holder::Alice).unwrap().unspent_money_coins.clone();
        assert!(alice_owncoins.len() == 2);
        assert!(alice_owncoins[1].note.value == BOB_INITIAL);
        assert!(alice_owncoins[1].note.token_id == bob_token_id);

        info!(target: "money", "[Bob] =======================================================");
        info!(target: "money", "[Bob] Charlie now need to send bob's share of teh swap to bob");
        info!(target: "money", "[Bob] =======================================================");

        let bob_token_idx = charlie_owncoins.len() - 1;
        let charlie2bob_token_id = charlie_owncoins[bob_token_idx].note.token_id;
        // (7) exchange sends A token to Bob
        let (tx, (params, fee_params), spent_coins) = th
            .transfer(
                ALICE_INITIAL,
                &Holder::Charlie,
                &Holder::Bob,
                &[charlie_owncoins[bob_token_idx].clone()],
                charlie2bob_token_id,
                current_block_height,
                false,
            )
            .await?;

        for coin in spent_coins {
            bob_owncoins.retain(|x| x != &coin);
        }
        assert!(alice_token_id == charlie2bob_token_id);
        assert!(params.inputs.len() == 1);
        assert!(params.outputs.len() == 1);

        for holder in &HOLDERS {
            th.execute_transfer_tx(
                holder,
                tx.clone(),
                &params,
                &fee_params,
                current_block_height,
                true,
            )
            .await?;
        }

        th.assert_trees(&HOLDERS);
        let bob_owncoins = th.holders.get(&Holder::Bob).unwrap().unspent_money_coins.clone();

        assert!(bob_owncoins.len() == 2);
        assert!(bob_owncoins[1].note.value == ALICE_INITIAL);
        assert!(bob_owncoins[1].note.token_id == alice_token_id);

        Ok(())
    })
}
