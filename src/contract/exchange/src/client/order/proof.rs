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

use darkfi::{
    zk::{halo2::Value, Proof, ProvingKey, Witness, ZkCircuit},
    zkas::ZkBinary,
    Result,
};
use darkfi_sdk::{
    crypto::{
        pasta_prelude::*, pedersen_commitment_u64, poseidon_hash, BaseBlind, FuncId, ScalarBlind,
    },
    pasta::pallas,
};

use rand::rngs::OsRng;

use super::OrderCallOutput;
use crate::model::OrderBulla;

pub struct OrderMintRevealed {
    pub order_bulla: OrderBulla,
    pub base_value_commit: pallas::Point,
    pub quote_value_commit: pallas::Point,
    pub base_token_id_commit: pallas::Base,
    pub quote_token_id_commit: pallas::Base,
    pub timeout_duration_commit: pallas::Point,
}

impl OrderMintRevealed {
    pub fn to_vec(&self) -> Vec<pallas::Base> {
        let base_valcom_coords = self.base_value_commit.to_affine().coordinates().unwrap();
        let quote_valcom_coords = self.quote_value_commit.to_affine().coordinates().unwrap();
        let timeout_duration_coords =
            self.timeout_duration_commit.to_affine().coordinates().unwrap();

        // NOTE: It's important to keep these in the same order
        // as the `constrain_instance` calls in the zkas code.
        vec![
            self.order_bulla.inner(),
            *base_valcom_coords.x(),
            *base_valcom_coords.y(),
            *quote_valcom_coords.x(),
            *quote_valcom_coords.y(),
            self.base_token_id_commit,
            self.quote_token_id_commit,
            *timeout_duration_coords.x(),
            *timeout_duration_coords.y(),
        ]
    }
}

#[allow(clippy::too_many_arguments)]
pub fn create_order_mint_proof(
    zkbin: &ZkBinary,
    pk: &ProvingKey,
    output: &OrderCallOutput,
    spend_hook: FuncId,
    user_data: pallas::Base,
    bulla_blind: BaseBlind,
    base_value_blind: ScalarBlind,
    quote_value_blind: ScalarBlind,
    base_token_blind: BaseBlind,
    quote_token_blind: BaseBlind,
    timeout_duration_blind: ScalarBlind,
) -> Result<(Proof, OrderMintRevealed)> {
    let (withdraw_x, withdraw_y) = output.withdraw_key.xy();
    let order_bulla = output.to_bulla();
    let base_value_commit = pedersen_commitment_u64(output.base_value, base_value_blind);
    let quote_value_commit = pedersen_commitment_u64(output.quote_value, quote_value_blind);
    let base_token_id_commit =
        poseidon_hash([output.base_token_id.inner(), base_token_blind.inner()]);
    let quote_token_id_commit =
        poseidon_hash([output.quote_token_id.inner(), quote_token_blind.inner()]);
    let timeout_duration_commit =
        pedersen_commitment_u64(output.timeout_duration, timeout_duration_blind);

    let public_inputs = OrderMintRevealed {
        order_bulla,
        base_value_commit,
        quote_value_commit,
        base_token_id_commit,
        quote_token_id_commit,
        timeout_duration_commit,
    };

    let prover_witnesses = vec![
        Witness::Base(Value::known(withdraw_x)),
        Witness::Base(Value::known(withdraw_y)),
        Witness::Base(Value::known(pallas::Base::from(output.base_value))),
        Witness::Base(Value::known(pallas::Base::from(output.quote_value))),
        Witness::Base(Value::known(output.base_token_id.inner())),
        Witness::Base(Value::known(output.quote_token_id.inner())),
        Witness::Base(Value::known(pallas::Base::from(output.timeout_duration))),
        Witness::Base(Value::known(spend_hook.inner())),
        Witness::Base(Value::known(user_data)),
        Witness::Base(Value::known(bulla_blind.inner())),
        Witness::Scalar(Value::known(base_value_blind.inner())),
        Witness::Scalar(Value::known(quote_value_blind.inner())),
        Witness::Base(Value::known(base_token_blind.inner())),
        Witness::Base(Value::known(quote_token_blind.inner())),
        Witness::Scalar(Value::known(timeout_duration_blind.inner())),
    ];

    darkfi::zk::export_witness_json(
        "proof/witness/order.json",
        &prover_witnesses,
        &public_inputs.to_vec(),
    );

    let circuit = ZkCircuit::new(prover_witnesses, zkbin);

    let proof = Proof::create(pk, &[circuit], &public_inputs.to_vec(), &mut OsRng)?;

    Ok((proof, public_inputs))
}
