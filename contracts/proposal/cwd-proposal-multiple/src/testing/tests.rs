use cosmwasm_std::{to_binary, Addr, Coin, CosmosMsg, Decimal, Empty, Timestamp, Uint128, WasmMsg};
use cw20::Cw20Coin;
use cw_denom::{CheckedDenom, UncheckedDenom};
use cw_multi_test::{next_block, App, BankSudo, Contract, ContractWrapper, Executor, SudoMsg};
use cw_utils::Duration;
use cwd_core::state::ProposalModule;
use cwd_hooks::HooksResponse;
use cwd_interface::{Admin, ModuleInstantiateInfo};
use cwd_voting::{
    deposit::{CheckedDepositInfo, DepositRefundPolicy, DepositToken, UncheckedDepositInfo},
    multiple_choice::{
        CheckedMultipleChoiceOption, MultipleChoiceOption, MultipleChoiceOptionType,
        MultipleChoiceOptions, MultipleChoiceVote, MultipleChoiceVotes, VotingStrategy,
        MAX_NUM_CHOICES,
    },
    pre_propose::PreProposeInfo,
    status::Status,
    threshold::{PercentageThreshold, Threshold},
};
use cwd_voting_cw20_staked::msg::ActiveThreshold;
use std::panic;

use crate::{
    msg::{ExecuteMsg, InstantiateMsg, QueryMsg},
    proposal::MultipleChoiceProposal,
    query::{ProposalListResponse, ProposalResponse, VoteInfo, VoteListResponse},
    state::Config,
    testing::{
        do_votes::do_test_votes_cw20_balances,
        execute::make_proposal,
        instantiate::{
            instantiate_with_cw20_balances_governance, instantiate_with_staked_balances_governance,
            instantiate_with_staking_active_threshold,
        },
        queries::{
            query_balance_cw20, query_balance_native, query_dao_token,
            query_deposit_config_and_pre_propose_module, query_list_proposals,
            query_list_proposals_reverse, query_multiple_proposal_module, query_proposal,
            query_proposal_config, query_proposal_hooks, query_vote_hooks,
        },
    },
    ContractError,
};
use cwd_pre_propose_multiple as cppm;

use cwd_testing::{
    contracts::{cw20_balances_voting_contract, cw20_base_contract},
    ShouldExecute,
};

pub const CREATOR_ADDR: &str = "creator";
pub const ALTERNATIVE_ADDR: &str = "alternative";

pub struct TestMultipleChoiceVote {
    /// The address casting the vote.
    pub voter: String,
    /// Position on the vote.
    pub position: MultipleChoiceVote,
    /// Voting power of the address.
    pub weight: Uint128,
    /// If this vote is expected to execute.
    pub should_execute: ShouldExecute,
}

pub fn proposal_multiple_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        crate::contract::execute,
        crate::contract::instantiate,
        crate::contract::query,
    )
    .with_reply(crate::contract::reply);
    Box::new(contract)
}

pub fn pre_propose_multiple_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        cppm::contract::execute,
        cppm::contract::instantiate,
        cppm::contract::query,
    );
    Box::new(contract)
}

pub fn get_pre_propose_info(
    app: &mut App,
    deposit_info: Option<UncheckedDepositInfo>,
    open_proposal_submission: bool,
) -> PreProposeInfo {
    let pre_propose_contract = app.store_code(pre_propose_multiple_contract());
    PreProposeInfo::ModuleMayPropose {
        info: ModuleInstantiateInfo {
            code_id: pre_propose_contract,
            msg: to_binary(&cppm::InstantiateMsg {
                deposit_info,
                open_proposal_submission,
                extension: Empty::default(),
            })
            .unwrap(),
            admin: Some(Admin::CoreModule {}),
            label: "pre_propose_contract".to_string(),
        },
    }
}

#[test]
fn test_propose() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());

    let max_voting_period = cw_utils::Duration::Height(6);
    let quorum = PercentageThreshold::Majority {};

    let voting_strategy = VotingStrategy::SingleChoice { quorum };

    let instantiate = InstantiateMsg {
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        voting_strategy: voting_strategy.clone(),
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };

    let core_addr = instantiate_with_staked_balances_governance(&mut app, instantiate, None);
    let govmod = query_multiple_proposal_module(&app, &core_addr);

    // Check that the config has been configured correctly.
    let config: Config = query_proposal_config(&app, &govmod);
    let expected = Config {
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        dao: core_addr,
        voting_strategy: voting_strategy.clone(),
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
    };
    assert_eq!(config, expected);

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];

    let mc_options = MultipleChoiceOptions { options };

    // Create a new proposal.
    make_proposal(&mut app, &govmod, CREATOR_ADDR, mc_options.clone());

    let created: ProposalResponse = query_proposal(&app, &govmod, 1);

    let current_block = app.block_info();
    let checked_options = mc_options.into_checked().unwrap();
    let expected = MultipleChoiceProposal {
        title: "title".to_string(),
        description: "description".to_string(),
        proposer: Addr::unchecked(CREATOR_ADDR),
        start_height: current_block.height,
        expiration: max_voting_period.after(&current_block),
        choices: checked_options.options,
        status: Status::Open,
        voting_strategy,
        total_power: Uint128::new(100_000_000),
        votes: MultipleChoiceVotes {
            vote_weights: vec![Uint128::zero(); 3],
        },
        allow_revoting: false,
        min_voting_period: None,
    };

    assert_eq!(created.proposal, expected);
    assert_eq!(created.id, 1u64);
}

#[test]
fn test_propose_wrong_num_choices() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());

    let max_voting_period = cw_utils::Duration::Height(6);
    let quorum = PercentageThreshold::Majority {};

    let voting_strategy = VotingStrategy::SingleChoice { quorum };

    let instantiate = InstantiateMsg {
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        voting_strategy: voting_strategy.clone(),
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };

    let core_addr = instantiate_with_staked_balances_governance(&mut app, instantiate, None);
    let govmod = query_multiple_proposal_module(&app, &core_addr);

    // Check that the config has been configured correctly.
    let config: Config = query_proposal_config(&app, &govmod);
    let expected = Config {
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        dao: core_addr,
        voting_strategy,
    };
    assert_eq!(config, expected);

    let options = vec![];

    // Create a proposal with less than min choices.
    let mc_options = MultipleChoiceOptions { options };
    let err = app.execute_contract(
        Addr::unchecked(CREATOR_ADDR),
        govmod.clone(),
        &ExecuteMsg::Propose {
            title: "A simple text proposal".to_string(),
            description: "A simple text proposal".to_string(),
            choices: mc_options,
            proposer: None,
        },
        &[],
    );
    assert!(err.is_err());

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        };
        std::convert::TryInto::try_into(MAX_NUM_CHOICES + 1).unwrap()
    ];

    // Create proposal with more than max choices.

    let mc_options = MultipleChoiceOptions { options };
    // Create a new proposal.
    let err = app.execute_contract(
        Addr::unchecked(CREATOR_ADDR),
        govmod,
        &ExecuteMsg::Propose {
            title: "A simple text proposal".to_string(),
            description: "A simple text proposal".to_string(),
            choices: mc_options,
            proposer: None,
        },
        &[],
    );
    assert!(err.is_err());
}

#[test]
fn test_proposal_count_initialized_to_zero() {
    let mut app = App::default();
    let _proposal_id = app.store_code(proposal_multiple_contract());
    let msg = InstantiateMsg {
        voting_strategy: VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Percent(Decimal::percent(10)),
        },
        max_voting_period: Duration::Height(10),
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        only_members_execute: true,
        allow_revoting: false,
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };
    let core_addr = instantiate_with_staked_balances_governance(&mut app, msg, None);

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let proposal_modules = gov_state.proposal_modules;

    assert_eq!(proposal_modules.len(), 1);
    let govmod = proposal_modules.into_iter().next().unwrap().address;

    let proposal_count: u64 = app
        .wrap()
        .query_wasm_smart(govmod, &QueryMsg::ProposalCount {})
        .unwrap();

    assert_eq!(proposal_count, 0);
}

#[test]
fn test_no_early_pass_with_min_duration() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let msg = InstantiateMsg {
        voting_strategy: VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Percent(Decimal::percent(10)),
        },
        max_voting_period: Duration::Height(10),
        min_voting_period: Some(Duration::Height(2)),
        only_members_execute: true,
        allow_revoting: false,
        close_proposal_on_execution_failure: true,
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };

    let core_addr = instantiate_with_staked_balances_governance(
        &mut app,
        msg,
        Some(vec![
            Cw20Coin {
                address: "blue".to_string(),
                amount: Uint128::new(10),
            },
            Cw20Coin {
                address: "whale".to_string(),
                amount: Uint128::new(90),
            },
        ]),
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let proposal_modules = gov_state.proposal_modules;

    assert_eq!(proposal_modules.len(), 1);
    let govmod = proposal_modules.into_iter().next().unwrap().address;

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];

    let mc_options = MultipleChoiceOptions { options };

    app.execute_contract(
        Addr::unchecked("whale"),
        govmod.clone(),
        &ExecuteMsg::Propose {
            title: "A simple text proposal".to_string(),
            description: "This is a simple text proposal".to_string(),
            choices: mc_options,
            proposer: None,
        },
        &[],
    )
    .unwrap();

    // Whale votes which under normal curcumstances would cause the
    // proposal to pass. Because there is a min duration it does not.
    app.execute_contract(
        Addr::unchecked("whale"),
        govmod.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap();

    let proposal: ProposalResponse = query_proposal(&app, &govmod, 1);

    assert_eq!(proposal.proposal.status, Status::Open);

    // Let the min voting period pass.
    app.update_block(|b| b.height += 2);

    let proposal: ProposalResponse = query_proposal(&app, &govmod, 1);

    assert_eq!(proposal.proposal.status, Status::Passed);
}

#[test]
fn test_propose_with_messages() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let msg = InstantiateMsg {
        voting_strategy: VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Percent(Decimal::percent(10)),
        },
        max_voting_period: Duration::Height(10),
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        only_members_execute: true,
        allow_revoting: false,
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };

    let core_addr = instantiate_with_staked_balances_governance(
        &mut app,
        msg,
        Some(vec![
            Cw20Coin {
                address: "blue".to_string(),
                amount: Uint128::new(10),
            },
            Cw20Coin {
                address: "whale".to_string(),
                amount: Uint128::new(90),
            },
        ]),
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let proposal_modules = gov_state.proposal_modules;

    assert_eq!(proposal_modules.len(), 1);
    let govmod = proposal_modules.into_iter().next().unwrap().address;

    let config_msg = ExecuteMsg::UpdateConfig {
        voting_strategy: VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {},
        },
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period: cw_utils::Duration::Height(20),
        only_members_execute: false,
        allow_revoting: false,
        dao: "dao".to_string(),
    };

    let wasm_msg = WasmMsg::Execute {
        contract_addr: govmod.to_string(),
        msg: to_binary(&config_msg).unwrap(),
        funds: vec![],
    };

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![CosmosMsg::Wasm(wasm_msg)],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];

    let mc_options = MultipleChoiceOptions { options };

    app.execute_contract(
        Addr::unchecked("whale"),
        govmod.clone(),
        &ExecuteMsg::Propose {
            title: "A simple text proposal".to_string(),
            description: "This is a simple text proposal".to_string(),
            choices: mc_options,
            proposer: None,
        },
        &[],
    )
    .unwrap();

    app.execute_contract(
        Addr::unchecked("whale"),
        govmod.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap();

    let proposal: ProposalResponse = query_proposal(&app, &govmod, 1);

    assert_eq!(proposal.proposal.status, Status::Passed);

    // Execute the proposal and messages
    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Execute { proposal_id: 1 },
        &[],
    )
    .unwrap();

    // Check that config was updated by proposal message
    let config: Config = query_proposal_config(&app, &govmod);
    assert_eq!(config.max_voting_period, Duration::Height(20))
}

#[test]
#[should_panic(
    expected = "min_voting_period and max_voting_period must have the same units (height or time)"
)]
fn test_min_duration_units_missmatch() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let msg = InstantiateMsg {
        voting_strategy: VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Percent(Decimal::percent(10)),
        },
        max_voting_period: Duration::Height(10),
        min_voting_period: Some(Duration::Time(2)),
        only_members_execute: true,
        allow_revoting: false,
        close_proposal_on_execution_failure: true,
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };
    instantiate_with_staked_balances_governance(
        &mut app,
        msg,
        Some(vec![
            Cw20Coin {
                address: "blue".to_string(),
                amount: Uint128::new(10),
            },
            Cw20Coin {
                address: "wale".to_string(),
                amount: Uint128::new(90),
            },
        ]),
    );
}

#[test]
#[should_panic(expected = "Min voting period must be less than or equal to max voting period")]
fn test_min_duration_larger_than_proposal_duration() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let msg = InstantiateMsg {
        voting_strategy: VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Percent(Decimal::percent(10)),
        },
        max_voting_period: Duration::Height(10),
        min_voting_period: Some(Duration::Height(11)),
        only_members_execute: true,
        allow_revoting: false,
        close_proposal_on_execution_failure: true,
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };
    instantiate_with_staked_balances_governance(
        &mut app,
        msg,
        Some(vec![
            Cw20Coin {
                address: "blue".to_string(),
                amount: Uint128::new(10),
            },
            Cw20Coin {
                address: "wale".to_string(),
                amount: Uint128::new(90),
            },
        ]),
    );
}

#[test]
fn test_min_duration_same_as_proposal_duration() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let msg = InstantiateMsg {
        voting_strategy: VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Percent(Decimal::percent(10)),
        },
        max_voting_period: Duration::Time(10),
        min_voting_period: Some(Duration::Time(10)),
        only_members_execute: true,
        allow_revoting: false,
        close_proposal_on_execution_failure: true,
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };

    let core_addr = instantiate_with_staked_balances_governance(
        &mut app,
        msg,
        Some(vec![
            Cw20Coin {
                address: "blue".to_string(),
                amount: Uint128::new(10),
            },
            Cw20Coin {
                address: "whale".to_string(),
                amount: Uint128::new(90),
            },
        ]),
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let proposal_modules = gov_state.proposal_modules;

    assert_eq!(proposal_modules.len(), 1);
    let govmod = proposal_modules.into_iter().next().unwrap().address;

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];

    let mc_options = MultipleChoiceOptions { options };

    app.execute_contract(
        Addr::unchecked("whale"),
        govmod.clone(),
        &ExecuteMsg::Propose {
            title: "A simple text proposal".to_string(),
            description: "This is a simple text proposal".to_string(),
            choices: mc_options,
            proposer: None,
        },
        &[],
    )
    .unwrap();

    // Whale votes which under normal curcumstances would cause the
    // proposal to pass. Because there is a min duration it does not.
    app.execute_contract(
        Addr::unchecked("whale"),
        govmod.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap();

    let proposal: ProposalResponse = query_proposal(&app, &govmod, 1);

    assert_eq!(proposal.proposal.status, Status::Open);

    // someone else can vote none of the above.
    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 2 },
        },
        &[],
    )
    .unwrap();

    // Let the min voting period pass.
    app.update_block(|b| b.time = b.time.plus_seconds(10));

    let proposal: ProposalResponse = query_proposal(&app, &govmod, 1);

    assert_eq!(proposal.proposal.status, Status::Passed);
}

/// Instantiate the contract and use the voting module's token
/// contract as the proposal deposit token.
#[test]
fn test_voting_module_token_proposal_deposit_instantiate() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());

    let quorum = PercentageThreshold::Majority {};
    let voting_strategy = VotingStrategy::SingleChoice { quorum };
    let max_voting_period = cw_utils::Duration::Height(6);

    let instantiate = InstantiateMsg {
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        voting_strategy,
        pre_propose_info: get_pre_propose_info(
            &mut app,
            Some(UncheckedDepositInfo {
                denom: DepositToken::VotingModuleToken {},
                amount: Uint128::new(1),
                refund_policy: DepositRefundPolicy::OnlyPassed,
            }),
            false,
        ),
    };

    let core_addr = instantiate_with_staked_balances_governance(&mut app, instantiate, None);

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr.clone(), &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let governance_modules = gov_state.proposal_modules;

    assert_eq!(governance_modules.len(), 1);
    let govmod = governance_modules.into_iter().next().unwrap().address;

    let token = query_dao_token(&app, &core_addr);

    let (deposit_config, _) = query_deposit_config_and_pre_propose_module(&app, &govmod);
    assert_eq!(
        deposit_config.deposit_info,
        Some(CheckedDepositInfo {
            denom: CheckedDenom::Cw20(token),
            amount: Uint128::new(1),
            refund_policy: DepositRefundPolicy::OnlyPassed
        })
    )
}

// Instantiate the contract and use a cw20 unrealated to the voting
// module for the proposal deposit.
#[test]
fn test_different_token_proposal_deposit() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let cw20_id = app.store_code(cw20_base_contract());
    let cw20_addr = app
        .instantiate_contract(
            cw20_id,
            Addr::unchecked(CREATOR_ADDR),
            &cw20_base::msg::InstantiateMsg {
                name: "OAD OAD".to_string(),
                symbol: "OAD".to_string(),
                decimals: 6,
                initial_balances: vec![],
                mint: None,
                marketing: None,
            },
            &[],
            "random-cw20",
            None,
        )
        .unwrap();

    let quorum = PercentageThreshold::Percent(Decimal::percent(10));
    let voting_strategy = VotingStrategy::SingleChoice { quorum };
    let max_voting_period = cw_utils::Duration::Height(6);
    let instantiate = InstantiateMsg {
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        voting_strategy,
        pre_propose_info: get_pre_propose_info(
            &mut app,
            Some(UncheckedDepositInfo {
                denom: DepositToken::Token {
                    denom: UncheckedDenom::Cw20(cw20_addr.to_string()),
                },
                amount: Uint128::new(1),
                refund_policy: DepositRefundPolicy::OnlyPassed,
            }),
            false,
        ),
    };

    instantiate_with_staked_balances_governance(&mut app, instantiate, None);
}

/// Try to instantiate the governance module with a non-cw20 as its
/// proposal deposit token. This should error as the `TokenInfo {}`
/// query ought to fail.
#[test]
#[should_panic(expected = "Error parsing into type cwd_voting_cw20_balance::msg::QueryMsg")]
fn test_bad_token_proposal_deposit() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let cw20_id = app.store_code(cw20_base_contract());
    let votemod_id = app.store_code(cw20_balances_voting_contract());

    let votemod_addr = app
        .instantiate_contract(
            votemod_id,
            Addr::unchecked(CREATOR_ADDR),
            &cwd_voting_cw20_balance::msg::InstantiateMsg {
                token_info: cwd_voting_cw20_balance::msg::TokenInfo::New {
                    code_id: cw20_id,
                    label: "DAO DAO governance token".to_string(),
                    name: "DAO".to_string(),
                    symbol: "DAO".to_string(),
                    decimals: 6,
                    initial_balances: vec![Cw20Coin {
                        address: CREATOR_ADDR.to_string(),
                        amount: Uint128::new(1),
                    }],
                    marketing: None,
                },
            },
            &[],
            "random-vote-module",
            None,
        )
        .unwrap();

    let quorum = PercentageThreshold::Percent(Decimal::percent(10));
    let voting_strategy = VotingStrategy::SingleChoice { quorum };
    let max_voting_period = cw_utils::Duration::Height(6);
    let instantiate = InstantiateMsg {
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        voting_strategy,
        pre_propose_info: get_pre_propose_info(
            &mut app,
            Some(UncheckedDepositInfo {
                denom: DepositToken::Token {
                    denom: UncheckedDenom::Cw20(votemod_addr.to_string()),
                },
                amount: Uint128::new(1),
                refund_policy: DepositRefundPolicy::OnlyPassed,
            }),
            false,
        ),
    };

    instantiate_with_staked_balances_governance(&mut app, instantiate, None);
}

#[test]
fn test_take_proposal_deposit() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());

    let quorum = PercentageThreshold::Percent(Decimal::percent(10));
    let voting_strategy = VotingStrategy::SingleChoice { quorum };
    let max_voting_period = cw_utils::Duration::Height(6);

    let instantiate = InstantiateMsg {
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        voting_strategy,
        pre_propose_info: get_pre_propose_info(
            &mut app,
            Some(UncheckedDepositInfo {
                denom: DepositToken::VotingModuleToken {},
                amount: Uint128::new(1),
                refund_policy: DepositRefundPolicy::OnlyPassed,
            }),
            false,
        ),
    };

    let core_addr = instantiate_with_cw20_balances_governance(
        &mut app,
        instantiate,
        Some(vec![Cw20Coin {
            address: "blue".to_string(),
            amount: Uint128::new(2),
        }]),
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let governance_modules = gov_state.proposal_modules;

    assert_eq!(governance_modules.len(), 1);
    let govmod = governance_modules.into_iter().next().unwrap().address;

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];

    let mc_options = MultipleChoiceOptions { options };

    let (deposit_config, pre_propose_module) =
        query_deposit_config_and_pre_propose_module(&app, &govmod);
    if let CheckedDepositInfo {
        denom: CheckedDenom::Cw20(ref token),
        ..
    } = deposit_config.deposit_info.unwrap()
    {
        app.execute_contract(
            Addr::unchecked("blue"),
            pre_propose_module,
            &cppm::ExecuteMsg::Propose {
                msg: cppm::ProposeMessage::Propose {
                    title: "title".to_string(),
                    description: "description".to_string(),
                    choices: mc_options.clone(),
                },
            },
            &[],
        )
        .unwrap_err();

        // Allow a proposal deposit.
        app.execute_contract(
            Addr::unchecked("blue"),
            Addr::unchecked(token),
            &cw20_base::msg::ExecuteMsg::IncreaseAllowance {
                spender: govmod.to_string(),
                amount: Uint128::new(1),
                expires: None,
            },
            &[],
        )
        .unwrap();

        make_proposal(&mut app, &govmod, "blue", mc_options);

        // Proposal has been executed so deposit has been refunded.
        let balance = query_balance_cw20(&app, token, "blue".to_string());
        assert_eq!(balance, Uint128::new(1));
    } else {
        panic!()
    };
}

#[test]
fn test_native_proposal_deposit() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());

    let max_voting_period = cw_utils::Duration::Height(6);
    let instantiate = InstantiateMsg {
        voting_strategy: VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Percent(Decimal::percent(100)),
        },
        max_voting_period,
        min_voting_period: None,
        only_members_execute: false,
        allow_revoting: false,
        close_proposal_on_execution_failure: true,
        pre_propose_info: get_pre_propose_info(
            &mut app,
            Some(UncheckedDepositInfo {
                denom: DepositToken::Token {
                    denom: UncheckedDenom::Native("ujuno".to_string()),
                },
                amount: Uint128::new(1),
                refund_policy: DepositRefundPolicy::Always,
            }),
            false,
        ),
    };

    let core_addr = instantiate_with_staked_balances_governance(
        &mut app,
        instantiate,
        Some(vec![Cw20Coin {
            address: "blue".to_string(),
            amount: Uint128::new(2),
        }]),
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let governance_modules = gov_state.proposal_modules;

    assert_eq!(governance_modules.len(), 1);
    let govmod = governance_modules.into_iter().next().unwrap().address;

    let (deposit_config, pre_propose_module) =
        query_deposit_config_and_pre_propose_module(&app, &govmod);
    if let CheckedDepositInfo {
        denom: CheckedDenom::Native(ref _token),
        refund_policy,
        ..
    } = deposit_config.deposit_info.unwrap()
    {
        assert_eq!(refund_policy, DepositRefundPolicy::Always);

        let mc_options = MultipleChoiceOptions {
            options: vec![
                MultipleChoiceOption {
                    description: "multiple choice option 1".to_string(),
                    msgs: vec![],
                    title: "title".to_string(),
                },
                MultipleChoiceOption {
                    description: "multiple choice option 2".to_string(),
                    msgs: vec![],
                    title: "title".to_string(),
                },
            ],
        };

        // This will fail because deposit not send
        app.execute_contract(
            Addr::unchecked("blue"),
            pre_propose_module.clone(),
            &cppm::ExecuteMsg::Propose {
                msg: cppm::ProposeMessage::Propose {
                    title: "title".to_string(),
                    description: "description".to_string(),
                    choices: mc_options.clone(),
                },
            },
            &[],
        )
        .unwrap_err();

        // Mint blue some tokens
        app.sudo(SudoMsg::Bank(BankSudo::Mint {
            to_address: "blue".to_string(),
            amount: vec![Coin {
                denom: "ujuno".to_string(),
                amount: Uint128::new(100),
            }],
        }))
        .unwrap();

        // Adding deposit will work
        make_proposal(&mut app, &govmod, "blue", mc_options);

        // "blue" has been refunded
        let balance = query_balance_native(&app, "blue", "ujuno");
        assert_eq!(balance, Uint128::new(99));

        // Govmod has refunded the token
        let balance = query_balance_native(&app, pre_propose_module.as_str(), "ujuno");
        assert_eq!(balance, Uint128::new(1));

        // Vote on the proposal.
        let res = app.execute_contract(
            Addr::unchecked("blue"),
            govmod.clone(),
            &ExecuteMsg::Vote {
                proposal_id: 1,
                vote: MultipleChoiceVote { option_id: 1 },
            },
            &[],
        );
        assert!(res.is_ok());

        // Execute the proposal, this should cause the deposit to be
        // refunded.
        app.execute_contract(
            Addr::unchecked("blue"),
            govmod.clone(),
            &ExecuteMsg::Execute { proposal_id: 1 },
            &[],
        )
        .unwrap();

        // "blue" has been refunded
        let balance = query_balance_native(&app, "blue", "ujuno");
        assert_eq!(balance, Uint128::new(100));

        // Govmod has refunded the token
        let balance = query_balance_native(&app, pre_propose_module.as_str(), "ujuno");
        assert_eq!(balance, Uint128::new(0));
    } else {
        panic!()
    };
}

#[test]
fn test_deposit_return_on_execute() {
    // Will create a proposal and execute it, one token will be
    // deposited to create said proposal, expectation is that the
    // token is then returned once the proposal is executed.
    let (mut app, core_addr) = do_test_votes_cw20_balances(
        vec![TestMultipleChoiceVote {
            voter: "blue".to_string(),
            position: MultipleChoiceVote { option_id: 0 },
            weight: Uint128::new(10),
            should_execute: ShouldExecute::Yes,
        }],
        VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {},
        },
        Status::Passed,
        None,
        Some(UncheckedDepositInfo {
            denom: DepositToken::VotingModuleToken {},
            amount: Uint128::new(1),
            refund_policy: DepositRefundPolicy::OnlyPassed,
        }),
        true,
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let governance_modules = gov_state.proposal_modules;

    assert_eq!(governance_modules.len(), 1);
    let govmod = governance_modules.into_iter().next().unwrap().address;

    // Ger deposit info
    let (deposit_config, _) = query_deposit_config_and_pre_propose_module(&app, &govmod);
    if let CheckedDepositInfo {
        denom: CheckedDenom::Cw20(ref token),
        ..
    } = deposit_config.deposit_info.unwrap()
    {
        // Proposal has not been executed so deposit has not been refunded.
        let balance = query_balance_cw20(&app, token, "blue".to_string());
        assert_eq!(balance, Uint128::new(9));

        // Execute the proposal, this should cause the deposit to be
        // refunded.
        app.execute_contract(
            Addr::unchecked("blue"),
            govmod,
            &ExecuteMsg::Execute { proposal_id: 1 },
            &[],
        )
        .unwrap();

        // Proposal has been executed so deposit has been refunded.
        let balance = query_balance_cw20(&app, token, "blue".to_string());
        assert_eq!(balance, Uint128::new(10));
    } else {
        panic!()
    };
}

#[test]
fn test_deposit_return_zero() {
    // Test that balance does not change when deposit is zero.
    let (mut app, core_addr) = do_test_votes_cw20_balances(
        vec![TestMultipleChoiceVote {
            voter: "blue".to_string(),
            position: MultipleChoiceVote { option_id: 0 },
            weight: Uint128::new(10),
            should_execute: ShouldExecute::Yes,
        }],
        VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {},
        },
        Status::Passed,
        None,
        None,
        true,
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr.clone(), &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let governance_modules = gov_state.proposal_modules;

    assert_eq!(governance_modules.len(), 1);
    let govmod = governance_modules.into_iter().next().unwrap().address;

    let token = query_dao_token(&app, &core_addr);

    // Execute the proposal, this should cause the deposit to be
    // refunded.
    app.execute_contract(
        Addr::unchecked("blue"),
        govmod,
        &ExecuteMsg::Execute { proposal_id: 1 },
        &[],
    )
    .unwrap();

    // Proposal has been executed so deposit has been refunded.
    let balance = query_balance_cw20(&app, token, "blue".to_string());
    assert_eq!(balance, Uint128::new(10));
}

#[test]
fn test_query_list_votes() {
    let (app, core_addr) = do_test_votes_cw20_balances(
        vec![
            TestMultipleChoiceVote {
                voter: "blue".to_string(),
                position: MultipleChoiceVote { option_id: 0 },
                weight: Uint128::new(10),
                should_execute: ShouldExecute::Yes,
            },
            TestMultipleChoiceVote {
                voter: "note".to_string(),
                position: MultipleChoiceVote { option_id: 1 },
                weight: Uint128::new(20),
                should_execute: ShouldExecute::Yes,
            },
        ],
        VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {},
        },
        Status::Passed,
        None,
        None,
        true,
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let governance_modules = gov_state.proposal_modules;

    assert_eq!(governance_modules.len(), 1);
    let govmod = governance_modules.into_iter().next().unwrap().address;

    let list_votes: VoteListResponse = app
        .wrap()
        .query_wasm_smart(
            govmod,
            &QueryMsg::ListVotes {
                proposal_id: 1,
                start_after: None,
                limit: None,
            },
        )
        .unwrap();

    let expected = vec![
        VoteInfo {
            voter: Addr::unchecked("blue"),
            vote: MultipleChoiceVote { option_id: 0 },
            power: Uint128::new(10),
        },
        VoteInfo {
            voter: Addr::unchecked("note"),
            vote: MultipleChoiceVote { option_id: 1 },
            power: Uint128::new(20),
        },
    ];

    assert_eq!(list_votes.votes, expected)
}

#[test]
fn test_invalid_quorum() {
    // Create a proposal that will be rejected
    let (_app, _core_addr) = do_test_votes_cw20_balances(
        vec![TestMultipleChoiceVote {
            voter: "blue".to_string(),
            position: MultipleChoiceVote { option_id: 2 },
            weight: Uint128::new(10),
            should_execute: ShouldExecute::Yes,
        }],
        VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Percent(Decimal::from_ratio(1u128, 10u128)),
        },
        Status::Rejected,
        None,
        None,
        true,
    );
}

#[test]
fn test_cant_vote_executed_or_closed() {
    // Create a proposal that will be rejected
    let (mut app, core_addr) = do_test_votes_cw20_balances(
        vec![TestMultipleChoiceVote {
            voter: "blue".to_string(),
            position: MultipleChoiceVote { option_id: 2 },
            weight: Uint128::new(10),
            should_execute: ShouldExecute::Yes,
        }],
        VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {},
        },
        Status::Rejected,
        None,
        None,
        true,
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let governance_modules = gov_state.proposal_modules;

    assert_eq!(governance_modules.len(), 1);
    let govmod = governance_modules.into_iter().next().unwrap().address;

    // Close the proposal
    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Close { proposal_id: 1 },
        &[],
    )
    .unwrap();

    // Try to vote, should error
    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap_err();

    // Create a proposal that will pass
    let (mut app, _core_addr) = do_test_votes_cw20_balances(
        vec![TestMultipleChoiceVote {
            voter: "blue".to_string(),
            position: MultipleChoiceVote { option_id: 0 },
            weight: Uint128::new(10),
            should_execute: ShouldExecute::Yes,
        }],
        VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {},
        },
        Status::Passed,
        None,
        None,
        true,
    );

    // Execute the proposal
    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Execute { proposal_id: 1 },
        &[],
    )
    .unwrap();

    // Try to vote, should error
    app.execute_contract(
        Addr::unchecked("blue"),
        govmod,
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap_err();
}

#[test]
fn test_cant_propose_zero_power() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let quorum = PercentageThreshold::Percent(Decimal::percent(10));
    let voting_strategy = VotingStrategy::SingleChoice { quorum };
    let max_voting_period = cw_utils::Duration::Height(6);
    let instantiate = InstantiateMsg {
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        voting_strategy,
        pre_propose_info: get_pre_propose_info(
            &mut app,
            Some(UncheckedDepositInfo {
                denom: DepositToken::VotingModuleToken {},
                amount: Uint128::new(1),
                refund_policy: DepositRefundPolicy::Always,
            }),
            false,
        ),
    };

    let core_addr = instantiate_with_cw20_balances_governance(
        &mut app,
        instantiate,
        Some(vec![
            Cw20Coin {
                address: "blue".to_string(),
                amount: Uint128::new(1),
            },
            Cw20Coin {
                address: "blue2".to_string(),
                amount: Uint128::new(10),
            },
        ]),
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let proposal_modules = gov_state.proposal_modules;

    assert_eq!(proposal_modules.len(), 1);
    let govmod = proposal_modules.into_iter().next().unwrap().address;

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];

    let mc_options = MultipleChoiceOptions { options };

    let (deposit_config, pre_propose_module) =
        query_deposit_config_and_pre_propose_module(&app, &govmod);
    if let Some(CheckedDepositInfo {
        denom: CheckedDenom::Cw20(ref token),
        amount,
        ..
    }) = deposit_config.deposit_info
    {
        app.execute_contract(
            Addr::unchecked("blue"),
            token.clone(),
            &cw20_base::msg::ExecuteMsg::IncreaseAllowance {
                spender: pre_propose_module.to_string(),
                amount,
                expires: None,
            },
            &[],
        )
        .unwrap();
    }

    // Blue proposes
    app.execute_contract(
        Addr::unchecked("blue"),
        pre_propose_module.clone(),
        &cppm::ExecuteMsg::Propose {
            msg: cppm::ProposeMessage::Propose {
                title: "A simple text proposal".to_string(),
                description: "A simple text proposal".to_string(),
                choices: mc_options.clone(),
            },
        },
        &[],
    )
    .unwrap();

    // Should fail as blue's balance is now 0
    let err = app.execute_contract(
        Addr::unchecked("blue"),
        pre_propose_module,
        &cppm::ExecuteMsg::Propose {
            msg: cppm::ProposeMessage::Propose {
                title: "A simple text proposal".to_string(),
                description: "A simple text proposal".to_string(),
                choices: mc_options,
            },
        },
        &[],
    );

    assert!(err.is_err())
}

#[test]
fn test_cant_vote_not_registered() {
    let (mut app, core_addr) = do_test_votes_cw20_balances(
        vec![TestMultipleChoiceVote {
            voter: "blue".to_string(),
            position: MultipleChoiceVote { option_id: 2 },
            weight: Uint128::new(10),
            should_execute: ShouldExecute::Yes,
        }],
        VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {},
        },
        Status::Open,
        Some(Uint128::new(100)),
        Some(UncheckedDepositInfo {
            denom: DepositToken::VotingModuleToken {},
            amount: Uint128::new(1),
            refund_policy: DepositRefundPolicy::Always,
        }),
        false,
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let governance_modules = gov_state.proposal_modules;

    assert_eq!(governance_modules.len(), 1);
    let govmod = governance_modules.into_iter().next().unwrap().address;

    // Should error as blue2 is not registered to vote
    let err = app
        .execute_contract(
            Addr::unchecked("blue2"),
            govmod,
            &ExecuteMsg::Vote {
                proposal_id: 1,
                vote: MultipleChoiceVote { option_id: 0 },
            },
            &[],
        )
        .unwrap_err();

    assert!(matches!(
        err.downcast().unwrap(),
        ContractError::NotRegistered {}
    ))
}

#[test]
fn test_cant_execute_not_member() {
    // Create proposal with only_members_execute: true
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());

    let max_voting_period = cw_utils::Duration::Height(6);
    let quorum = PercentageThreshold::Majority {};

    let voting_strategy = VotingStrategy::SingleChoice { quorum };

    let instantiate = InstantiateMsg {
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period,
        only_members_execute: true,
        allow_revoting: false,
        voting_strategy,
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };

    let core_addr = instantiate_with_staked_balances_governance(
        &mut app,
        instantiate,
        Some(vec![Cw20Coin {
            address: "blue".to_string(),
            amount: Uint128::new(10),
        }]),
    );
    let govmod = query_multiple_proposal_module(&app, &core_addr);

    // Create proposal
    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];

    let mc_options = MultipleChoiceOptions { options };

    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Propose {
            title: "A simple text proposal".to_string(),
            description: "A simple text proposal".to_string(),
            choices: mc_options,
            proposer: None,
        },
        &[],
    )
    .unwrap();

    // Proposal should pass after this vote
    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap();

    // Execute should error as blue2 is not a member
    let err = app
        .execute_contract(
            Addr::unchecked("blue2"),
            govmod,
            &ExecuteMsg::Execute { proposal_id: 1 },
            &[],
        )
        .unwrap_err();

    assert!(matches!(
        err.downcast().unwrap(),
        ContractError::Unauthorized {}
    ))
}

#[test]
fn test_open_proposal_submission() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());

    let max_voting_period = cw_utils::Duration::Height(6);

    // Instantiate with open_proposal_submission enabled
    let instantiate = InstantiateMsg {
        voting_strategy: VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Percent(Decimal::percent(100)),
        },
        max_voting_period,
        min_voting_period: None,
        only_members_execute: false,
        allow_revoting: false,
        close_proposal_on_execution_failure: true,
        pre_propose_info: get_pre_propose_info(&mut app, None, true),
    };
    let core_addr = instantiate_with_staked_balances_governance(&mut app, instantiate, None);
    let govmod = query_multiple_proposal_module(&app, &core_addr);

    make_proposal(
        &mut app,
        &govmod,
        "random",
        MultipleChoiceOptions {
            options: vec![
                MultipleChoiceOption {
                    description: "multiple choice option 1".to_string(),
                    msgs: vec![],
                    title: "title".to_string(),
                },
                MultipleChoiceOption {
                    description: "multiple choice option 2".to_string(),
                    msgs: vec![],
                    title: "title".to_string(),
                },
            ],
        },
    );

    let created: ProposalResponse = query_proposal(&app, &govmod, 1);
    let current_block = app.block_info();
    let expected = MultipleChoiceProposal {
        title: "title".to_string(),
        description: "description".to_string(),
        proposer: Addr::unchecked("random"),
        start_height: current_block.height,
        expiration: max_voting_period.after(&current_block),
        min_voting_period: None,
        allow_revoting: false,
        total_power: Uint128::new(100_000_000),
        status: Status::Open,
        voting_strategy: VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Percent(Decimal::percent(100)),
        },
        choices: vec![
            CheckedMultipleChoiceOption {
                description: "multiple choice option 1".to_string(),
                msgs: vec![],
                option_type: MultipleChoiceOptionType::Standard,
                vote_count: Uint128::zero(),
                index: 0,
                title: "title".to_string(),
            },
            CheckedMultipleChoiceOption {
                description: "multiple choice option 2".to_string(),
                msgs: vec![],
                option_type: MultipleChoiceOptionType::Standard,
                vote_count: Uint128::zero(),
                index: 1,
                title: "title".to_string(),
            },
            CheckedMultipleChoiceOption {
                description: "None of the above".to_string(),
                msgs: vec![],
                option_type: MultipleChoiceOptionType::None,
                vote_count: Uint128::zero(),
                index: 2,
                title: "None of the above".to_string(),
            },
        ],
        votes: MultipleChoiceVotes {
            vote_weights: vec![Uint128::zero(); 3],
        },
    };

    assert_eq!(created.proposal, expected);
    assert_eq!(created.id, 1u64);
}

#[test]
fn test_close_open_proposal() {
    let (mut app, core_addr) = do_test_votes_cw20_balances(
        vec![TestMultipleChoiceVote {
            voter: "blue".to_string(),
            position: MultipleChoiceVote { option_id: 2 },
            weight: Uint128::new(10),
            should_execute: ShouldExecute::Yes,
        }],
        VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {},
        },
        Status::Open,
        Some(Uint128::new(100)),
        Some(UncheckedDepositInfo {
            denom: DepositToken::VotingModuleToken {},
            amount: Uint128::new(1),
            refund_policy: DepositRefundPolicy::Always,
        }),
        false,
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let governance_modules = gov_state.proposal_modules;

    assert_eq!(governance_modules.len(), 1);
    let govmod = governance_modules.into_iter().next().unwrap().address;

    // Close the proposal, this should error as the proposal is still
    // open and not expired.
    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Close { proposal_id: 1 },
        &[],
    )
    .unwrap_err();

    // Make the proposal expire.
    app.update_block(|block| block.height += 10);

    // Close the proposal, this should work as the proposal is now
    // open and expired.
    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Close { proposal_id: 1 },
        &[],
    )
    .unwrap();

    let (deposit_config, _) = query_deposit_config_and_pre_propose_module(&app, &govmod);
    if let CheckedDepositInfo {
        denom: CheckedDenom::Cw20(ref token),
        ..
    } = deposit_config.deposit_info.unwrap()
    {
        // Proposal has been executed so deposit has been refunded.
        let balance = query_balance_cw20(&app, token, "blue".to_string());
        assert_eq!(balance, Uint128::new(10));
    } else {
        panic!()
    };
}

#[test]
fn test_no_refund_failed_proposal() {
    let (mut app, core_addr) = do_test_votes_cw20_balances(
        vec![TestMultipleChoiceVote {
            voter: "blue".to_string(),
            position: MultipleChoiceVote { option_id: 2 },
            weight: Uint128::new(10),
            should_execute: ShouldExecute::Yes,
        }],
        VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {},
        },
        Status::Open,
        Some(Uint128::new(100)),
        Some(UncheckedDepositInfo {
            denom: DepositToken::VotingModuleToken {},
            amount: Uint128::new(1),
            refund_policy: DepositRefundPolicy::OnlyPassed,
        }),
        false,
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let governance_modules = gov_state.proposal_modules;

    assert_eq!(governance_modules.len(), 1);
    let govmod = governance_modules.into_iter().next().unwrap().address;

    // Make the proposal expire.
    app.update_block(|block| block.height += 10);

    // Close the proposal, this should work as the proposal is now
    // open and expired.
    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Close { proposal_id: 1 },
        &[],
    )
    .unwrap();

    let (deposit_config, _) = query_deposit_config_and_pre_propose_module(&app, &govmod);
    if let CheckedDepositInfo {
        denom: CheckedDenom::Cw20(ref token),
        ..
    } = deposit_config.deposit_info.unwrap()
    {
        // Proposal has been executed so deposit has been refunded.
        let balance = query_balance_cw20(&app, token, "blue".to_string());
        assert_eq!(balance, Uint128::new(9));
    } else {
        panic!()
    };
}

#[test]
fn test_zero_deposit() {
    do_test_votes_cw20_balances(
        vec![TestMultipleChoiceVote {
            voter: "blue".to_string(),
            position: MultipleChoiceVote { option_id: 0 },
            weight: Uint128::new(10),
            should_execute: ShouldExecute::Yes,
        }],
        VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {},
        },
        Status::Passed,
        None,
        None,
        true,
    );
}

#[test]
fn test_deposit_return_on_close() {
    let quorum = PercentageThreshold::Percent(Decimal::percent(10));
    let voting_strategy = VotingStrategy::SingleChoice { quorum };

    let (mut app, core_addr) = do_test_votes_cw20_balances(
        vec![TestMultipleChoiceVote {
            voter: "blue".to_string(),
            position: MultipleChoiceVote { option_id: 2 },
            weight: Uint128::new(10),
            should_execute: ShouldExecute::Yes,
        }],
        voting_strategy,
        Status::Rejected,
        None,
        Some(UncheckedDepositInfo {
            denom: DepositToken::VotingModuleToken {},
            amount: Uint128::new(1),
            refund_policy: DepositRefundPolicy::Always,
        }),
        false,
    );
    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let governance_modules = gov_state.proposal_modules;

    assert_eq!(governance_modules.len(), 1);
    let govmod = governance_modules.into_iter().next().unwrap().address;

    let (deposit_config, _) = query_deposit_config_and_pre_propose_module(&app, &govmod);
    if let CheckedDepositInfo {
        denom: CheckedDenom::Cw20(ref token),
        ..
    } = deposit_config.deposit_info.unwrap()
    {
        // Proposal has been executed so deposit has been refunded.
        let balance = query_balance_cw20(&app, token, "blue".to_string());
        assert_eq!(balance, Uint128::new(9));

        // Close the proposal, this should cause the deposit to be
        // refunded.
        app.execute_contract(
            Addr::unchecked("blue"),
            govmod,
            &ExecuteMsg::Close { proposal_id: 1 },
            &[],
        )
        .unwrap();

        // Proposal has been executed so deposit has been refunded.
        let balance = query_balance_cw20(&app, token, "blue".to_string());
        assert_eq!(balance, Uint128::new(10));
    } else {
        panic!()
    };
}

#[test]
fn test_execute_expired_proposal() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let quorum = PercentageThreshold::Percent(Decimal::percent(10));
    let voting_strategy = VotingStrategy::SingleChoice { quorum };
    let max_voting_period = cw_utils::Duration::Height(6);
    let instantiate = InstantiateMsg {
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        voting_strategy,
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };

    let core_addr = instantiate_with_staked_balances_governance(
        &mut app,
        instantiate,
        Some(vec![
            Cw20Coin {
                address: "blue".to_string(),
                amount: Uint128::new(10),
            },
            Cw20Coin {
                address: "inactive".to_string(),
                amount: Uint128::new(90),
            },
        ]),
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let proposal_modules = gov_state.proposal_modules;

    assert_eq!(proposal_modules.len(), 1);
    let govmod = proposal_modules.into_iter().next().unwrap().address;

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];

    let mc_options = MultipleChoiceOptions { options };

    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Propose {
            title: "A simple text proposal".to_string(),
            description: "A simple text proposal".to_string(),
            choices: mc_options,
            proposer: None,
        },
        &[],
    )
    .unwrap();

    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap();

    // Proposal has now reached quorum but should not be passed.
    let proposal: ProposalResponse = query_proposal(&app, &govmod, 1);
    assert_eq!(proposal.proposal.status, Status::Open);

    // Expire the proposal. It should now be passed as quorum was reached.
    app.update_block(|b| b.height += 10);

    let proposal: ProposalResponse = query_proposal(&app, &govmod, 1);
    assert_eq!(proposal.proposal.status, Status::Passed);

    // Try to close the proposal. This should fail as the proposal is
    // passed.
    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Close { proposal_id: 1 },
        &[],
    )
    .unwrap_err();

    // Check that we can execute the proposal despite the fact that it
    // is technically expired.
    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Execute { proposal_id: 1 },
        &[],
    )
    .unwrap();

    // Can't execute more than once.
    app.execute_contract(
        Addr::unchecked("blue"),
        govmod.clone(),
        &ExecuteMsg::Execute { proposal_id: 1 },
        &[],
    )
    .unwrap_err();

    let proposal: ProposalResponse = query_proposal(&app, &govmod, 1);
    assert_eq!(proposal.proposal.status, Status::Executed);
}

#[test]
fn test_update_config() {
    let (mut app, core_addr) = do_test_votes_cw20_balances(
        vec![TestMultipleChoiceVote {
            voter: "blue".to_string(),
            position: MultipleChoiceVote { option_id: 0 },
            weight: Uint128::new(10),
            should_execute: ShouldExecute::Yes,
        }],
        VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {},
        },
        Status::Passed,
        None,
        None,
        false,
    );

    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let governance_modules = gov_state.proposal_modules;

    assert_eq!(governance_modules.len(), 1);
    let govmod = governance_modules.into_iter().next().unwrap().address;

    let govmod_config: Config = query_proposal_config(&app, &govmod);

    assert_eq!(
        govmod_config.voting_strategy,
        VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {}
        }
    );

    let dao = govmod_config.dao;

    // Attempt to update the config from a non-dao address. This
    // should fail as it is unauthorized.
    app.execute_contract(
        Addr::unchecked("wrong"),
        govmod.clone(),
        &ExecuteMsg::UpdateConfig {
            voting_strategy: VotingStrategy::SingleChoice {
                quorum: PercentageThreshold::Majority {},
            },
            min_voting_period: None,
            close_proposal_on_execution_failure: true,
            max_voting_period: cw_utils::Duration::Height(10),
            only_members_execute: false,
            allow_revoting: false,
            dao: dao.to_string(),
        },
        &[],
    )
    .unwrap_err();

    // Update the config from the DAO address. This should succeed.
    app.execute_contract(
        dao.clone(),
        govmod.clone(),
        &ExecuteMsg::UpdateConfig {
            voting_strategy: VotingStrategy::SingleChoice {
                quorum: PercentageThreshold::Majority {},
            },
            min_voting_period: None,
            close_proposal_on_execution_failure: true,
            max_voting_period: cw_utils::Duration::Height(10),
            only_members_execute: false,
            allow_revoting: false,
            dao: Addr::unchecked(CREATOR_ADDR).to_string(),
        },
        &[],
    )
    .unwrap();

    let govmod_config: Config = query_proposal_config(&app, &govmod);

    let expected = Config {
        voting_strategy: VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {},
        },
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period: cw_utils::Duration::Height(10),
        only_members_execute: false,
        allow_revoting: false,
        dao: Addr::unchecked(CREATOR_ADDR),
    };
    assert_eq!(govmod_config, expected);

    // As we have changed the DAO address updating the config using
    // the original one should now fail.
    app.execute_contract(
        dao,
        govmod,
        &ExecuteMsg::UpdateConfig {
            voting_strategy: VotingStrategy::SingleChoice {
                quorum: PercentageThreshold::Majority {},
            },
            min_voting_period: None,
            close_proposal_on_execution_failure: true,
            max_voting_period: cw_utils::Duration::Height(10),
            only_members_execute: false,
            allow_revoting: false,
            dao: Addr::unchecked(CREATOR_ADDR).to_string(),
        },
        &[],
    )
    .unwrap_err();
}

#[test]
fn test_no_return_if_no_refunds() {
    let (mut app, core_addr) = do_test_votes_cw20_balances(
        vec![TestMultipleChoiceVote {
            voter: "blue".to_string(),
            position: MultipleChoiceVote { option_id: 2 },
            weight: Uint128::new(10),
            should_execute: ShouldExecute::Yes,
        }],
        VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {},
        },
        Status::Rejected,
        None,
        Some(UncheckedDepositInfo {
            denom: DepositToken::VotingModuleToken {},
            amount: Uint128::new(1),
            refund_policy: DepositRefundPolicy::OnlyPassed,
        }),
        true,
    );
    let gov_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr, &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let governance_modules = gov_state.proposal_modules;

    assert_eq!(governance_modules.len(), 1);
    let govmod = governance_modules.into_iter().next().unwrap().address;

    let (deposit_config, _) = query_deposit_config_and_pre_propose_module(&app, &govmod);
    if let CheckedDepositInfo {
        denom: CheckedDenom::Cw20(ref token),
        ..
    } = deposit_config.deposit_info.unwrap()
    {
        // Close the proposal, this should cause the deposit to be
        // refunded.
        app.execute_contract(
            Addr::unchecked("blue"),
            govmod,
            &ExecuteMsg::Close { proposal_id: 1 },
            &[],
        )
        .unwrap();

        // Proposal has been executed so deposit has been refunded.
        let balance = query_balance_cw20(&app, token, "blue".to_string());
        assert_eq!(balance, Uint128::new(9));
    } else {
        panic!()
    };
}

#[test]
fn test_query_list_proposals() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let quorum = PercentageThreshold::Majority {};
    let voting_strategy = VotingStrategy::SingleChoice { quorum };
    let max_voting_period = cw_utils::Duration::Height(6);
    let instantiate = InstantiateMsg {
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        voting_strategy: voting_strategy.clone(),
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };
    let gov_addr = instantiate_with_staked_balances_governance(
        &mut app,
        instantiate,
        Some(vec![Cw20Coin {
            address: CREATOR_ADDR.to_string(),
            amount: Uint128::new(100),
        }]),
    );

    let gov_modules: Vec<ProposalModule> = app
        .wrap()
        .query_wasm_smart(
            gov_addr,
            &cwd_core::msg::QueryMsg::ProposalModules {
                start_after: None,
                limit: None,
            },
        )
        .unwrap();
    assert_eq!(gov_modules.len(), 1);

    let govmod = gov_modules.into_iter().next().unwrap().address;

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];

    let mc_options = MultipleChoiceOptions { options };

    for _i in 1..10 {
        app.execute_contract(
            Addr::unchecked(CREATOR_ADDR),
            govmod.clone(),
            &ExecuteMsg::Propose {
                title: "A simple text proposal".to_string(),
                description: "A simple text proposal".to_string(),
                choices: mc_options.clone(),
                proposer: None,
            },
            &[],
        )
        .unwrap();
    }

    let proposals_forward: ProposalListResponse = query_list_proposals(&app, &govmod, None, None);
    let mut proposals_backward: ProposalListResponse =
        query_list_proposals_reverse(&app, &govmod, None, None);

    proposals_backward.proposals.reverse();

    assert_eq!(proposals_forward.proposals, proposals_backward.proposals);
    let checked_options = mc_options.into_checked().unwrap();
    let current_block = app.block_info();
    let expected = ProposalResponse {
        id: 1,
        proposal: MultipleChoiceProposal {
            title: "A simple text proposal".to_string(),
            description: "A simple text proposal".to_string(),
            proposer: Addr::unchecked(CREATOR_ADDR),
            start_height: current_block.height,
            expiration: max_voting_period.after(&current_block),
            choices: checked_options.options.clone(),
            status: Status::Open,
            voting_strategy: voting_strategy.clone(),
            total_power: Uint128::new(100),
            votes: MultipleChoiceVotes {
                vote_weights: vec![Uint128::zero(); 3],
            },
            allow_revoting: false,
            min_voting_period: None,
        },
    };
    assert_eq!(proposals_forward.proposals[0], expected);

    // Get proposals (3, 5]
    let proposals_forward: ProposalListResponse =
        query_list_proposals(&app, &govmod, Some(3), Some(2));

    let mut proposals_backward: ProposalListResponse =
        query_list_proposals_reverse(&app, &govmod, Some(6), Some(2));

    let expected = ProposalResponse {
        id: 4,
        proposal: MultipleChoiceProposal {
            title: "A simple text proposal".to_string(),
            description: "A simple text proposal".to_string(),
            proposer: Addr::unchecked(CREATOR_ADDR),
            start_height: current_block.height,
            expiration: max_voting_period.after(&current_block),
            choices: checked_options.options,
            status: Status::Open,
            voting_strategy,
            total_power: Uint128::new(100),
            votes: MultipleChoiceVotes {
                vote_weights: vec![Uint128::zero(); 3],
            },
            allow_revoting: false,
            min_voting_period: None,
        },
    };
    assert_eq!(proposals_forward.proposals[0], expected);
    assert_eq!(proposals_backward.proposals[1], expected);

    proposals_backward.proposals.reverse();
    assert_eq!(proposals_forward.proposals, proposals_backward.proposals);
}

#[test]
fn test_hooks() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());

    let quorum = PercentageThreshold::Majority {};
    let voting_strategy = VotingStrategy::SingleChoice { quorum };
    let max_voting_period = cw_utils::Duration::Height(6);
    let instantiate = InstantiateMsg {
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        voting_strategy,
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };

    let core_addr = instantiate_with_staked_balances_governance(&mut app, instantiate, None);
    let govmod = query_multiple_proposal_module(&app, &core_addr);

    let govmod_config: Config = query_proposal_config(&app, &govmod);
    let dao = govmod_config.dao;

    // Expect no hooks
    let hooks: HooksResponse = query_proposal_hooks(&app, &govmod);
    assert_eq!(hooks.hooks.len(), 0);

    let hooks: HooksResponse = query_vote_hooks(&app, &govmod);
    assert_eq!(hooks.hooks.len(), 0);

    let msg = ExecuteMsg::AddProposalHook {
        address: "some_addr".to_string(),
    };

    // Expect error as sender is not DAO
    let _err = app
        .execute_contract(Addr::unchecked(CREATOR_ADDR), govmod.clone(), &msg, &[])
        .unwrap_err();

    // Expect success as sender is now DAO
    let _res = app
        .execute_contract(dao.clone(), govmod.clone(), &msg, &[])
        .unwrap();

    let hooks: HooksResponse = query_proposal_hooks(&app, &govmod);
    assert_eq!(hooks.hooks.len(), 1);

    // Expect error as hook is already set
    let _err = app
        .execute_contract(dao.clone(), govmod.clone(), &msg, &[])
        .unwrap_err();

    // Expect error as hook does not exist
    let _err = app
        .execute_contract(
            dao.clone(),
            govmod.clone(),
            &ExecuteMsg::RemoveProposalHook {
                address: "not_exist".to_string(),
            },
            &[],
        )
        .unwrap_err();

    let msg = ExecuteMsg::RemoveProposalHook {
        address: "some_addr".to_string(),
    };

    // Expect error as sender is not DAO
    let _err = app
        .execute_contract(Addr::unchecked(CREATOR_ADDR), govmod.clone(), &msg, &[])
        .unwrap_err();

    // Expect success
    let _res = app
        .execute_contract(dao.clone(), govmod.clone(), &msg, &[])
        .unwrap();

    let msg = ExecuteMsg::AddVoteHook {
        address: "some_addr".to_string(),
    };

    // Expect error as sender is not DAO
    let _err = app
        .execute_contract(Addr::unchecked(CREATOR_ADDR), govmod.clone(), &msg, &[])
        .unwrap_err();

    // Expect success as sender is now DAO
    let _res = app
        .execute_contract(dao.clone(), govmod.clone(), &msg, &[])
        .unwrap();

    let hooks: HooksResponse = query_vote_hooks(&app, &govmod);
    assert_eq!(hooks.hooks.len(), 1);

    // Expect error as hook is already set
    let _err = app
        .execute_contract(dao.clone(), govmod.clone(), &msg, &[])
        .unwrap_err();

    // Expect error as hook does not exist
    let _err = app
        .execute_contract(
            dao.clone(),
            govmod.clone(),
            &ExecuteMsg::RemoveVoteHook {
                address: "not_exist".to_string(),
            },
            &[],
        )
        .unwrap_err();

    let msg = ExecuteMsg::RemoveVoteHook {
        address: "some_addr".to_string(),
    };

    // Expect error as sender is not DAO
    let _err = app
        .execute_contract(Addr::unchecked(CREATOR_ADDR), govmod.clone(), &msg, &[])
        .unwrap_err();

    // Expect success
    let _res = app.execute_contract(dao, govmod, &msg, &[]).unwrap();
}

#[test]
fn test_active_threshold_absolute() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());

    let quorum = PercentageThreshold::Majority {};
    let voting_strategy = VotingStrategy::SingleChoice { quorum };
    let max_voting_period = cw_utils::Duration::Height(6);
    let instantiate = InstantiateMsg {
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        voting_strategy,
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };

    let core_addr = instantiate_with_staking_active_threshold(
        &mut app,
        instantiate,
        None,
        Some(ActiveThreshold::AbsoluteCount {
            count: Uint128::new(100),
        }),
    );
    let govmod = query_multiple_proposal_module(&app, &core_addr);

    let govmod_config: Config = query_proposal_config(&app, &govmod);
    let dao = govmod_config.dao;
    let voting_module: Addr = app
        .wrap()
        .query_wasm_smart(dao, &cwd_core::msg::QueryMsg::VotingModule {})
        .unwrap();
    let staking_contract: Addr = app
        .wrap()
        .query_wasm_smart(
            voting_module.clone(),
            &cwd_voting_cw20_staked::msg::QueryMsg::StakingContract {},
        )
        .unwrap();
    let token_contract: Addr = app
        .wrap()
        .query_wasm_smart(
            voting_module,
            &cwd_interface::voting::Query::TokenContract {},
        )
        .unwrap();

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];

    let mc_options = MultipleChoiceOptions { options };

    // Try and create a proposal, will fail as inactive
    let _err = app
        .execute_contract(
            Addr::unchecked(CREATOR_ADDR),
            govmod.clone(),
            &crate::msg::ExecuteMsg::Propose {
                title: "A simple text proposal".to_string(),
                description: "This is a simple text proposal".to_string(),
                choices: mc_options.clone(),
                proposer: None,
            },
            &[],
        )
        .unwrap_err();

    // Stake enough tokens
    let msg = cw20::Cw20ExecuteMsg::Send {
        contract: staking_contract.to_string(),
        amount: Uint128::new(100),
        msg: to_binary(&cw20_stake::msg::ReceiveMsg::Stake {}).unwrap(),
    };
    app.execute_contract(Addr::unchecked(CREATOR_ADDR), token_contract, &msg, &[])
        .unwrap();
    app.update_block(next_block);

    // Try and create a proposal, will now succeed as enough tokens are staked
    let _res = app
        .execute_contract(
            Addr::unchecked(CREATOR_ADDR),
            govmod.clone(),
            &crate::msg::ExecuteMsg::Propose {
                title: "A simple text proposal".to_string(),
                description: "This is a simple text proposal".to_string(),
                choices: mc_options.clone(),
                proposer: None,
            },
            &[],
        )
        .unwrap();

    // Unstake some tokens to make it inactive again
    let msg = cw20_stake::msg::ExecuteMsg::Unstake {
        amount: Uint128::new(50),
    };
    app.execute_contract(Addr::unchecked(CREATOR_ADDR), staking_contract, &msg, &[])
        .unwrap();
    app.update_block(next_block);

    // Try and create a proposal, will fail as no longer active
    let _err = app
        .execute_contract(
            Addr::unchecked(CREATOR_ADDR),
            govmod,
            &crate::msg::ExecuteMsg::Propose {
                title: "A simple text proposal".to_string(),
                description: "This is a simple text proposal".to_string(),
                choices: mc_options,
                proposer: None,
            },
            &[],
        )
        .unwrap_err();
}

#[test]
fn test_active_threshold_percent() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let quorum = PercentageThreshold::Majority {};
    let voting_strategy = VotingStrategy::SingleChoice { quorum };
    let max_voting_period = cw_utils::Duration::Height(6);
    let instantiate = InstantiateMsg {
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        voting_strategy,
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };

    // 20% needed to be active, 20% of 100000000 is 20000000
    let core_addr = instantiate_with_staking_active_threshold(
        &mut app,
        instantiate,
        None,
        Some(ActiveThreshold::Percentage {
            percent: Decimal::percent(20),
        }),
    );
    let govmod = query_multiple_proposal_module(&app, &core_addr);

    let govmod_config: Config = query_proposal_config(&app, &govmod);
    let dao = govmod_config.dao;
    let voting_module: Addr = app
        .wrap()
        .query_wasm_smart(dao, &cwd_core::msg::QueryMsg::VotingModule {})
        .unwrap();
    let staking_contract: Addr = app
        .wrap()
        .query_wasm_smart(
            voting_module.clone(),
            &cwd_voting_cw20_staked::msg::QueryMsg::StakingContract {},
        )
        .unwrap();
    let token_contract: Addr = app
        .wrap()
        .query_wasm_smart(
            voting_module,
            &cwd_interface::voting::Query::TokenContract {},
        )
        .unwrap();

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];

    let mc_options = MultipleChoiceOptions { options };

    // Try and create a proposal, will fail as inactive
    let _res = app
        .execute_contract(
            Addr::unchecked(CREATOR_ADDR),
            govmod.clone(),
            &ExecuteMsg::Propose {
                title: "A simple text proposal".to_string(),
                description: "A simple text proposal".to_string(),
                choices: mc_options.clone(),
                proposer: None,
            },
            &[],
        )
        .unwrap_err();

    // Stake enough tokens
    let msg = cw20::Cw20ExecuteMsg::Send {
        contract: staking_contract.to_string(),
        amount: Uint128::new(20000000),
        msg: to_binary(&cw20_stake::msg::ReceiveMsg::Stake {}).unwrap(),
    };
    app.execute_contract(Addr::unchecked(CREATOR_ADDR), token_contract, &msg, &[])
        .unwrap();
    app.update_block(next_block);

    // Try and create a proposal, will now succeed as enough tokens are staked
    let _res = app
        .execute_contract(
            Addr::unchecked(CREATOR_ADDR),
            govmod.clone(),
            &ExecuteMsg::Propose {
                title: "A simple text proposal".to_string(),
                description: "A simple text proposal".to_string(),
                choices: mc_options.clone(),
                proposer: None,
            },
            &[],
        )
        .unwrap();

    // Unstake some tokens to make it inactive again
    let msg = cw20_stake::msg::ExecuteMsg::Unstake {
        amount: Uint128::new(1000),
    };
    app.execute_contract(Addr::unchecked(CREATOR_ADDR), staking_contract, &msg, &[])
        .unwrap();
    app.update_block(next_block);

    // Try and create a proposal, will fail as no longer active
    let _res = app
        .execute_contract(
            Addr::unchecked(CREATOR_ADDR),
            govmod,
            &ExecuteMsg::Propose {
                title: "A simple text proposal".to_string(),
                description: "A simple text proposal".to_string(),
                choices: mc_options,
                proposer: None,
            },
            &[],
        )
        .unwrap_err();
}

#[test]
fn test_active_threshold_none() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let quorum = PercentageThreshold::Majority {};
    let voting_strategy = VotingStrategy::SingleChoice { quorum };
    let max_voting_period = cw_utils::Duration::Height(6);
    let instantiate = InstantiateMsg {
        min_voting_period: None,
        close_proposal_on_execution_failure: true,
        max_voting_period,
        only_members_execute: false,
        allow_revoting: false,
        voting_strategy,
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };

    let core_addr =
        instantiate_with_staking_active_threshold(&mut app, instantiate.clone(), None, None);
    let govmod = query_multiple_proposal_module(&app, &core_addr);

    let govmod_config: Config = query_proposal_config(&app, &govmod);
    let dao = govmod_config.dao;
    let voting_module: Addr = app
        .wrap()
        .query_wasm_smart(dao, &cwd_core::msg::QueryMsg::VotingModule {})
        .unwrap();
    let staking_contract: Addr = app
        .wrap()
        .query_wasm_smart(
            voting_module.clone(),
            &cwd_voting_cw20_staked::msg::QueryMsg::StakingContract {},
        )
        .unwrap();
    let token_contract: Addr = app
        .wrap()
        .query_wasm_smart(
            voting_module,
            &cwd_interface::voting::Query::TokenContract {},
        )
        .unwrap();

    // Stake some tokens so we can propose
    let msg = cw20::Cw20ExecuteMsg::Send {
        contract: staking_contract.to_string(),
        amount: Uint128::new(2000),
        msg: to_binary(&cw20_stake::msg::ReceiveMsg::Stake {}).unwrap(),
    };
    app.execute_contract(Addr::unchecked(CREATOR_ADDR), token_contract, &msg, &[])
        .unwrap();
    app.update_block(next_block);

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];

    let mc_options = MultipleChoiceOptions { options };

    // Try and create a proposal, will succeed as no threshold
    let _res = app
        .execute_contract(
            Addr::unchecked(CREATOR_ADDR),
            govmod,
            &ExecuteMsg::Propose {
                title: "A simple text proposal".to_string(),
                description: "A simple text proposal".to_string(),
                choices: mc_options.clone(),
                proposer: None,
            },
            &[],
        )
        .unwrap();

    // Now try with balance voting to test when IsActive is not implemented
    // on the contract
    let _threshold = Threshold::AbsolutePercentage {
        percentage: PercentageThreshold::Majority {},
    };
    let _max_voting_period = cw_utils::Duration::Height(6);

    let core_addr = instantiate_with_staked_balances_governance(&mut app, instantiate, None);
    let govmod = query_multiple_proposal_module(&app, &core_addr);

    // Try and create a proposal, will succeed as IsActive is not implemented
    let _res = app
        .execute_contract(
            Addr::unchecked(CREATOR_ADDR),
            govmod,
            &ExecuteMsg::Propose {
                title: "A simple text proposal".to_string(),
                description: "A simple text proposal".to_string(),
                choices: mc_options,
                proposer: None,
            },
            &[],
        )
        .unwrap();
}

/// Basic test for revoting on prop-multiple
#[test]
fn test_revoting() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let core_addr = instantiate_with_staked_balances_governance(
        &mut app,
        InstantiateMsg {
            min_voting_period: None,
            max_voting_period: Duration::Height(6),
            only_members_execute: false,
            allow_revoting: true,
            voting_strategy: VotingStrategy::SingleChoice {
                quorum: PercentageThreshold::Majority {},
            },
            close_proposal_on_execution_failure: false,
            pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
        },
        Some(vec![
            Cw20Coin {
                address: "a-1".to_string(),
                amount: Uint128::new(100_000_000),
            },
            Cw20Coin {
                address: "a-2".to_string(),
                amount: Uint128::new(100_000_000),
            },
        ]),
    );

    let govmod = query_multiple_proposal_module(&app, &core_addr);

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];
    let mc_options = MultipleChoiceOptions { options };

    // Create a basic proposal with 2 options
    app.execute_contract(
        Addr::unchecked("a-1"),
        govmod.clone(),
        &ExecuteMsg::Propose {
            title: "A simple text proposal".to_string(),
            description: "A simple text proposal".to_string(),
            choices: mc_options,
            proposer: None,
        },
        &[],
    )
    .unwrap();

    // a-1 votes, vote_weights: [100_000_000, 0]
    app.execute_contract(
        Addr::unchecked("a-1"),
        govmod.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap();

    // a-2 votes, vote_weights: [100_000_000, 100_000_000]
    app.execute_contract(
        Addr::unchecked("a-2"),
        govmod.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 1 },
        },
        &[],
    )
    .unwrap();

    // Time passes..
    app.update_block(|b| b.height += 2);

    // Assert that both vote options have equal vote weights at some block
    let proposal: ProposalResponse = query_proposal(&app, &govmod, 1);
    assert_eq!(proposal.proposal.status, Status::Open);
    assert_eq!(
        proposal.proposal.votes.vote_weights[0],
        Uint128::new(100_000_000),
    );
    assert_eq!(
        proposal.proposal.votes.vote_weights[1],
        Uint128::new(100_000_000),
    );

    // More time passes..
    app.update_block(|b| b.height += 3);

    // Last moment a-2 has a change of mind,
    // votes shift to [200_000_000, 0]
    app.execute_contract(
        Addr::unchecked("a-2"),
        govmod.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap();

    app.update_block(next_block);

    // Assert that revote succeeded
    let proposal: ProposalResponse = query_proposal(&app, &govmod, 1);
    assert_eq!(proposal.proposal.status, Status::Passed);
    assert_eq!(
        proposal.proposal.votes.vote_weights[0],
        Uint128::new(200_000_000),
    );
    assert_eq!(proposal.proposal.votes.vote_weights[1], Uint128::new(0),);
}

/// Tests that revoting is stored at a per-proposal level.
/// Proposals created while revoting is enabled should not
/// have it disabled if a config change turns if off.
#[test]
fn test_allow_revoting_config_changes() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let core_addr = instantiate_with_staked_balances_governance(
        &mut app,
        InstantiateMsg {
            min_voting_period: None,
            max_voting_period: Duration::Height(6),
            only_members_execute: false,
            allow_revoting: true,
            voting_strategy: VotingStrategy::SingleChoice {
                quorum: PercentageThreshold::Majority {},
            },
            close_proposal_on_execution_failure: false,
            pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
        },
        Some(vec![
            Cw20Coin {
                address: "a-1".to_string(),
                amount: Uint128::new(100_000_000),
            },
            Cw20Coin {
                address: "a-2".to_string(),
                amount: Uint128::new(100_000_000),
            },
        ]),
    );

    let proposal_module = query_multiple_proposal_module(&app, &core_addr);

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];
    let mc_options = MultipleChoiceOptions { options };

    // Create a basic proposal with 2 options that allows revoting
    app.execute_contract(
        Addr::unchecked("a-1"),
        proposal_module.clone(),
        &ExecuteMsg::Propose {
            title: "A simple text proposal".to_string(),
            description: "A simple text proposal".to_string(),
            choices: mc_options.clone(),
            proposer: None,
        },
        &[],
    )
    .unwrap();

    // Disable revoting
    app.execute_contract(
        core_addr.clone(),
        proposal_module.clone(),
        &ExecuteMsg::UpdateConfig {
            min_voting_period: None,
            max_voting_period: Duration::Height(6),
            only_members_execute: false,
            allow_revoting: false,
            dao: core_addr.to_string(),
            voting_strategy: VotingStrategy::SingleChoice {
                quorum: PercentageThreshold::Majority {},
            },
            close_proposal_on_execution_failure: false,
        },
        &[],
    )
    .unwrap();

    // Assert that proposal_id: 1 still allows revoting
    let proposal: ProposalResponse = query_proposal(&app, &proposal_module, 1);
    assert!(proposal.proposal.allow_revoting);

    app.execute_contract(
        Addr::unchecked("a-1"),
        proposal_module.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap();
    app.execute_contract(
        Addr::unchecked("a-1"),
        proposal_module.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 1 },
        },
        &[],
    )
    .unwrap();

    // New proposals should not allow revoting
    app.execute_contract(
        Addr::unchecked("a-2"),
        proposal_module.clone(),
        &ExecuteMsg::Propose {
            title: "A very complex text proposal".to_string(),
            description: "A very complex text proposal".to_string(),
            choices: mc_options,
            proposer: None,
        },
        &[],
    )
    .unwrap();

    app.execute_contract(
        Addr::unchecked("a-2"),
        proposal_module.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 2,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap();

    let err: ContractError = app
        .execute_contract(
            Addr::unchecked("a-2"),
            proposal_module,
            &ExecuteMsg::Vote {
                proposal_id: 2,
                vote: MultipleChoiceVote { option_id: 1 },
            },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();

    assert!(matches!(err, ContractError::AlreadyVoted {}));
}

/// Tests that we error if a revote casts the same vote as the
/// previous vote.
#[test]
fn test_revoting_same_vote_twice() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let core_addr = instantiate_with_staked_balances_governance(
        &mut app,
        InstantiateMsg {
            min_voting_period: None,
            max_voting_period: Duration::Height(6),
            only_members_execute: false,
            allow_revoting: true,
            voting_strategy: VotingStrategy::SingleChoice {
                quorum: PercentageThreshold::Majority {},
            },
            close_proposal_on_execution_failure: false,
            pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
        },
        Some(vec![
            Cw20Coin {
                address: "a-1".to_string(),
                amount: Uint128::new(100_000_000),
            },
            Cw20Coin {
                address: "a-2".to_string(),
                amount: Uint128::new(100_000_000),
            },
        ]),
    );

    let proprosal_module = query_multiple_proposal_module(&app, &core_addr);

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];
    let mc_options = MultipleChoiceOptions { options };

    // Create a basic proposal with 2 options that allows revoting
    app.execute_contract(
        Addr::unchecked("a-1"),
        proprosal_module.clone(),
        &ExecuteMsg::Propose {
            title: "A simple text proposal".to_string(),
            description: "A simple text proposal".to_string(),
            choices: mc_options,
            proposer: None,
        },
        &[],
    )
    .unwrap();

    // Cast a vote
    app.execute_contract(
        Addr::unchecked("a-1"),
        proprosal_module.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap();

    // Revote for the same option as currently voted
    let err: ContractError = app
        .execute_contract(
            Addr::unchecked("a-1"),
            proprosal_module,
            &ExecuteMsg::Vote {
                proposal_id: 1,
                vote: MultipleChoiceVote { option_id: 0 },
            },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();

    // Can't cast the same vote twice.
    assert!(matches!(err, ContractError::AlreadyCast {}));
}

/// Tests that revoting into a non-existing vote option
/// does not invalidate the initial vote
#[test]
fn test_invalid_revote_does_not_invalidate_initial_vote() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());
    let core_addr = instantiate_with_staked_balances_governance(
        &mut app,
        InstantiateMsg {
            min_voting_period: None,
            max_voting_period: Duration::Height(6),
            only_members_execute: false,
            allow_revoting: true,
            voting_strategy: VotingStrategy::SingleChoice {
                quorum: PercentageThreshold::Majority {},
            },
            close_proposal_on_execution_failure: false,
            pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
        },
        Some(vec![
            Cw20Coin {
                address: "a-1".to_string(),
                amount: Uint128::new(100_000_000),
            },
            Cw20Coin {
                address: "a-2".to_string(),
                amount: Uint128::new(100_000_000),
            },
        ]),
    );

    let proposal_module = query_multiple_proposal_module(&app, &core_addr);

    let options = vec![
        MultipleChoiceOption {
            description: "multiple choice option 1".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "multiple choice option 2".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];
    let mc_options = MultipleChoiceOptions { options };

    // Create a basic proposal with 2 options
    app.execute_contract(
        Addr::unchecked("a-1"),
        proposal_module.clone(),
        &ExecuteMsg::Propose {
            title: "A simple text proposal".to_string(),
            description: "A simple text proposal".to_string(),
            choices: mc_options,
            proposer: None,
        },
        &[],
    )
    .unwrap();

    // a-1 votes, vote_weights: [100_000_000, 0]
    app.execute_contract(
        Addr::unchecked("a-1"),
        proposal_module.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap();

    // a-2 votes, vote_weights: [100_000_000, 100_000_000]
    app.execute_contract(
        Addr::unchecked("a-2"),
        proposal_module.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 1 },
        },
        &[],
    )
    .unwrap();

    app.update_block(next_block);

    // Assert that both vote options have equal vote weights at some block
    let proposal: ProposalResponse = query_proposal(&app, &proposal_module, 1);
    assert_eq!(proposal.proposal.status, Status::Open);
    assert_eq!(
        proposal.proposal.votes.vote_weights[0],
        Uint128::new(100_000_000),
    );
    assert_eq!(
        proposal.proposal.votes.vote_weights[1],
        Uint128::new(100_000_000),
    );

    // Time passes..
    app.update_block(|b| b.height += 3);

    // Last moment a-2 has a change of mind and attempts
    // to vote for a non-existing option
    let err: ContractError = app
        .execute_contract(
            Addr::unchecked("a-2"),
            proposal_module,
            &ExecuteMsg::Vote {
                proposal_id: 1,
                vote: MultipleChoiceVote { option_id: 99 },
            },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    // Assert that prior votes remained the same
    assert_eq!(
        proposal.proposal.votes.vote_weights[0],
        Uint128::new(100_000_000),
    );
    assert_eq!(
        proposal.proposal.votes.vote_weights[1],
        Uint128::new(100_000_000),
    );
    assert!(matches!(err, ContractError::InvalidVote {}));
}

#[test]
fn test_return_deposit_to_dao_on_proposal_failure() {
    let (mut app, core_addr) = do_test_votes_cw20_balances(
        vec![TestMultipleChoiceVote {
            voter: "blue".to_string(),
            position: MultipleChoiceVote { option_id: 2 },
            weight: Uint128::new(10),
            should_execute: ShouldExecute::Yes,
        }],
        VotingStrategy::SingleChoice {
            quorum: PercentageThreshold::Majority {},
        },
        Status::Open,
        Some(Uint128::new(100)),
        Some(UncheckedDepositInfo {
            denom: DepositToken::VotingModuleToken {},
            amount: Uint128::new(1),
            refund_policy: DepositRefundPolicy::OnlyPassed,
        }),
        false,
    );

    let core_state: cwd_core::query::DumpStateResponse = app
        .wrap()
        .query_wasm_smart(core_addr.clone(), &cwd_core::msg::QueryMsg::DumpState {})
        .unwrap();
    let proposal_modules = core_state.proposal_modules;

    assert_eq!(proposal_modules.len(), 1);
    let proposal_multiple = proposal_modules.into_iter().next().unwrap().address;

    // Make the proposal expire. It has now failed.
    app.update_block(|block| block.height += 10);

    // Close the proposal, this should work as the proposal is now
    // open and expired.
    app.execute_contract(
        Addr::unchecked("keze"),
        proposal_multiple.clone(),
        &ExecuteMsg::Close { proposal_id: 1 },
        &[],
    )
    .unwrap();

    let (deposit_config, _) = query_deposit_config_and_pre_propose_module(&app, &proposal_multiple);
    if let CheckedDepositInfo {
        denom: CheckedDenom::Cw20(ref token),
        ..
    } = deposit_config.deposit_info.unwrap()
    {
        // // Deposit should now belong to the DAO.
        let balance = query_balance_cw20(&app, token, core_addr.to_string());
        assert_eq!(balance, Uint128::new(1));
    } else {
        panic!()
    };
}

#[test]
fn test_close_failed_proposal() {
    let mut app = App::default();
    let _govmod_id = app.store_code(proposal_multiple_contract());

    let quorum = PercentageThreshold::Majority {};
    let voting_strategy = VotingStrategy::SingleChoice { quorum };
    let max_voting_period = cw_utils::Duration::Height(6);
    let instantiate = InstantiateMsg {
        max_voting_period,
        voting_strategy,
        min_voting_period: None,
        only_members_execute: false,
        allow_revoting: false,
        close_proposal_on_execution_failure: true,
        pre_propose_info: PreProposeInfo::AnyoneMayPropose {},
    };

    let core_addr = instantiate_with_staking_active_threshold(&mut app, instantiate, None, None);
    let govmod = query_multiple_proposal_module(&app, &core_addr);

    let govmod_config: Config = query_proposal_config(&app, &govmod);
    let dao = govmod_config.dao;
    let voting_module: Addr = app
        .wrap()
        .query_wasm_smart(dao, &cwd_core::msg::QueryMsg::VotingModule {})
        .unwrap();
    let staking_contract: Addr = app
        .wrap()
        .query_wasm_smart(
            voting_module.clone(),
            &cwd_voting_cw20_staked::msg::QueryMsg::StakingContract {},
        )
        .unwrap();
    let token_contract: Addr = app
        .wrap()
        .query_wasm_smart(
            voting_module,
            &cwd_interface::voting::Query::TokenContract {},
        )
        .unwrap();

    // Stake some tokens so we can propose
    let msg = cw20::Cw20ExecuteMsg::Send {
        contract: staking_contract.to_string(),
        amount: Uint128::new(2000),
        msg: to_binary(&cw20_stake::msg::ReceiveMsg::Stake {}).unwrap(),
    };
    app.execute_contract(
        Addr::unchecked(CREATOR_ADDR),
        token_contract.clone(),
        &msg,
        &[],
    )
    .unwrap();
    app.update_block(next_block);

    let msg = cw20::Cw20ExecuteMsg::Burn {
        amount: Uint128::new(2000),
    };
    let binary_msg = to_binary(&msg).unwrap();

    let options = vec![
        MultipleChoiceOption {
            description: "Burn or burn".to_string(),
            msgs: vec![WasmMsg::Execute {
                contract_addr: token_contract.to_string(),
                msg: binary_msg,
                funds: vec![],
            }
            .into()],
            title: "title".to_string(),
        },
        MultipleChoiceOption {
            description: "Don't burn".to_string(),
            msgs: vec![],
            title: "title".to_string(),
        },
    ];

    let mc_options = MultipleChoiceOptions { options };

    // Overburn tokens
    app.execute_contract(
        Addr::unchecked(CREATOR_ADDR),
        govmod.clone(),
        &ExecuteMsg::Propose {
            title: "A simple burn tokens proposal".to_string(),
            description: "Burning more tokens, than dao treasury have".to_string(),
            choices: mc_options.clone(),
            proposer: None,
        },
        &[],
    )
    .unwrap();

    // Vote on proposal
    app.execute_contract(
        Addr::unchecked(CREATOR_ADDR),
        govmod.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap();

    // Update block
    let timestamp = Timestamp::from_seconds(300_000_000);
    app.update_block(|block| block.time = timestamp);

    // Execute proposal
    app.execute_contract(
        Addr::unchecked(CREATOR_ADDR),
        govmod.clone(),
        &ExecuteMsg::Execute { proposal_id: 1 },
        &[],
    )
    .unwrap();

    let failed: ProposalResponse = query_proposal(&app, &govmod, 1);

    assert_eq!(failed.proposal.status, Status::ExecutionFailed);
    // With disabled feature
    // Disable feature first
    {
        let original: Config = query_proposal_config(&app, &govmod);

        app.execute_contract(
            Addr::unchecked(CREATOR_ADDR),
            govmod.clone(),
            &ExecuteMsg::Propose {
                title: "Disable closing failed proposals".to_string(),
                description: "We want to re-execute failed proposals".to_string(),
                choices: MultipleChoiceOptions {
                    options: vec![
                        MultipleChoiceOption {
                            description: "Disable closing failed proposals".to_string(),
                            msgs: vec![WasmMsg::Execute {
                                contract_addr: govmod.to_string(),
                                msg: to_binary(&ExecuteMsg::UpdateConfig {
                                    voting_strategy: VotingStrategy::SingleChoice { quorum },
                                    max_voting_period: original.max_voting_period,
                                    min_voting_period: original.min_voting_period,
                                    only_members_execute: original.only_members_execute,
                                    allow_revoting: false,
                                    dao: original.dao.to_string(),
                                    close_proposal_on_execution_failure: false,
                                })
                                .unwrap(),
                                funds: vec![],
                            }
                            .into()],
                            title: "title".to_string(),
                        },
                        MultipleChoiceOption {
                            description: "Don't disable".to_string(),
                            msgs: vec![],
                            title: "title".to_string(),
                        },
                    ],
                },
                proposer: None,
            },
            &[],
        )
        .unwrap();

        // Vote on proposal
        app.execute_contract(
            Addr::unchecked(CREATOR_ADDR),
            govmod.clone(),
            &ExecuteMsg::Vote {
                proposal_id: 2,
                vote: MultipleChoiceVote { option_id: 0 },
            },
            &[],
        )
        .unwrap();

        // Execute proposal
        app.execute_contract(
            Addr::unchecked(CREATOR_ADDR),
            govmod.clone(),
            &ExecuteMsg::Execute { proposal_id: 2 },
            &[],
        )
        .unwrap();
    }

    // Overburn tokens (again), this time without reverting
    app.execute_contract(
        Addr::unchecked(CREATOR_ADDR),
        govmod.clone(),
        &ExecuteMsg::Propose {
            title: "A simple burn tokens proposal".to_string(),
            description: "Burning more tokens, than dao treasury have".to_string(),
            choices: mc_options,
            proposer: None,
        },
        &[],
    )
    .unwrap();

    // Vote on proposal
    app.execute_contract(
        Addr::unchecked(CREATOR_ADDR),
        govmod.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 3,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap();

    // Execute proposal
    app.execute_contract(
        Addr::unchecked(CREATOR_ADDR),
        govmod.clone(),
        &ExecuteMsg::Execute { proposal_id: 3 },
        &[],
    )
    .expect_err("Should be sub overflow");

    // Status should still be passed
    let updated: ProposalResponse = query_proposal(&app, &govmod, 3);

    // not reverted
    assert_eq!(updated.proposal.status, Status::Passed);
}

#[test]
fn test_no_double_refund_on_execute_fail_and_close() {
    let mut app = App::default();
    let _proposal_module_id = app.store_code(proposal_multiple_contract());

    let voting_strategy = VotingStrategy::SingleChoice {
        quorum: PercentageThreshold::Majority {},
    };
    let max_voting_period = cw_utils::Duration::Height(6);
    let instantiate = InstantiateMsg {
        voting_strategy,
        max_voting_period,
        min_voting_period: None,
        only_members_execute: false,
        allow_revoting: false,
        close_proposal_on_execution_failure: true,
        pre_propose_info: get_pre_propose_info(
            &mut app,
            Some(UncheckedDepositInfo {
                denom: DepositToken::VotingModuleToken {},
                amount: Uint128::new(1),
                // Important to set to true here as we want to be sure
                // that we don't get a second refund on close. Refunds on
                // close only happen if this is true.
                refund_policy: DepositRefundPolicy::Always,
            }),
            false,
        ),
    };

    let core_addr = instantiate_with_staking_active_threshold(
        &mut app,
        instantiate,
        Some(vec![Cw20Coin {
            address: CREATOR_ADDR.to_string(),
            // One token for sending to the DAO treasury, one token
            // for staking, one token for paying the proposal deposit.
            amount: Uint128::new(3),
        }]),
        None,
    );
    let govmod = query_multiple_proposal_module(&app, &core_addr);

    let proposal_config: Config = query_proposal_config(&app, &govmod);
    let dao = proposal_config.dao;
    let voting_module: Addr = app
        .wrap()
        .query_wasm_smart(dao, &cwd_core::msg::QueryMsg::VotingModule {})
        .unwrap();
    let staking_contract: Addr = app
        .wrap()
        .query_wasm_smart(
            voting_module.clone(),
            &cwd_voting_cw20_staked::msg::QueryMsg::StakingContract {},
        )
        .unwrap();
    let token_contract: Addr = app
        .wrap()
        .query_wasm_smart(
            voting_module,
            &cwd_interface::voting::Query::TokenContract {},
        )
        .unwrap();

    // Stake a token so we can propose.
    let msg = cw20::Cw20ExecuteMsg::Send {
        contract: staking_contract.to_string(),
        amount: Uint128::new(1),
        msg: to_binary(&cw20_stake::msg::ReceiveMsg::Stake {}).unwrap(),
    };
    app.execute_contract(
        Addr::unchecked(CREATOR_ADDR),
        token_contract.clone(),
        &msg,
        &[],
    )
    .unwrap();
    app.update_block(next_block);

    // Send some tokens to the proposal module so it has the ability
    // to double refund if the code is buggy.
    let msg = cw20::Cw20ExecuteMsg::Transfer {
        recipient: govmod.to_string(),
        amount: Uint128::new(1),
    };
    app.execute_contract(
        Addr::unchecked(CREATOR_ADDR),
        token_contract.clone(),
        &msg,
        &[],
    )
    .unwrap();

    let msg = cw20::Cw20ExecuteMsg::Burn {
        amount: Uint128::new(2000),
    };
    let binary_msg = to_binary(&msg).unwrap();

    // Increase allowance to pay the proposal deposit.
    app.execute_contract(
        Addr::unchecked(CREATOR_ADDR),
        token_contract.clone(),
        &cw20_base::msg::ExecuteMsg::IncreaseAllowance {
            spender: govmod.to_string(),
            amount: Uint128::new(1),
            expires: None,
        },
        &[],
    )
    .unwrap();

    let choices = MultipleChoiceOptions {
        options: vec![
            MultipleChoiceOption {
                description: "Burning more tokens, than dao treasury have".to_string(),
                msgs: vec![WasmMsg::Execute {
                    contract_addr: token_contract.to_string(),
                    msg: binary_msg,
                    funds: vec![],
                }
                .into()],
                title: "title".to_string(),
            },
            MultipleChoiceOption {
                description: "hi there".to_string(),
                msgs: vec![],
                title: "title".to_string(),
            },
        ],
    };

    make_proposal(
        &mut app,
        &govmod,
        Addr::unchecked(CREATOR_ADDR).as_str(),
        choices,
    );

    // Vote on proposal
    app.execute_contract(
        Addr::unchecked(CREATOR_ADDR),
        govmod.clone(),
        &ExecuteMsg::Vote {
            proposal_id: 1,
            vote: MultipleChoiceVote { option_id: 0 },
        },
        &[],
    )
    .unwrap();

    // Update block
    let timestamp = Timestamp::from_seconds(300_000_000);
    app.update_block(|block| block.time = timestamp);

    // Execute proposal
    app.execute_contract(
        Addr::unchecked(CREATOR_ADDR),
        govmod.clone(),
        &ExecuteMsg::Execute { proposal_id: 1 },
        &[],
    )
    .unwrap();

    let failed: ProposalResponse = query_proposal(&app, &govmod, 1);

    assert_eq!(failed.proposal.status, Status::ExecutionFailed);

    // Check that our deposit has been refunded.
    let balance = query_balance_cw20(&app, token_contract.to_string(), CREATOR_ADDR);
    assert_eq!(balance, Uint128::new(1));

    // Close the proposal - this should fail as it was executed.
    let err: ContractError = app
        .execute_contract(
            Addr::unchecked(CREATOR_ADDR),
            govmod,
            &ExecuteMsg::Close { proposal_id: 1 },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();

    assert!(matches!(err, ContractError::WrongCloseStatus {}));

    // Check that our deposit was not refunded a second time on close.
    let balance = query_balance_cw20(&app, token_contract.to_string(), CREATOR_ADDR);
    assert_eq!(balance, Uint128::new(1));
}
