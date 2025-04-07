use darkfi_sdk::error::ContractError;
/// Functions available in the contract
#[repr(u8)]
// ANCHOR: exchange-function
pub enum ExchangeFunction {
    OrderMatch = 0x00,
}

impl TryFrom<u8> for ExchangeFunction {
    type Error = ContractError;

    fn try_from(b: u8) -> core::result::Result<Self, Self::Error> {
        match b {
            0x00 => Ok(Self::OrderMatch),
            _ => Err(ContractError::InvalidFunction),
        }
    }
}

#[cfg(feature = "client")]
pub mod client;
#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint;
pub mod error;
pub mod model;

// These are the different sled trees that will be created
pub const EXCHANGE_CONTRACT_INFO_TREE: &str = "exchange_info";
pub const EXCHANGE_CONTRACT_COINS_TREE: &str = "exchange_coins";
pub const EXCHANGE_CONTRACT_COIN_ROOTS_TREE: &str = "exchange_coin_roots";
pub const EXCHANGE_CONTRACT_NULLIFIERS_TREE: &str = "exchange_nullifiers";
pub const EXCHANGE_CONTRACT_NULLIFIER_ROOTS_TREE: &str = "exchange_nullifier_roots";
pub const EXCHANGE_CONTRACT_TOKEN_FREEZE_TREE: &str = "exchange_token_freezes";
pub const EXCHANGE_CONTRACT_ORDER_MATCH_TREE: &str = "exchange_fees";

// These are keys inside the info tree
pub const EXCHANGE_CONTRACT_DB_VERSION: &[u8] = b"db_version";
pub const EXCHANGE_CONTRACT_COIN_MERKLE_TREE: &[u8] = b"coins_tree";
pub const EXCHANGE_CONTRACT_LATEST_COIN_ROOT: &[u8] = b"last_coins_root";
pub const EXCHANGE_CONTRACT_LATEST_NULLIFIER_ROOT: &[u8] = b"last_nullifiers_root";

/// Precalculated root hash for a tree containing only a single Fp::ZERO coin.
/// Used to save gas.
pub const EMPTY_COINS_TREE_ROOT: [u8; 32] = [
    0xb8, 0xc1, 0x07, 0x5a, 0x80, 0xa8, 0x09, 0x65, 0xc2, 0x39, 0x8f, 0x71, 0x1f, 0xe7, 0x3e, 0x05,
    0xb4, 0xed, 0xae, 0xde, 0xf1, 0x62, 0xf2, 0x61, 0xd4, 0xee, 0xd7, 0xcd, 0x72, 0x74, 0x8d, 0x17,
];

/// zkas order match circuit namespace
pub const EXCHANGE_CONTRACT_ZKAS_ORDER_MATCH: &str = "Order";

pub const MINIMAL_TIMEOUT_DURATION: u64 = 100;
