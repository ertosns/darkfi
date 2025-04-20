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
    zk::{Proof, ProvingKey},
    zkas::ZkBinary,
    ClientFailed, Result,
};
use darkfi_sdk::crypto::{note::AeadEncryptedNote, BaseBlind, Blind, ScalarBlind, SecretKey};
use log::debug;
use rand::rngs::OsRng;

use super::proof::create_order_mint_proof;
use darkfi_money_contract::client::compute_remainder_blind;

use crate::{
    client::OrderNote,
    error::OrderError,
    model::{Input, OrderAttributes, OrderMatchParams, Output},
};

pub struct OrderCallInput {
    pub transfer_secrets: darkfi_money_contract::client::transfer_v1::TransferCallSecrets,
    /// Transfer call spent coins's inputs
    pub transfer_inputs: Vec<darkfi_money_contract::model::Input>,
    /// Transfer mint call outputs
    pub transfer_outputs: Vec<darkfi_money_contract::model::Output>,
}

pub type OrderCallOutput = OrderAttributes;

/// Struct holding necessary information to build a `Exchange::OrderMatch` contract call.
pub struct OrderCallBuilder {
    pub inputs: Vec<OrderCallInput>,
    /// Anonymous outputs
    pub outputs: Vec<OrderCallOutput>,
    /// `OrderMint` zkas circuit ZkBinary
    pub order_zkbin: ZkBinary,
    /// Proving key for the `OrderMint` zk circuit
    pub order_pk: ProvingKey,
}

impl OrderCallBuilder {
    pub fn build(self) -> Result<(OrderMatchParams, OrderCallSecrets)> {
        let mut params = OrderMatchParams { inputs: vec![], outputs: vec![] };
        let mut signature_secrets = vec![];
        let mut proofs = vec![];
        let base_token_id_blind = self.inputs[0].transfer_secrets.output_notes[0].token_blind;
        let quote_token_id_blind = BaseBlind::random(&mut OsRng);
        let timeout_duration_blind = ScalarBlind::random(&mut OsRng);
        let mut base_input_blinds = vec![];
        let mut base_output_blinds = vec![];
        let mut quote_output_blinds = vec![];

        for input in self.inputs.iter() {
            for value_blind in input.transfer_secrets.input_value_blinds.clone() {
                base_input_blinds.push(value_blind);
            }
            for signature_secret in input.transfer_secrets.signature_secrets.clone() {
                signature_secrets.push(signature_secret);
            }
            params.inputs.push(Input {
                transfer_inputs: input.transfer_inputs.clone(),
                transfer_outputs: input.transfer_outputs.clone(),
            });
            for proof in input.transfer_secrets.proofs.clone() {
                proofs.push(proof);
            }
        }

        if self.outputs.is_empty() {
            return Err(ClientFailed::VerifyError(OrderError::OrderMissingOutputs.to_string()).into())
        }

        let mut output_notes = vec![];

        for (i, output) in self.outputs.iter().enumerate() {
            let base_value_blind = if i == self.outputs.len() - 1 {
                compute_remainder_blind(&base_input_blinds, &base_output_blinds)
            } else {
                Blind::random(&mut OsRng)
            };

            base_output_blinds.push(base_value_blind);
            let quote_value_blind = Blind::random(&mut OsRng);
            quote_output_blinds.push(quote_value_blind);

            debug!(target: "contract::money::client::transfer::build", "Creating Order mint proof for output {}", i);
            let (proof, public_inputs) = create_order_mint_proof(
                &self.order_zkbin,
                &self.order_pk,
                output,
                output.spend_hook,
                output.user_data,
                output.bulla_blind,
                base_value_blind,
                quote_value_blind,
                base_token_id_blind,
                quote_token_id_blind,
                timeout_duration_blind,
            )?;
            proofs.push(proof);

            // Encrypted note
            let note = OrderNote {
                base_value: output.base_value,
                quote_value: output.quote_value,
                base_token_id: output.base_token_id,
                quote_token_id: output.quote_token_id,
                timeout_duration: output.timeout_duration,
                spend_hook: output.spend_hook,
                user_data: output.user_data,
                bulla_blind: output.bulla_blind,
                base_value_blind,
                quote_value_blind,
                base_token_id_blind,
                quote_token_id_blind,
                timeout_duration_blind,
                memo: vec![],
            };

            let encrypted_note =
                AeadEncryptedNote::encrypt(&note, &output.withdraw_key, &mut OsRng)?;
            output_notes.push(note);

            params.outputs.push(Output {
                order_bulla: public_inputs.order_bulla.inner(),
                base_value_commit: public_inputs.base_value_commit,
                quote_value_commit: public_inputs.quote_value_commit,
                base_token_commit: public_inputs.base_token_id_commit,
                quote_token_commit: public_inputs.quote_token_id_commit,
                timeout_duration_commit: public_inputs.timeout_duration_commit,
                note: encrypted_note,
            });
        }

        // Now we should have all the params, zk proofs, and signature secrets.
        // We return it all and let the caller deal with it.
        let secrets = OrderCallSecrets {
            proofs,
            signature_secrets,
            output_notes,
            base_input_value_blinds: base_input_blinds,
            quote_input_value_blinds: vec![],
            base_output_value_blinds: base_output_blinds,
            quote_output_value_blinds: quote_output_blinds,
        };
        Ok((params, secrets))
    }
}

pub struct OrderCallSecrets {
    /// The ZK proofs created in this builder
    pub proofs: Vec<Proof>,
    /// The ephemeral secret keys created for signing
    pub signature_secrets: Vec<SecretKey>,
    /// Decrypted notes associated with each output
    pub output_notes: Vec<OrderNote>,
    /// The value blinds created for the base inputs
    pub base_input_value_blinds: Vec<ScalarBlind>,
    /// The value blinds created for the quote inputs
    pub quote_input_value_blinds: Vec<ScalarBlind>,
    /// The value blinds created for the base outputs
    pub base_output_value_blinds: Vec<ScalarBlind>,
    /// The value blinds created for the quote outputs
    pub quote_output_value_blinds: Vec<ScalarBlind>,
}
