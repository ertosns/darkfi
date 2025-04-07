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
use darkfi_sdk::blockchain::expected_reward;

#[test]
fn mint_order() -> Result<()> {
    smol::block_on(async {
        init_logger();
        // Holders this test will use
        const HOLDERS: [Holder; 3] = [Holder::Alice, Holder::Bob, Holder::Charlie];

        // Initialize harness

        let mut th = TestHarness::new(&HOLDERS, true).await?;

        // Generate three new blocks mined by Alice
        th.generate_block(&Holder::Alice, &HOLDERS).await?;
        th.generate_block(&Holder::Alice, &HOLDERS).await?;
        th.generate_block(&Holder::Alice, &HOLDERS).await?;

        // Generate three new blocks mined by Bob
        th.generate_block(&Holder::Bob, &HOLDERS).await?;
        th.generate_block(&Holder::Bob, &HOLDERS).await?;
        th.generate_block(&Holder::Bob, &HOLDERS).await?;

        // Generate three new blocks minted by Charlie
        th.generate_block(&Holder::Charlie, &HOLDERS).await?;
        th.generate_block(&Holder::Charlie, &HOLDERS).await?;
        th.generate_block(&Holder::Charlie, &HOLDERS).await?;

        // Assert correct rewards
        let alice_coins = &th.holders.get(&Holder::Alice).unwrap().unspent_money_coins;
        let bob_coins = &th.holders.get(&Holder::Bob).unwrap().unspent_money_coins;
        let charlie_coins = &th.holders.get(&Holder::Charlie).unwrap().unspent_money_coins;
        assert!(alice_coins.len() == 3);
        assert!(bob_coins.len() == 3);
        assert!(charlie_coins.len() == 3);
        assert!(alice_coins[0].note.value == expected_reward(1));
        assert!(alice_coins[1].note.value == expected_reward(2));
        assert!(bob_coins[0].note.value == expected_reward(3));
        assert!(bob_coins[1].note.value == expected_reward(4));

        let current_block_height = 9;
        let alice_coins = &th.holders.get(&Holder::Alice).unwrap().unspent_money_coins;
        let (tx, (xfer_params, order_params, fee_params), _spent_coins) = th
            .order_match(
                alice_coins[0].clone().note.value,
                bob_coins[0].clone().note.value,
                &Holder::Alice,
                &Holder::Charlie,
                &[alice_coins[0].clone()],
                alice_coins[0].note.token_id,
                bob_coins[0].clone().note.token_id,
                100,
                current_block_height,
            )
            .await?;
        // Execute the transaction
        // this require adding fee tranascation to pass validation/verification.
        for holder in &HOLDERS {
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

        let alice_coins = &th.holders.get(&Holder::Alice).unwrap().unspent_money_coins;
        let bob_coins = &th.holders.get(&Holder::Bob).unwrap().unspent_money_coins;

        let (tx, (xfer_params, order_params, fee_params), _spent_coins) = th
            .order_match(
                bob_coins[0].clone().note.value,
                alice_coins[0].note.value,
                &Holder::Bob,
                &Holder::Charlie,
                &[bob_coins[0].clone()],
                bob_coins[0].clone().note.token_id,
                alice_coins[0].note.token_id,
                100,
                current_block_height,
            )
            .await?;
        // Execute the transaction
        // this require adding fee tranascation to pass validation/verification.
        for holder in &HOLDERS {
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
        let alice_coins = th.holders.get(&Holder::Alice).unwrap().unspent_money_coins.clone();
        let bob_coins = th.holders.get(&Holder::Bob).unwrap().unspent_money_coins.clone();
        let charlie_coins = &th.holders.get(&Holder::Charlie).unwrap().unspent_money_coins;
        //let charlie = &th.holders.get(&Holder::Charlie).unwrap().unspent_money_coins;
        assert!(alice_coins.len() == 2);
        assert!(bob_coins.len() == 2);
        assert!(charlie_coins.len() == 5);
        Ok(())
    })
}
