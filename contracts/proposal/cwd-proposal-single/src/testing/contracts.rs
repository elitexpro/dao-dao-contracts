use cosmwasm_std::Empty;

use cw_multi_test::{Contract, ContractWrapper};
use cwd_pre_propose_single as cppbps;

pub(crate) fn cw20_base_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        cw20_base::contract::execute,
        cw20_base::contract::instantiate,
        cw20_base::contract::query,
    );
    Box::new(contract)
}

pub(crate) fn cw4_group_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        cw4_group::contract::execute,
        cw4_group::contract::instantiate,
        cw4_group::contract::query,
    );
    Box::new(contract)
}

pub(crate) fn cw721_base_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        cw721_base::entry::execute,
        cw721_base::entry::instantiate,
        cw721_base::entry::query,
    );
    Box::new(contract)
}

pub(crate) fn cw20_stake_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        cw20_stake::contract::execute,
        cw20_stake::contract::instantiate,
        cw20_stake::contract::query,
    );
    Box::new(contract)
}

pub(crate) fn v1_proposal_single_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        cw_proposal_single_v1::contract::execute,
        cw_proposal_single_v1::contract::instantiate,
        cw_proposal_single_v1::contract::query,
    )
    .with_reply(cw_proposal_single_v1::contract::reply)
    .with_migrate(cw_proposal_single_v1::contract::migrate);
    Box::new(contract)
}

pub(crate) fn proposal_single_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        crate::contract::execute,
        crate::contract::instantiate,
        crate::contract::query,
    )
    .with_reply(crate::contract::reply)
    .with_migrate(crate::contract::migrate);
    Box::new(contract)
}

pub(crate) fn pre_propose_single_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        cppbps::contract::execute,
        cppbps::contract::instantiate,
        cppbps::contract::query,
    );
    Box::new(contract)
}

pub(crate) fn cw20_staked_balances_voting_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        cwd_voting_cw20_staked::contract::execute,
        cwd_voting_cw20_staked::contract::instantiate,
        cwd_voting_cw20_staked::contract::query,
    )
    .with_reply(cwd_voting_cw20_staked::contract::reply);
    Box::new(contract)
}

pub(crate) fn native_staked_balances_voting_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        cwd_voting_native_staked::contract::execute,
        cwd_voting_native_staked::contract::instantiate,
        cwd_voting_native_staked::contract::query,
    );
    Box::new(contract)
}

pub(crate) fn cw721_stake_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        cwd_voting_cw721_staked::contract::execute,
        cwd_voting_cw721_staked::contract::instantiate,
        cwd_voting_cw721_staked::contract::query,
    );
    Box::new(contract)
}

pub(crate) fn cw_core_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        cwd_core::contract::execute,
        cwd_core::contract::instantiate,
        cwd_core::contract::query,
    )
    .with_reply(cwd_core::contract::reply);
    Box::new(contract)
}

pub(crate) fn cw4_voting_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        cwd_voting_cw4::contract::execute,
        cwd_voting_cw4::contract::instantiate,
        cwd_voting_cw4::contract::query,
    )
    .with_reply(cwd_voting_cw4::contract::reply);
    Box::new(contract)
}
