use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Empty, StdError, StdResult, Storage, Uint128};
use cw721_controllers::NftClaims;
use cw_controllers::Hooks;
use cw_storage_plus::{Item, Map, SnapshotItem, SnapshotMap, Strategy};
use cw_utils::Duration;

use crate::ContractError;

#[cw_serde]
pub struct Config {
    pub owner: Option<Addr>,
    pub nft_address: Addr,
    pub unstaking_duration: Option<Duration>,
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const DAO: Item<Addr> = Item::new("dao");

/// The set of NFTs currently staked by each address. The existence of
/// an `(address, token_id)` pair implies that `address` has staked
/// `token_id`.
pub const STAKED_NFTS_PER_OWNER: Map<(&Addr, &str), Empty> = Map::new("snpw");
/// The number of NFTs staked by an address as a function of block
/// height.
pub const NFT_BALANCES: SnapshotMap<&Addr, Uint128> = SnapshotMap::new(
    "nb",
    "nb__checkpoints",
    "nb__changelog",
    Strategy::EveryBlock,
);
/// The number of NFTs staked with this contract as a function of
/// block height.
pub const TOTAL_STAKED_NFTS: SnapshotItem<Uint128> = SnapshotItem::new(
    "tsn",
    "tsn__checkpoints",
    "tsn__changelog",
    Strategy::EveryBlock,
);

/// The maximum number of claims that may be outstanding.
pub const MAX_CLAIMS: u64 = 100;
pub const NFT_CLAIMS: NftClaims = NftClaims::new("nft_claims");

// Hooks to contracts that will receive staking and unstaking
// messages.
pub const HOOKS: Hooks = Hooks::new("hooks");

pub fn register_staked_nft(
    storage: &mut dyn Storage,
    height: u64,
    staker: &Addr,
    token_id: &String,
) -> StdResult<()> {
    let add_one = |prev: Option<Uint128>| -> StdResult<Uint128> {
        prev.unwrap_or_default()
            .checked_add(Uint128::new(1))
            .map_err(StdError::overflow)
    };

    STAKED_NFTS_PER_OWNER.save(storage, (staker, token_id), &Empty::default())?;
    NFT_BALANCES.update(storage, staker, height, add_one)?;
    TOTAL_STAKED_NFTS
        .update(storage, height, add_one)
        .map(|_| ())
}

pub fn register_unstaked_nft(
    storage: &mut dyn Storage,
    height: u64,
    staker: &Addr,
    token_ids: &[String],
) -> Result<(), ContractError> {
    let subtractor = |amount: u128| {
        move |prev: Option<Uint128>| -> StdResult<Uint128> {
            prev.expect("unstaking that which was not staked")
                .checked_sub(Uint128::new(amount))
                .map_err(StdError::overflow)
        }
    };

    for token in token_ids {
        let key = (staker, token.as_str());
        if STAKED_NFTS_PER_OWNER.has(storage, key) {
            STAKED_NFTS_PER_OWNER.remove(storage, key);
        } else {
            return Err(ContractError::NotStaked {
                token_id: token.clone(),
            });
        }
    }

    // invariant: token_ids has unique values. for loop asserts this.

    let sub_n = subtractor(token_ids.len() as u128);
    TOTAL_STAKED_NFTS.update(storage, height, sub_n)?;
    NFT_BALANCES.update(storage, staker, height, sub_n)?;
    Ok(())
}
