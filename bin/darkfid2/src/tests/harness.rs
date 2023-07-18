/* This file is part of DarkFi (https://dark.fi)
 *
 * Copyright (C) 2020-2023 Dyne.org foundation
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

use darkfi::{
    blockchain::{BlockInfo, Header},
    net::{P2p, Settings},
    util::time::TimeKeeper,
    validator::{
        consensus::{next_block_reward, pid::slot_pid_output},
        Validator, ValidatorConfig,
    },
    Result,
};
use darkfi_contract_test_harness::{vks, Holder, TestHarness};
use darkfi_sdk::{
    blockchain::Slot,
    pasta::{group::ff::Field, pallas},
};

use crate::{utils::genesis_txs_total, Darkfid};

pub struct HarnessConfig {
    pub testing_node: bool,
    pub alice_initial: u64,
    pub bob_initial: u64,
}

pub struct Harness {
    pub config: HarnessConfig,
    pub alice: Darkfid,
    pub bob: Darkfid,
}

impl Harness {
    pub async fn new(config: HarnessConfig) -> Result<Self> {
        // Use test harness to generate genesis transactions
        let mut th = TestHarness::new(&["money".to_string(), "consensus".to_string()]).await?;
        let (genesis_stake_tx, _) = th.genesis_stake(Holder::Alice, config.alice_initial)?;
        let (genesis_mint_tx, _) = th.genesis_mint(Holder::Bob, config.bob_initial)?;

        // Generate default genesis block
        let mut genesis_block = BlockInfo::default();

        // Append genesis transactions and calculate their total
        genesis_block.txs.push(genesis_stake_tx);
        genesis_block.txs.push(genesis_mint_tx);
        let genesis_txs_total = genesis_txs_total(&genesis_block.txs)?;
        genesis_block.slots[0].total_tokens = genesis_txs_total;

        // Generate validators configuration
        // NOTE: we are not using consensus constants here so we
        // don't get circular dependencies.
        let time_keeper = TimeKeeper::new(genesis_block.header.timestamp, 10, 90, 0);
        let val_config = ValidatorConfig::new(
            time_keeper,
            genesis_block,
            genesis_txs_total,
            vec![],
            config.testing_node,
        );

        // Generate validators using pregenerated vks
        let sync_p2p = P2p::new(Settings::default()).await;
        let sled_db = sled::Config::new().temporary(true).open()?;
        vks::inject(&sled_db)?;
        let validator = Validator::new(&sled_db, val_config.clone()).await?;
        let alice = Darkfid::new(sync_p2p, None, validator).await;

        let sync_p2p = P2p::new(Settings::default()).await;
        let sled_db = sled::Config::new().temporary(true).open()?;
        vks::inject(&sled_db)?;
        let validator = Validator::new(&sled_db, val_config.clone()).await?;
        let bob = Darkfid::new(sync_p2p, None, validator).await;

        Ok(Self { config, alice, bob })
    }

    pub async fn validate_chains(&self) -> Result<()> {
        let genesis_txs_total = self.config.alice_initial + self.config.bob_initial;
        let alice = &self.alice.validator.read().await;
        let bob = &self.bob.validator.read().await;

        alice.validate_blockchain(genesis_txs_total, vec![]).await?;
        bob.validate_blockchain(genesis_txs_total, vec![]).await?;

        assert_eq!(alice.blockchain.len(), bob.blockchain.len());

        Ok(())
    }

    pub async fn add_blocks(&self, blocks: &[BlockInfo]) -> Result<()> {
        let alice = &self.alice.validator.read().await;
        let bob = &self.bob.validator.read().await;

        alice.add_blocks(blocks).await?;
        bob.add_blocks(blocks).await?;

        Ok(())
    }

    pub async fn generate_next_block(
        &self,
        previous: &BlockInfo,
        slots_count: usize,
    ) -> Result<BlockInfo> {
        let previous_hash = previous.blockhash();

        // Generate empty slots
        let mut slots = Vec::with_capacity(slots_count);
        let mut previous_slot = previous.slots.last().unwrap().clone();
        for _ in 0..slots_count - 1 {
            let (f, error, sigma1, sigma2) = slot_pid_output(&previous_slot);
            let slot = Slot::new(
                previous_slot.id + 1,
                pallas::Base::ZERO,
                vec![previous_hash],
                vec![previous.header.previous.clone()],
                f,
                error,
                previous_slot.error,
                previous_slot.total_tokens + previous_slot.reward,
                0,
                sigma1,
                sigma2,
            );
            slots.push(slot.clone());
            previous_slot = slot;
        }

        // Generate slot
        let (f, error, sigma1, sigma2) = slot_pid_output(&previous_slot);
        let slot = Slot::new(
            previous_slot.id + 1,
            pallas::Base::ZERO,
            vec![previous_hash],
            vec![previous.header.previous.clone()],
            f,
            error,
            previous_slot.error,
            previous_slot.total_tokens + previous_slot.reward,
            next_block_reward(),
            sigma1,
            sigma2,
        );
        slots.push(slot);

        // We increment timestamp so we don't have to use sleep
        let mut timestamp = previous.header.timestamp;
        timestamp.add(1);

        // Generate header
        let header = Header::new(
            previous_hash,
            previous.header.epoch,
            previous_slot.id + 1,
            timestamp,
            previous.header.root.clone(),
        );

        // Generate block
        let block = BlockInfo::new(header, vec![], previous.producer.clone(), slots);

        Ok(block)
    }
}
