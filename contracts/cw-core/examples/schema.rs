use std::env::current_dir;
use std::fs::create_dir_all;

use cosmwasm_schema::{export_schema, export_schema_with_title, remove_schemas, schema_for};
use cosmwasm_std::Addr;
use cw_core::{
    msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg},
    query::{
        AdminNominationResponse, Cw20BalanceResponse, DumpStateResponse, GetItemResponse,
        PauseInfoResponse, SubDao,
    },
    state::{Config, ProposalModule},
};
use cw_core_interface::voting::{
    InfoResponse, TotalPowerAtHeightResponse, VotingPowerAtHeightResponse,
};

fn main() {
    let mut out_dir = current_dir().unwrap();
    out_dir.push("schema");
    create_dir_all(&out_dir).unwrap();
    remove_schemas(&out_dir).unwrap();

    export_schema(&schema_for!(InstantiateMsg), &out_dir);
    export_schema(&schema_for!(ExecuteMsg), &out_dir);
    export_schema(&schema_for!(QueryMsg), &out_dir);
    export_schema(&schema_for!(MigrateMsg), &out_dir);

    export_schema(&schema_for!(DumpStateResponse), &out_dir);
    export_schema(&schema_for!(PauseInfoResponse), &out_dir);
    export_schema(&schema_for!(GetItemResponse), &out_dir);
    export_schema(&schema_for!(InfoResponse), &out_dir);
    export_schema(&schema_for!(TotalPowerAtHeightResponse), &out_dir);
    export_schema(&schema_for!(VotingPowerAtHeightResponse), &out_dir);
    export_schema(&schema_for!(AdminNominationResponse), &out_dir);
    export_schema(&schema_for!(SubDao), &out_dir);

    // Auto TS code generation expects the query return type as QueryNameResponse
    // Here we map query responses to the correct name
    export_schema_with_title(&schema_for!(Option<Addr>), &out_dir, "AdminResponse");
    export_schema_with_title(&schema_for!(Option<String>), &out_dir, "DaoURIResponse");
    export_schema_with_title(&schema_for!(Config), &out_dir, "ConfigResponse");
    export_schema_with_title(
        &schema_for!(Cw20BalanceResponse),
        &out_dir,
        "Cw20BalancesResponse",
    );
    export_schema_with_title(&schema_for!(Vec<Addr>), &out_dir, "Cw20TokenListResponse");
    export_schema_with_title(&schema_for!(Vec<Addr>), &out_dir, "Cw721TokenListResponse");
    export_schema_with_title(&schema_for!(Vec<String>), &out_dir, "ListItemsResponse");
    export_schema_with_title(&schema_for!(Addr), &out_dir, "VotingModuleResponse");
    export_schema_with_title(
        &schema_for!(Vec<ProposalModule>),
        &out_dir,
        "ProposalModulesResponse",
    );
    export_schema_with_title(
        &schema_for!(Vec<ProposalModule>),
        &out_dir,
        "ActiveProposalModulesResponse",
    );
    export_schema_with_title(&schema_for!(Vec<SubDao>), &out_dir, "ListSubDaosResponse");
}
