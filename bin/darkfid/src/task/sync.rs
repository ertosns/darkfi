/* This file is part of DarkFi (https://dark.fi)
 *
 * Copyright (C) 2020-2024 Dyne.org foundation
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

use std::collections::HashMap;

use darkfi::{
    blockchain::HeaderHash, net::ChannelPtr, rpc::jsonrpc::JsonSubscriber, system::sleep,
    util::encoding::base64, validator::consensus::Proposal, Error, Result,
};
use darkfi_serial::serialize_async;
use log::{debug, info, warn};
use rand::{prelude::SliceRandom, rngs::OsRng};
use tinyjson::JsonValue;

use crate::{
    proto::{
        ForkSyncRequest, ForkSyncResponse, HeaderSyncRequest, HeaderSyncResponse, SyncRequest,
        SyncResponse, TipRequest, TipResponse, BATCH,
    },
    Darkfid,
};

// TODO: Parallelize independent requests.
//       We can also make them be like torrents, where we retrieve chunks not in order.
/// async task used for block syncing.
/// A checkpoint can be provided to ensure node syncs the correct sequence.
pub async fn sync_task(node: &Darkfid, checkpoint: Option<(u32, HeaderHash)>) -> Result<()> {
    info!(target: "darkfid::task::sync_task", "Starting blockchain sync...");

    // Grab blocks subscriber
    let block_sub = node.subscribers.get("blocks").unwrap();

    // Grab last known block header, including existing pending sync ones
    let mut last = node.validator.blockchain.last()?;

    // If checkpoint is not reached, purge headers and start syncing from scratch
    if let Some(checkpoint) = checkpoint {
        if checkpoint.0 > last.0 {
            node.validator.blockchain.headers.remove_all_sync()?;
        }
    }

    // Check sync headers first record is the next one
    if let Some(next) = node.validator.blockchain.headers.get_first_sync()? {
        if next.height == last.0 + 1 {
            // Grab last sync header to continue syncing from
            if let Some(last_sync) = node.validator.blockchain.headers.get_last_sync()? {
                last = (last_sync.height, last_sync.hash());
            }
        } else {
            // Purge headers and start syncing from scratch
            node.validator.blockchain.headers.remove_all_sync()?;
        }
    }
    info!(target: "darkfid::task::sync_task", "Last known block: {} - {}", last.0, last.1);

    // Grab the most common tip and the corresponding peers
    let (mut common_tip_height, mut common_tip_peers) =
        most_common_tip(node, &last.1, checkpoint).await?;

    // If last known block header is before the checkpoint, we sync until that first.
    if let Some(checkpoint) = checkpoint {
        if checkpoint.0 > last.0 {
            info!(target: "darkfid::task::sync_task", "Syncing until configured checkpoint: {} - {}", checkpoint.0, checkpoint.1);
            // Retrieve all the headers backwards until our last known one and verify them.
            // We use the next height, in order to also retrieve the checkpoint header.
            retrieve_headers(node, &common_tip_peers, last.0, checkpoint.0 + 1).await?;

            // Retrieve all the blocks for those headers and apply them to canonical
            last = retrieve_blocks(node, &common_tip_peers, last, block_sub, true).await?;
            info!(target: "darkfid::task::sync_task", "Last received block: {} - {}", last.0, last.1);

            // Grab synced peers most common tip again
            (common_tip_height, common_tip_peers) = most_common_tip(node, &last.1, None).await?;
        }
    }

    // Sync headers and blocks
    loop {
        // Retrieve all the headers backwards until our last known one and verify them.
        // We use the next height, in order to also retrieve the peers tip header.
        retrieve_headers(node, &common_tip_peers, last.0, common_tip_height + 1).await?;

        // Retrieve all the blocks for those headers and apply them to canonical
        let last_received =
            retrieve_blocks(node, &common_tip_peers, last, block_sub, false).await?;
        info!(target: "darkfid::task::sync_task", "Last received block: {} - {}", last_received.0, last_received.1);

        if last == last_received {
            break
        }

        last = last_received;

        // Grab synced peers most common tip again
        (common_tip_height, common_tip_peers) = most_common_tip(node, &last.1, None).await?;
    }

    // Sync best fork
    sync_best_fork(node, &common_tip_peers, &last.1).await?;

    // Perform finalization
    let finalized = node.validator.finalization().await?;
    if !finalized.is_empty() {
        // Notify subscriber
        let mut notif_blocks = Vec::with_capacity(finalized.len());
        for block in finalized {
            notif_blocks.push(JsonValue::String(base64::encode(&serialize_async(&block).await)));
        }
        block_sub.notify(JsonValue::Array(notif_blocks)).await;
    }

    *node.validator.synced.write().await = true;
    info!(target: "darkfid::task::sync_task", "Blockchain synced!");
    Ok(())
}

/// Auxiliary function to block until node is connected to at least one synced peer,
/// and retrieve the synced peers tips.
async fn synced_peers(
    node: &Darkfid,
    last_tip: &HeaderHash,
    checkpoint: Option<(u32, HeaderHash)>,
) -> Result<HashMap<(u32, [u8; 32]), Vec<ChannelPtr>>> {
    info!(target: "darkfid::task::sync::synced_peers", "Receiving tip from peers...");
    let comms_timeout = node.p2p.settings().outbound_connect_timeout;
    let mut tips = HashMap::new();
    loop {
        // Grab channels
        let peers = node.p2p.hosts().channels();

        // Ask each peer(if we got any) if they are synced
        for peer in peers {
            // If a checkpoint was provider, we check that the peer follows that sequence
            if let Some(c) = checkpoint {
                // Communication setup
                let response_sub = peer.subscribe_msg::<HeaderSyncResponse>().await?;

                // Node creates a `HeaderSyncRequest` and sends it
                let request = HeaderSyncRequest { height: c.0 + 1 };
                peer.send(&request).await?;

                // Node waits for response
                let Ok(response) = response_sub.receive_with_timeout(comms_timeout).await else {
                    continue
                };

                // Handle response
                if response.headers.is_empty() || response.headers.last().unwrap().hash() != c.1 {
                    continue
                }
            }

            // Communication setup
            let response_sub = peer.subscribe_msg::<TipResponse>().await?;

            // Node creates a `TipRequest` and sends it
            let request = TipRequest { tip: *last_tip };
            peer.send(&request).await?;

            // Node waits for response
            let Ok(response) = response_sub.receive_with_timeout(comms_timeout).await else {
                continue
            };

            // Handle response
            if response.synced && response.height.is_some() && response.hash.is_some() {
                let tip = (response.height.unwrap(), *response.hash.unwrap().inner());
                let Some(tip_peers) = tips.get_mut(&tip) else {
                    tips.insert(tip, vec![peer.clone()]);
                    continue
                };
                tip_peers.push(peer.clone());
            }
        }

        // Check if we got any tips
        if !tips.is_empty() {
            break
        }

        warn!(target: "darkfid::task::sync::synced_peers", "Node is not connected to other synced nodes, waiting to retry...");
        let subscription = node.p2p.hosts().subscribe_channel().await;
        let _ = subscription.receive().await;
        subscription.unsubscribe().await;

        info!(target: "darkfid::task::sync::synced_peers", "Sleeping for {comms_timeout} to allow for more nodes to connect...");
        sleep(comms_timeout).await;
    }

    Ok(tips)
}

/// Auxiliary function to ask all peers for their current tip and find the most common one.
async fn most_common_tip(
    node: &Darkfid,
    last_tip: &HeaderHash,
    checkpoint: Option<(u32, HeaderHash)>,
) -> Result<(u32, Vec<ChannelPtr>)> {
    // Grab synced peers tips
    let tips = synced_peers(node, last_tip, checkpoint).await?;

    // Grab the most common highest tip peers
    info!(target: "darkfid::task::sync::most_common_tip", "Finding most common tip...");
    let mut common_tip = (0, [0u8; 32], vec![]);
    for (tip, peers) in tips {
        // Check if tip peers is less than the most common tip peers
        if peers.len() < common_tip.2.len() {
            continue;
        }
        // If peers are the same length, skip if tip height is less than
        // the most common tip height.
        if peers.len() == common_tip.2.len() || tip.0 < common_tip.0 {
            continue;
        }
        // Keep the heighest tip with the most peers
        common_tip = (tip.0, tip.1, peers);
    }

    info!(target: "darkfid::task::sync::most_common_tip", "Most common tip: {} - {}", common_tip.0, HeaderHash::new(common_tip.1));
    Ok((common_tip.0, common_tip.2))
}

/// Auxiliary function to retrieve headers backwards until our last known one and verify them.
async fn retrieve_headers(
    node: &Darkfid,
    peers: &[ChannelPtr],
    last_known: u32,
    tip_height: u32,
) -> Result<()> {
    info!(target: "darkfid::task::sync::retrieve_headers", "Retrieving missing headers from peers...");
    // Communication setup
    let mut peer_subs = vec![];
    for peer in peers {
        peer_subs.push(peer.subscribe_msg::<HeaderSyncResponse>().await?);
    }
    let comms_timeout = node.p2p.settings().outbound_connect_timeout;

    // We subtract 1 since tip_height is increased by one
    let total = tip_height - last_known - 1;
    let mut last_tip_height = tip_height;
    'headers_loop: loop {
        for (index, peer) in peers.iter().enumerate() {
            // Node creates a `HeaderSyncRequest` and sends it
            let request = HeaderSyncRequest { height: last_tip_height };
            peer.send(&request).await?;

            // Node waits for response
            let Ok(response) = peer_subs[index].receive_with_timeout(comms_timeout).await else {
                continue
            };

            // Retain only the headers after our last known
            let mut response_headers = response.headers.to_vec();
            response_headers.retain(|h| h.height > last_known);

            if response_headers.is_empty() {
                break 'headers_loop
            }

            // Store the headers
            node.validator.blockchain.headers.insert_sync(&response_headers)?;
            last_tip_height = response_headers[0].height;
            info!(target: "darkfid::task::sync::retrieve_headers", "Headers received: {}/{}", node.validator.blockchain.headers.len_sync(), total);
        }
    }

    // Check if we retrieved any new headers
    if node.validator.blockchain.headers.is_empty_sync() {
        return Ok(());
    }

    // Verify headers sequence. Here we do a quick and dirty verification
    // of just the hashes and heights sequence. We will formaly verify
    // the blocks when we retrieve them. We verify them in batches,
    // to not load them all in memory.
    info!(target: "darkfid::task::sync::retrieve_headers", "Verifying headers sequence...");
    let mut verified_headers = 0;
    let total = node.validator.blockchain.headers.len_sync();
    // First we verify the first `BATCH` sequence, using the last known header
    // as the first sync header previous.
    let last_known = node.validator.consensus.best_fork_last_header().await?;
    let mut headers = node.validator.blockchain.headers.get_after_sync(0, BATCH)?;
    if headers[0].previous != last_known.1 || headers[0].height != last_known.0 + 1 {
        node.validator.blockchain.headers.remove_all_sync()?;
        return Err(Error::BlockIsInvalid(headers[0].hash().as_string()))
    }
    verified_headers += 1;
    for (index, header) in headers[1..].iter().enumerate() {
        if header.previous != headers[index].hash() || header.height != headers[index].height + 1 {
            node.validator.blockchain.headers.remove_all_sync()?;
            return Err(Error::BlockIsInvalid(header.hash().as_string()))
        }
        verified_headers += 1;
    }
    info!(target: "darkfid::task::sync::retrieve_headers", "Headers verified: {}/{}", verified_headers, total);

    // Now we verify the rest sequences
    let mut last_checked = headers.last().unwrap().clone();
    headers = node.validator.blockchain.headers.get_after_sync(last_checked.height, BATCH)?;
    while !headers.is_empty() {
        if headers[0].previous != last_checked.hash() ||
            headers[0].height != last_checked.height + 1
        {
            node.validator.blockchain.headers.remove_all_sync()?;
            return Err(Error::BlockIsInvalid(headers[0].hash().as_string()))
        }
        verified_headers += 1;
        for (index, header) in headers[1..].iter().enumerate() {
            if header.previous != headers[index].hash() ||
                header.height != headers[index].height + 1
            {
                node.validator.blockchain.headers.remove_all_sync()?;
                return Err(Error::BlockIsInvalid(header.hash().as_string()))
            }
            verified_headers += 1;
        }
        last_checked = headers.last().unwrap().clone();
        headers = node.validator.blockchain.headers.get_after_sync(last_checked.height, BATCH)?;
        info!(target: "darkfid::task::sync::retrieve_headers", "Headers verified: {}/{}", verified_headers, total);
    }

    info!(target: "darkfid::task::sync::retrieve_headers", "Headers sequence verified!");
    Ok(())
}

/// Auxiliary function to retrieve blocks of provided headers and apply them to canonical.
async fn retrieve_blocks(
    node: &Darkfid,
    peers: &[ChannelPtr],
    last_known: (u32, HeaderHash),
    block_sub: &JsonSubscriber,
    checkpoint_blocks: bool,
) -> Result<(u32, HeaderHash)> {
    info!(target: "darkfid::task::sync::retrieve_blocks", "Retrieving missing blocks from peers...");
    let mut last_received = last_known;
    // Communication setup
    let mut peer_subs = vec![];
    for peer in peers {
        peer_subs.push(peer.subscribe_msg::<SyncResponse>().await?);
    }
    let comms_timeout = node.p2p.settings().outbound_connect_timeout;

    let mut received_blocks = 0;
    let total = node.validator.blockchain.headers.len_sync();
    'blocks_loop: loop {
        for (index, peer) in peers.iter().enumerate() {
            // Grab first `BATCH` headers
            let headers = node.validator.blockchain.headers.get_after_sync(0, BATCH)?;
            if headers.is_empty() {
                break 'blocks_loop
            }
            let mut headers_hashes = Vec::with_capacity(headers.len());
            let mut synced_headers = Vec::with_capacity(headers.len());
            for header in &headers {
                headers_hashes.push(header.hash());
                synced_headers.push(header.height);
            }

            // Node creates a `SyncRequest` and sends it
            let request = SyncRequest { headers: headers_hashes.clone() };
            peer.send(&request).await?;

            // Node waits for response
            let Ok(response) = peer_subs[index].receive_with_timeout(comms_timeout).await else {
                continue
            };

            // Verify and store retrieved blocks
            debug!(target: "darkfid::task::sync::retrieve_blocks", "Processing received blocks");
            received_blocks += response.blocks.len();
            if checkpoint_blocks {
                node.validator.add_checkpoint_blocks(&response.blocks, &headers_hashes).await?;
            } else {
                for block in &response.blocks {
                    node.validator.append_proposal(&Proposal::new(block.clone())).await?;
                }
            }
            last_received = (*synced_headers.last().unwrap(), *headers_hashes.last().unwrap());

            // Remove synced headers
            node.validator.blockchain.headers.remove_sync(&synced_headers)?;

            if checkpoint_blocks {
                // Notify subscriber
                let mut notif_blocks = Vec::with_capacity(response.blocks.len());
                info!(target: "darkfid::task::sync::retrieve_blocks", "Blocks added:");
                for (index, block) in response.blocks.iter().enumerate() {
                    info!(target: "darkfid::task::sync::retrieve_blocks", "\t{} - {}", headers_hashes[index], headers[index].height);
                    notif_blocks
                        .push(JsonValue::String(base64::encode(&serialize_async(block).await)));
                }
                block_sub.notify(JsonValue::Array(notif_blocks)).await;
            } else {
                // Perform finalization for received blocks
                let finalized = node.validator.finalization().await?;
                if !finalized.is_empty() {
                    // Notify subscriber
                    let mut notif_blocks = Vec::with_capacity(finalized.len());
                    for block in finalized {
                        notif_blocks.push(JsonValue::String(base64::encode(
                            &serialize_async(&block).await,
                        )));
                    }
                    block_sub.notify(JsonValue::Array(notif_blocks)).await;
                }
            }

            info!(target: "darkfid::task::sync::retrieve_blocks", "Blocks received: {}/{}", received_blocks, total);
        }
    }

    Ok(last_received)
}

/// Auxiliary function to retrieve best fork state from a random peer.
async fn sync_best_fork(node: &Darkfid, peers: &[ChannelPtr], last_tip: &HeaderHash) -> Result<()> {
    info!(target: "darkfid::task::sync::sync_best_fork", "Syncing fork states from peers...");
    // Getting a random peer to ask for blocks
    let channel = &peers.choose(&mut OsRng).unwrap();

    // Communication setup
    let response_sub = channel.subscribe_msg::<ForkSyncResponse>().await?;
    let notif_sub = node.subscribers.get("proposals").unwrap();

    // Node creates a `ForkSyncRequest` and sends it
    let request = ForkSyncRequest { tip: *last_tip, fork_tip: None };
    channel.send(&request).await?;

    // Node waits for response
    let response =
        response_sub.receive_with_timeout(node.p2p.settings().outbound_connect_timeout).await?;

    // Verify and store retrieved proposals
    debug!(target: "darkfid::task::sync_task", "Processing received proposals");
    for proposal in &response.proposals {
        node.validator.append_proposal(proposal).await?;
        // Notify subscriber
        let enc_prop = JsonValue::String(base64::encode(&serialize_async(proposal).await));
        notif_sub.notify(vec![enc_prop].into()).await;
    }

    Ok(())
}
