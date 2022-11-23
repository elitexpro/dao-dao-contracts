use cosmwasm_schema::{cw_serde, QueryResponses};
use cwd_macros::voting_module_query;

#[cw_serde]
pub struct InstantiateMsg {
    pub cw4_group_code_id: u64,
    pub initial_members: Vec<cw4::Member>,
}

#[cw_serde]
pub enum ExecuteMsg {
    MemberChangedHook { diffs: Vec<cw4::MemberDiff> },
}

#[voting_module_query]
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(cosmwasm_std::Addr)]
    GroupContract {},
}

#[cw_serde]
pub struct MigrateMsg {}
