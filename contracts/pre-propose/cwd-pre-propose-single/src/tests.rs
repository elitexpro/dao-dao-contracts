use cosmwasm_std::{coins, from_slice, to_binary, Addr, Coin, Empty, Uint128};
use cps::query::ProposalResponse;
use cw2::ContractVersion;
use cw20::Cw20Coin;
use cw_denom::UncheckedDenom;
use cw_multi_test::{App, BankSudo, Contract, ContractWrapper, Executor};
use cw_utils::Duration;
use cwd_core::state::ProposalModule;
use cwd_interface::{Admin, ModuleInstantiateInfo};
use cwd_pre_propose_base::{error::PreProposeError, msg::DepositInfoResponse, state::Config};
use cwd_proposal_single as cps;
use cwd_testing::helpers::instantiate_with_cw4_groups_governance;
use cwd_voting::{
    deposit::{CheckedDepositInfo, DepositRefundPolicy, DepositToken, UncheckedDepositInfo},
    pre_propose::{PreProposeInfo, ProposalCreationPolicy},
    status::Status,
    threshold::{PercentageThreshold, Threshold},
    voting::Vote,
};

use crate::contract::*;

fn cw_dao_proposal_single_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        cps::contract::execute,
        cps::contract::instantiate,
        cps::contract::query,
    )
    .with_migrate(cps::contract::migrate)
    .with_reply(cps::contract::reply);
    Box::new(contract)
}

fn cw_pre_propose_base_proposal_single() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(execute, instantiate, query);
    Box::new(contract)
}

fn cw20_base_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        cw20_base::contract::execute,
        cw20_base::contract::instantiate,
        cw20_base::contract::query,
    );
    Box::new(contract)
}

fn get_default_proposal_module_instantiate(
    app: &mut App,
    deposit_info: Option<UncheckedDepositInfo>,
    open_proposal_submission: bool,
) -> cps::msg::InstantiateMsg {
    let pre_propose_id = app.store_code(cw_pre_propose_base_proposal_single());

    cps::msg::InstantiateMsg {
        threshold: Threshold::AbsolutePercentage {
            percentage: PercentageThreshold::Majority {},
        },
        max_voting_period: cw_utils::Duration::Time(86400),
        min_voting_period: None,
        only_members_execute: false,
        allow_revoting: false,
        pre_propose_info: PreProposeInfo::ModuleMayPropose {
            info: ModuleInstantiateInfo {
                code_id: pre_propose_id,
                msg: to_binary(&InstantiateMsg {
                    deposit_info,
                    open_proposal_submission,
                    extension: Empty::default(),
                })
                .unwrap(),
                admin: Some(Admin::CoreModule {}),
                label: "baby's first pre-propose module".to_string(),
            },
        },
        close_proposal_on_execution_failure: false,
    }
}

fn instantiate_cw20_base_default(app: &mut App) -> Addr {
    let cw20_id = app.store_code(cw20_base_contract());
    let cw20_instantiate = cw20_base::msg::InstantiateMsg {
        name: "cw20 token".to_string(),
        symbol: "cwtwenty".to_string(),
        decimals: 6,
        initial_balances: vec![Cw20Coin {
            address: "ekez".to_string(),
            amount: Uint128::new(10),
        }],
        mint: None,
        marketing: None,
    };
    app.instantiate_contract(
        cw20_id,
        Addr::unchecked("ekez"),
        &cw20_instantiate,
        &[],
        "cw20-base",
        None,
    )
    .unwrap()
}

struct DefaultTestSetup {
    core_addr: Addr,
    proposal_single: Addr,
    pre_propose: Addr,
}
fn setup_default_test(
    app: &mut App,
    deposit_info: Option<UncheckedDepositInfo>,
    open_proposal_submission: bool,
) -> DefaultTestSetup {
    let cps_id = app.store_code(cw_dao_proposal_single_contract());

    let proposal_module_instantiate =
        get_default_proposal_module_instantiate(app, deposit_info, open_proposal_submission);

    let core_addr = instantiate_with_cw4_groups_governance(
        app,
        cps_id,
        to_binary(&proposal_module_instantiate).unwrap(),
        Some(vec![
            cw20::Cw20Coin {
                address: "ekez".to_string(),
                amount: Uint128::new(9),
            },
            cw20::Cw20Coin {
                address: "keze".to_string(),
                amount: Uint128::new(8),
            },
        ]),
    );
    let proposal_modules: Vec<ProposalModule> = app
        .wrap()
        .query_wasm_smart(
            core_addr.clone(),
            &cwd_core::msg::QueryMsg::ProposalModules {
                start_after: None,
                limit: None,
            },
        )
        .unwrap();

    assert_eq!(proposal_modules.len(), 1);
    let proposal_single = proposal_modules.into_iter().next().unwrap().address;
    let proposal_creation_policy = app
        .wrap()
        .query_wasm_smart(
            proposal_single.clone(),
            &cps::msg::QueryMsg::ProposalCreationPolicy {},
        )
        .unwrap();

    let pre_propose = match proposal_creation_policy {
        ProposalCreationPolicy::Module { addr } => addr,
        _ => panic!("expected a module for the proposal creation policy"),
    };

    // Make sure things were set up correctly.
    assert_eq!(
        proposal_single,
        get_proposal_module(app, pre_propose.clone())
    );
    assert_eq!(core_addr, get_dao(app, pre_propose.clone()));

    DefaultTestSetup {
        core_addr,
        proposal_single,
        pre_propose,
    }
}

fn make_proposal(
    app: &mut App,
    pre_propose: Addr,
    proposal_module: Addr,
    proposer: &str,
    funds: &[Coin],
) -> u64 {
    let res = app
        .execute_contract(
            Addr::unchecked(proposer),
            pre_propose,
            &ExecuteMsg::Propose {
                msg: ProposeMessage::Propose {
                    title: "title".to_string(),
                    description: "description".to_string(),
                    msgs: vec![],
                },
            },
            funds,
        )
        .unwrap();

    // The new proposal hook is the last message that fires in
    // this process so we get the proposal ID from it's
    // attributes. We could do this by looking at the proposal
    // creation attributes but this changes relative position
    // depending on if a cw20 or native deposit is being used.
    let attrs = res.custom_attrs(res.events.len() - 1);
    let id = attrs[attrs.len() - 1].value.parse().unwrap();
    let proposal: ProposalResponse = app
        .wrap()
        .query_wasm_smart(
            proposal_module,
            &cps::msg::QueryMsg::Proposal { proposal_id: id },
        )
        .unwrap();

    assert_eq!(proposal.proposal.proposer, Addr::unchecked(proposer));
    assert_eq!(proposal.proposal.title, "title".to_string());
    assert_eq!(proposal.proposal.description, "description".to_string());
    assert_eq!(proposal.proposal.msgs, vec![]);

    id
}

fn mint_natives(app: &mut App, receiver: &str, coins: Vec<Coin>) {
    // Mint some ekez tokens for ekez so we can pay the deposit.
    app.sudo(cw_multi_test::SudoMsg::Bank(BankSudo::Mint {
        to_address: receiver.to_string(),
        amount: coins,
    }))
    .unwrap();
}

fn increase_allowance(app: &mut App, sender: &str, receiver: &Addr, cw20: Addr, amount: Uint128) {
    app.execute_contract(
        Addr::unchecked(sender),
        cw20,
        &cw20::Cw20ExecuteMsg::IncreaseAllowance {
            spender: receiver.to_string(),
            amount,
            expires: None,
        },
        &[],
    )
    .unwrap();
}

fn get_balance_cw20<T: Into<String>, U: Into<String>>(
    app: &App,
    contract_addr: T,
    address: U,
) -> Uint128 {
    let msg = cw20::Cw20QueryMsg::Balance {
        address: address.into(),
    };
    let result: cw20::BalanceResponse = app.wrap().query_wasm_smart(contract_addr, &msg).unwrap();
    result.balance
}

fn get_balance_native(app: &App, who: &str, denom: &str) -> Uint128 {
    let res = app.wrap().query_balance(who, denom).unwrap();
    res.amount
}

fn vote(app: &mut App, module: Addr, sender: &str, id: u64, position: Vote) -> Status {
    app.execute_contract(
        Addr::unchecked(sender),
        module.clone(),
        &cps::msg::ExecuteMsg::Vote {
            rationale: None,
            proposal_id: id,
            vote: position,
        },
        &[],
    )
    .unwrap();

    let proposal: ProposalResponse = app
        .wrap()
        .query_wasm_smart(module, &cps::msg::QueryMsg::Proposal { proposal_id: id })
        .unwrap();

    proposal.proposal.status
}

fn get_config(app: &App, module: Addr) -> Config {
    app.wrap()
        .query_wasm_smart(module, &QueryMsg::Config {})
        .unwrap()
}

fn get_dao(app: &App, module: Addr) -> Addr {
    app.wrap()
        .query_wasm_smart(module, &QueryMsg::Dao {})
        .unwrap()
}

fn get_proposal_module(app: &App, module: Addr) -> Addr {
    app.wrap()
        .query_wasm_smart(module, &QueryMsg::ProposalModule {})
        .unwrap()
}

fn get_deposit_info(app: &App, module: Addr, id: u64) -> DepositInfoResponse {
    app.wrap()
        .query_wasm_smart(module, &QueryMsg::DepositInfo { proposal_id: id })
        .unwrap()
}

fn update_config(
    app: &mut App,
    module: Addr,
    sender: &str,
    deposit_info: Option<UncheckedDepositInfo>,
    open_proposal_submission: bool,
) -> Config {
    app.execute_contract(
        Addr::unchecked(sender),
        module.clone(),
        &ExecuteMsg::UpdateConfig {
            deposit_info,
            open_proposal_submission,
        },
        &[],
    )
    .unwrap();

    get_config(app, module)
}

fn update_config_should_fail(
    app: &mut App,
    module: Addr,
    sender: &str,
    deposit_info: Option<UncheckedDepositInfo>,
    open_proposal_submission: bool,
) -> PreProposeError {
    app.execute_contract(
        Addr::unchecked(sender),
        module,
        &ExecuteMsg::UpdateConfig {
            deposit_info,
            open_proposal_submission,
        },
        &[],
    )
    .unwrap_err()
    .downcast()
    .unwrap()
}

fn withdraw(app: &mut App, module: Addr, sender: &str, denom: Option<UncheckedDenom>) {
    app.execute_contract(
        Addr::unchecked(sender),
        module,
        &ExecuteMsg::Withdraw { denom },
        &[],
    )
    .unwrap();
}

fn withdraw_should_fail(
    app: &mut App,
    module: Addr,
    sender: &str,
    denom: Option<UncheckedDenom>,
) -> PreProposeError {
    app.execute_contract(
        Addr::unchecked(sender),
        module,
        &ExecuteMsg::Withdraw { denom },
        &[],
    )
    .unwrap_err()
    .downcast()
    .unwrap()
}

fn close_proposal(app: &mut App, module: Addr, sender: &str, proposal_id: u64) {
    app.execute_contract(
        Addr::unchecked(sender),
        module,
        &cps::msg::ExecuteMsg::Close { proposal_id },
        &[],
    )
    .unwrap();
}

fn execute_proposal(app: &mut App, module: Addr, sender: &str, proposal_id: u64) {
    app.execute_contract(
        Addr::unchecked(sender),
        module,
        &cps::msg::ExecuteMsg::Execute { proposal_id },
        &[],
    )
    .unwrap();
}

enum EndStatus {
    Passed,
    Failed,
}
enum RefundReceiver {
    Proposer,
    Dao,
}

fn test_native_permutation(
    end_status: EndStatus,
    refund_policy: DepositRefundPolicy,
    receiver: RefundReceiver,
) {
    let mut app = App::default();

    let DefaultTestSetup {
        core_addr,
        proposal_single,
        pre_propose,
    } = setup_default_test(
        &mut app,
        Some(UncheckedDepositInfo {
            denom: DepositToken::Token {
                denom: UncheckedDenom::Native("ujuno".to_string()),
            },
            amount: Uint128::new(10),
            refund_policy,
        }),
        false,
    );

    mint_natives(&mut app, "ekez", coins(10, "ujuno"));
    let id = make_proposal(
        &mut app,
        pre_propose,
        proposal_single.clone(),
        "ekez",
        &coins(10, "ujuno"),
    );

    // Make sure it went away.
    let balance = get_balance_native(&app, "ekez", "ujuno");
    assert_eq!(balance, Uint128::zero());

    #[allow(clippy::type_complexity)]
    let (position, expected_status, trigger_refund): (
        _,
        _,
        fn(&mut App, Addr, &str, u64) -> (),
    ) = match end_status {
        EndStatus::Passed => (Vote::Yes, Status::Passed, execute_proposal),
        EndStatus::Failed => (Vote::No, Status::Rejected, close_proposal),
    };
    let new_status = vote(&mut app, proposal_single.clone(), "ekez", id, position);
    assert_eq!(new_status, expected_status);

    // Close or execute the proposal to trigger a refund.
    trigger_refund(&mut app, proposal_single, "ekez", id);

    let (dao_expected, proposer_expected) = match receiver {
        RefundReceiver::Proposer => (0, 10),
        RefundReceiver::Dao => (10, 0),
    };

    let proposer_balance = get_balance_native(&app, "ekez", "ujuno");
    let dao_balance = get_balance_native(&app, core_addr.as_str(), "ujuno");
    assert_eq!(proposer_expected, proposer_balance.u128());
    assert_eq!(dao_expected, dao_balance.u128())
}

fn test_cw20_permutation(
    end_status: EndStatus,
    refund_policy: DepositRefundPolicy,
    receiver: RefundReceiver,
) {
    let mut app = App::default();

    let cw20_address = instantiate_cw20_base_default(&mut app);

    let DefaultTestSetup {
        core_addr,
        proposal_single,
        pre_propose,
    } = setup_default_test(
        &mut app,
        Some(UncheckedDepositInfo {
            denom: DepositToken::Token {
                denom: UncheckedDenom::Cw20(cw20_address.to_string()),
            },
            amount: Uint128::new(10),
            refund_policy,
        }),
        false,
    );

    increase_allowance(
        &mut app,
        "ekez",
        &pre_propose,
        cw20_address.clone(),
        Uint128::new(10),
    );
    let id = make_proposal(
        &mut app,
        pre_propose.clone(),
        proposal_single.clone(),
        "ekez",
        &[],
    );

    // Make sure it went await.
    let balance = get_balance_cw20(&app, cw20_address.clone(), "ekez");
    assert_eq!(balance, Uint128::zero());

    #[allow(clippy::type_complexity)]
    let (position, expected_status, trigger_refund): (
        _,
        _,
        fn(&mut App, Addr, &str, u64) -> (),
    ) = match end_status {
        EndStatus::Passed => (Vote::Yes, Status::Passed, execute_proposal),
        EndStatus::Failed => (Vote::No, Status::Rejected, close_proposal),
    };
    let new_status = vote(&mut app, proposal_single.clone(), "ekez", id, position);
    assert_eq!(new_status, expected_status);

    // Close or execute the proposal to trigger a refund.
    trigger_refund(&mut app, proposal_single, "ekez", id);

    let (dao_expected, proposer_expected) = match receiver {
        RefundReceiver::Proposer => (0, 10),
        RefundReceiver::Dao => (10, 0),
    };

    let proposer_balance = get_balance_cw20(&app, &cw20_address, "ekez");
    let dao_balance = get_balance_cw20(&app, &cw20_address, core_addr);
    assert_eq!(proposer_expected, proposer_balance.u128());
    assert_eq!(dao_expected, dao_balance.u128())
}

#[test]
fn test_native_failed_always_refund() {
    test_native_permutation(
        EndStatus::Failed,
        DepositRefundPolicy::Always,
        RefundReceiver::Proposer,
    )
}
#[test]
fn test_cw20_failed_always_refund() {
    test_cw20_permutation(
        EndStatus::Failed,
        DepositRefundPolicy::Always,
        RefundReceiver::Proposer,
    )
}

#[test]
fn test_native_passed_always_refund() {
    test_native_permutation(
        EndStatus::Passed,
        DepositRefundPolicy::Always,
        RefundReceiver::Proposer,
    )
}

#[test]
fn test_cw20_passed_always_refund() {
    test_cw20_permutation(
        EndStatus::Passed,
        DepositRefundPolicy::Always,
        RefundReceiver::Proposer,
    )
}

#[test]
fn test_native_passed_never_refund() {
    test_native_permutation(
        EndStatus::Passed,
        DepositRefundPolicy::Never,
        RefundReceiver::Dao,
    )
}
#[test]
fn test_cw20_passed_never_refund() {
    test_cw20_permutation(
        EndStatus::Passed,
        DepositRefundPolicy::Never,
        RefundReceiver::Dao,
    )
}

#[test]
fn test_native_failed_never_refund() {
    test_native_permutation(
        EndStatus::Failed,
        DepositRefundPolicy::Never,
        RefundReceiver::Dao,
    )
}
#[test]
fn test_cw20_failed_never_refund() {
    test_cw20_permutation(
        EndStatus::Failed,
        DepositRefundPolicy::Never,
        RefundReceiver::Dao,
    )
}

#[test]
fn test_native_passed_passed_refund() {
    test_native_permutation(
        EndStatus::Passed,
        DepositRefundPolicy::OnlyPassed,
        RefundReceiver::Proposer,
    )
}
#[test]
fn test_cw20_passed_passed_refund() {
    test_cw20_permutation(
        EndStatus::Passed,
        DepositRefundPolicy::OnlyPassed,
        RefundReceiver::Proposer,
    )
}

#[test]
fn test_native_failed_passed_refund() {
    test_native_permutation(
        EndStatus::Failed,
        DepositRefundPolicy::OnlyPassed,
        RefundReceiver::Dao,
    )
}
#[test]
fn test_cw20_failed_passed_refund() {
    test_cw20_permutation(
        EndStatus::Failed,
        DepositRefundPolicy::OnlyPassed,
        RefundReceiver::Dao,
    )
}

// See: <https://github.com/DA0-DA0/dao-contracts/pull/465#discussion_r960092321>
#[test]
fn test_multiple_open_proposals() {
    let mut app = App::default();

    let DefaultTestSetup {
        core_addr: _,
        proposal_single,
        pre_propose,
    } = setup_default_test(
        &mut app,
        Some(UncheckedDepositInfo {
            denom: DepositToken::Token {
                denom: UncheckedDenom::Native("ujuno".to_string()),
            },
            amount: Uint128::new(10),
            refund_policy: DepositRefundPolicy::Always,
        }),
        false,
    );

    mint_natives(&mut app, "ekez", coins(20, "ujuno"));
    let first_id = make_proposal(
        &mut app,
        pre_propose.clone(),
        proposal_single.clone(),
        "ekez",
        &coins(10, "ujuno"),
    );
    let balance = get_balance_native(&app, "ekez", "ujuno");
    assert_eq!(10, balance.u128());

    let second_id = make_proposal(
        &mut app,
        pre_propose,
        proposal_single.clone(),
        "ekez",
        &coins(10, "ujuno"),
    );
    let balance = get_balance_native(&app, "ekez", "ujuno");
    assert_eq!(0, balance.u128());

    // Finish up the first proposal.
    let new_status = vote(
        &mut app,
        proposal_single.clone(),
        "ekez",
        first_id,
        Vote::Yes,
    );
    assert_eq!(Status::Passed, new_status);

    // Still zero.
    let balance = get_balance_native(&app, "ekez", "ujuno");
    assert_eq!(0, balance.u128());

    execute_proposal(&mut app, proposal_single.clone(), "ekez", first_id);

    // First proposal refunded.
    let balance = get_balance_native(&app, "ekez", "ujuno");
    assert_eq!(10, balance.u128());

    // Finish up the second proposal.
    let new_status = vote(
        &mut app,
        proposal_single.clone(),
        "ekez",
        second_id,
        Vote::No,
    );
    assert_eq!(Status::Rejected, new_status);

    // Still zero.
    let balance = get_balance_native(&app, "ekez", "ujuno");
    assert_eq!(10, balance.u128());

    close_proposal(&mut app, proposal_single, "ekez", second_id);

    // All deposits have been refunded.
    let balance = get_balance_native(&app, "ekez", "ujuno");
    assert_eq!(20, balance.u128());
}

#[test]
fn test_set_version() {
    let mut app = App::default();

    let DefaultTestSetup {
        core_addr: _,
        proposal_single: _,
        pre_propose,
    } = setup_default_test(
        &mut app,
        Some(UncheckedDepositInfo {
            denom: DepositToken::Token {
                denom: UncheckedDenom::Native("ujuno".to_string()),
            },
            amount: Uint128::new(10),
            refund_policy: DepositRefundPolicy::Always,
        }),
        false,
    );

    let info: ContractVersion = from_slice(
        &app.wrap()
            .query_wasm_raw(pre_propose, "contract_info".as_bytes())
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        ContractVersion {
            contract: CONTRACT_NAME.to_string(),
            version: CONTRACT_VERSION.to_string()
        },
        info
    )
}

#[test]
fn test_permissions() {
    let mut app = App::default();

    let DefaultTestSetup {
        core_addr,
        proposal_single: _,
        pre_propose,
    } = setup_default_test(
        &mut app,
        Some(UncheckedDepositInfo {
            denom: DepositToken::Token {
                denom: UncheckedDenom::Native("ujuno".to_string()),
            },
            amount: Uint128::new(10),
            refund_policy: DepositRefundPolicy::Always,
        }),
        false, // no open proposal submission.
    );

    let err: PreProposeError = app
        .execute_contract(
            Addr::unchecked("notmodule"),
            pre_propose.clone(),
            &ExecuteMsg::ProposalCreatedHook {
                proposal_id: 1,
                proposer: "ekez".to_string(),
            },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(err, PreProposeError::NotModule {});

    let err: PreProposeError = app
        .execute_contract(
            core_addr,
            pre_propose.clone(),
            &ExecuteMsg::ProposalCompletedHook {
                proposal_id: 1,
                new_status: Status::Closed,
            },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(err, PreProposeError::NotModule {});

    // Non-members may not propose when open_propose_submission is
    // disabled.
    let err: PreProposeError = app
        .execute_contract(
            Addr::unchecked("nonmember"),
            pre_propose,
            &ExecuteMsg::Propose {
                msg: ProposeMessage::Propose {
                    title: "I would like to join the DAO".to_string(),
                    description: "though, I am currently not a member.".to_string(),
                    msgs: vec![],
                },
            },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(err, PreProposeError::NotMember {})
}

#[test]
fn test_propose_open_proposal_submission() {
    let mut app = App::default();
    let DefaultTestSetup {
        core_addr: _,
        proposal_single,
        pre_propose,
    } = setup_default_test(
        &mut app,
        Some(UncheckedDepositInfo {
            denom: DepositToken::Token {
                denom: UncheckedDenom::Native("ujuno".to_string()),
            },
            amount: Uint128::new(10),
            refund_policy: DepositRefundPolicy::Always,
        }),
        true, // yes, open proposal submission.
    );

    // Non-member proposes.
    mint_natives(&mut app, "nonmember", coins(10, "ujuno"));
    let id = make_proposal(
        &mut app,
        pre_propose,
        proposal_single.clone(),
        "nonmember",
        &coins(10, "ujuno"),
    );
    // Member votes.
    let new_status = vote(&mut app, proposal_single, "ekez", id, Vote::Yes);
    assert_eq!(Status::Passed, new_status)
}

#[test]
fn test_no_deposit_required_open_submission() {
    let mut app = App::default();
    let DefaultTestSetup {
        core_addr: _,
        proposal_single,
        pre_propose,
    } = setup_default_test(
        &mut app, None, true, // yes, open proposal submission.
    );

    // Non-member proposes.
    let id = make_proposal(
        &mut app,
        pre_propose,
        proposal_single.clone(),
        "nonmember",
        &[],
    );
    // Member votes.
    let new_status = vote(&mut app, proposal_single, "ekez", id, Vote::Yes);
    assert_eq!(Status::Passed, new_status)
}

#[test]
fn test_no_deposit_required_members_submission() {
    let mut app = App::default();
    let DefaultTestSetup {
        core_addr: _,
        proposal_single,
        pre_propose,
    } = setup_default_test(
        &mut app, None, false, // no open proposal submission.
    );

    // Non-member proposes and this fails.
    let err: PreProposeError = app
        .execute_contract(
            Addr::unchecked("nonmember"),
            pre_propose.clone(),
            &ExecuteMsg::Propose {
                msg: ProposeMessage::Propose {
                    title: "I would like to join the DAO".to_string(),
                    description: "though, I am currently not a member.".to_string(),
                    msgs: vec![],
                },
            },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(err, PreProposeError::NotMember {});

    let id = make_proposal(&mut app, pre_propose, proposal_single.clone(), "ekez", &[]);
    let new_status = vote(&mut app, proposal_single, "ekez", id, Vote::Yes);
    assert_eq!(Status::Passed, new_status)
}

#[test]
fn test_execute_extension_does_nothing() {
    let mut app = App::default();
    let DefaultTestSetup {
        core_addr: _,
        proposal_single: _,
        pre_propose,
    } = setup_default_test(
        &mut app, None, false, // no open proposal submission.
    );

    let res = app
        .execute_contract(
            Addr::unchecked("ekez"),
            pre_propose,
            &ExecuteMsg::Extension {
                msg: Empty::default(),
            },
            &[],
        )
        .unwrap();

    // There should be one event which is the invocation of the contract.
    assert_eq!(res.events.len(), 1);
    assert_eq!(res.events[0].ty, "execute".to_string());
    assert_eq!(res.events[0].attributes.len(), 1);
    assert_eq!(
        res.events[0].attributes[0].key,
        "_contract_addr".to_string()
    )
}

#[test]
#[should_panic(expected = "invalid zero deposit. set the deposit to `None` to have no deposit")]
fn test_instantiate_with_zero_native_deposit() {
    let mut app = App::default();

    let cps_id = app.store_code(cw_dao_proposal_single_contract());

    let proposal_module_instantiate = {
        let pre_propose_id = app.store_code(cw_pre_propose_base_proposal_single());

        cps::msg::InstantiateMsg {
            threshold: Threshold::AbsolutePercentage {
                percentage: PercentageThreshold::Majority {},
            },
            max_voting_period: Duration::Time(86400),
            min_voting_period: None,
            only_members_execute: false,
            allow_revoting: false,
            pre_propose_info: PreProposeInfo::ModuleMayPropose {
                info: ModuleInstantiateInfo {
                    code_id: pre_propose_id,
                    msg: to_binary(&InstantiateMsg {
                        deposit_info: Some(UncheckedDepositInfo {
                            denom: DepositToken::Token {
                                denom: UncheckedDenom::Native("ujuno".to_string()),
                            },
                            amount: Uint128::zero(),
                            refund_policy: DepositRefundPolicy::OnlyPassed,
                        }),
                        open_proposal_submission: false,
                        extension: Empty::default(),
                    })
                    .unwrap(),
                    admin: Some(Admin::CoreModule {}),
                    label: "baby's first pre-propose module".to_string(),
                },
            },
            close_proposal_on_execution_failure: false,
        }
    };

    // Should panic.
    instantiate_with_cw4_groups_governance(
        &mut app,
        cps_id,
        to_binary(&proposal_module_instantiate).unwrap(),
        Some(vec![
            cw20::Cw20Coin {
                address: "ekez".to_string(),
                amount: Uint128::new(9),
            },
            cw20::Cw20Coin {
                address: "keze".to_string(),
                amount: Uint128::new(8),
            },
        ]),
    );
}

#[test]
#[should_panic(expected = "invalid zero deposit. set the deposit to `None` to have no deposit")]
fn test_instantiate_with_zero_cw20_deposit() {
    let mut app = App::default();

    let cw20_addr = instantiate_cw20_base_default(&mut app);

    let cps_id = app.store_code(cw_dao_proposal_single_contract());

    let proposal_module_instantiate = {
        let pre_propose_id = app.store_code(cw_pre_propose_base_proposal_single());

        cps::msg::InstantiateMsg {
            threshold: Threshold::AbsolutePercentage {
                percentage: PercentageThreshold::Majority {},
            },
            max_voting_period: Duration::Time(86400),
            min_voting_period: None,
            only_members_execute: false,
            allow_revoting: false,
            pre_propose_info: PreProposeInfo::ModuleMayPropose {
                info: ModuleInstantiateInfo {
                    code_id: pre_propose_id,
                    msg: to_binary(&InstantiateMsg {
                        deposit_info: Some(UncheckedDepositInfo {
                            denom: DepositToken::Token {
                                denom: UncheckedDenom::Cw20(cw20_addr.into_string()),
                            },
                            amount: Uint128::zero(),
                            refund_policy: DepositRefundPolicy::OnlyPassed,
                        }),
                        open_proposal_submission: false,
                        extension: Empty::default(),
                    })
                    .unwrap(),
                    admin: Some(Admin::CoreModule {}),
                    label: "baby's first pre-propose module".to_string(),
                },
            },
            close_proposal_on_execution_failure: false,
        }
    };

    // Should panic.
    instantiate_with_cw4_groups_governance(
        &mut app,
        cps_id,
        to_binary(&proposal_module_instantiate).unwrap(),
        Some(vec![
            cw20::Cw20Coin {
                address: "ekez".to_string(),
                amount: Uint128::new(9),
            },
            cw20::Cw20Coin {
                address: "keze".to_string(),
                amount: Uint128::new(8),
            },
        ]),
    );
}

#[test]
fn test_update_config() {
    let mut app = App::default();
    let DefaultTestSetup {
        core_addr,
        proposal_single,
        pre_propose,
    } = setup_default_test(&mut app, None, false);

    let config = get_config(&app, pre_propose.clone());
    assert_eq!(
        config,
        Config {
            deposit_info: None,
            open_proposal_submission: false
        }
    );

    let id = make_proposal(
        &mut app,
        pre_propose.clone(),
        proposal_single.clone(),
        "ekez",
        &[],
    );

    update_config(
        &mut app,
        pre_propose.clone(),
        core_addr.as_str(),
        Some(UncheckedDepositInfo {
            denom: DepositToken::Token {
                denom: UncheckedDenom::Native("ujuno".to_string()),
            },
            amount: Uint128::new(10),
            refund_policy: DepositRefundPolicy::Never,
        }),
        true,
    );

    let config = get_config(&app, pre_propose.clone());
    assert_eq!(
        config,
        Config {
            deposit_info: Some(CheckedDepositInfo {
                denom: cw_denom::CheckedDenom::Native("ujuno".to_string()),
                amount: Uint128::new(10),
                refund_policy: DepositRefundPolicy::Never
            }),
            open_proposal_submission: true,
        }
    );

    // Old proposal should still have same deposit info.
    let info = get_deposit_info(&app, pre_propose.clone(), id);
    assert_eq!(
        info,
        DepositInfoResponse {
            deposit_info: None,
            proposer: Addr::unchecked("ekez"),
        }
    );

    // New proposals should have the new deposit info.
    mint_natives(&mut app, "ekez", coins(10, "ujuno"));
    let new_id = make_proposal(
        &mut app,
        pre_propose.clone(),
        proposal_single.clone(),
        "ekez",
        &coins(10, "ujuno"),
    );
    let info = get_deposit_info(&app, pre_propose.clone(), new_id);
    assert_eq!(
        info,
        DepositInfoResponse {
            deposit_info: Some(CheckedDepositInfo {
                denom: cw_denom::CheckedDenom::Native("ujuno".to_string()),
                amount: Uint128::new(10),
                refund_policy: DepositRefundPolicy::Never
            }),
            proposer: Addr::unchecked("ekez"),
        }
    );

    // Both proposals should be allowed to complete.
    vote(&mut app, proposal_single.clone(), "ekez", id, Vote::Yes);
    vote(&mut app, proposal_single.clone(), "ekez", new_id, Vote::Yes);
    execute_proposal(&mut app, proposal_single.clone(), "ekez", id);
    execute_proposal(&mut app, proposal_single.clone(), "ekez", new_id);
    // Deposit should not have been refunded (never policy in use).
    let balance = get_balance_native(&app, "ekez", "ujuno");
    assert_eq!(balance, Uint128::new(0));

    // Only the core module can update the config.
    let err =
        update_config_should_fail(&mut app, pre_propose, proposal_single.as_str(), None, true);
    assert_eq!(err, PreProposeError::NotDao {});
}

#[test]
fn test_withdraw() {
    let mut app = App::default();

    let DefaultTestSetup {
        core_addr,
        proposal_single,
        pre_propose,
    } = setup_default_test(&mut app, None, false);

    let err = withdraw_should_fail(
        &mut app,
        pre_propose.clone(),
        proposal_single.as_str(),
        Some(UncheckedDenom::Native("ujuno".to_string())),
    );
    assert_eq!(err, PreProposeError::NotDao {});

    let err = withdraw_should_fail(
        &mut app,
        pre_propose.clone(),
        core_addr.as_str(),
        Some(UncheckedDenom::Native("ujuno".to_string())),
    );
    assert_eq!(err, PreProposeError::NothingToWithdraw {});

    let err = withdraw_should_fail(&mut app, pre_propose.clone(), core_addr.as_str(), None);
    assert_eq!(err, PreProposeError::NoWithdrawalDenom {});

    // Turn on native deposits.
    update_config(
        &mut app,
        pre_propose.clone(),
        core_addr.as_str(),
        Some(UncheckedDepositInfo {
            denom: DepositToken::Token {
                denom: UncheckedDenom::Native("ujuno".to_string()),
            },
            amount: Uint128::new(10),
            refund_policy: DepositRefundPolicy::Always,
        }),
        false,
    );

    // Withdraw with no specified denom - should fall back to the one
    // in the config.
    mint_natives(&mut app, pre_propose.as_str(), coins(10, "ujuno"));
    withdraw(&mut app, pre_propose.clone(), core_addr.as_str(), None);
    let balance = get_balance_native(&app, core_addr.as_str(), "ujuno");
    assert_eq!(balance, Uint128::new(10));

    // Withdraw again, this time specifying a native denomination.
    mint_natives(&mut app, pre_propose.as_str(), coins(10, "ujuno"));
    withdraw(
        &mut app,
        pre_propose.clone(),
        core_addr.as_str(),
        Some(UncheckedDenom::Native("ujuno".to_string())),
    );
    let balance = get_balance_native(&app, core_addr.as_str(), "ujuno");
    assert_eq!(balance, Uint128::new(20));

    // Make a proposal with the native tokens to put some in the system.
    mint_natives(&mut app, "ekez", coins(10, "ujuno"));
    let native_id = make_proposal(
        &mut app,
        pre_propose.clone(),
        proposal_single.clone(),
        "ekez",
        &coins(10, "ujuno"),
    );

    // Update the config to use a cw20 token.
    let cw20_address = instantiate_cw20_base_default(&mut app);
    update_config(
        &mut app,
        pre_propose.clone(),
        core_addr.as_str(),
        Some(UncheckedDepositInfo {
            denom: DepositToken::Token {
                denom: UncheckedDenom::Cw20(cw20_address.to_string()),
            },
            amount: Uint128::new(10),
            refund_policy: DepositRefundPolicy::Always,
        }),
        false,
    );

    increase_allowance(
        &mut app,
        "ekez",
        &pre_propose,
        cw20_address.clone(),
        Uint128::new(10),
    );
    let cw20_id = make_proposal(
        &mut app,
        pre_propose.clone(),
        proposal_single.clone(),
        "ekez",
        &[],
    );

    // There is now a pending proposal and cw20 tokens in the
    // pre-propose module that should be returned on that proposal's
    // completion. To make things interesting, we withdraw those
    // tokens which should cause the status change hook on the
    // proposal's execution to fail as we don't have sufficent balance
    // to return the deposit.
    withdraw(&mut app, pre_propose.clone(), core_addr.as_str(), None);
    let balance = get_balance_cw20(&app, &cw20_address, core_addr.as_str());
    assert_eq!(balance, Uint128::new(10));

    // Proposal should still be executable! We just get removed from
    // the proposal module's hook receiver list.
    vote(
        &mut app,
        proposal_single.clone(),
        "ekez",
        cw20_id,
        Vote::Yes,
    );
    execute_proposal(&mut app, proposal_single.clone(), "ekez", cw20_id);

    // Make sure the proposal module has fallen back to anyone can
    // propose becuase of our malfunction.
    let proposal_creation_policy: ProposalCreationPolicy = app
        .wrap()
        .query_wasm_smart(
            proposal_single.clone(),
            &cps::msg::QueryMsg::ProposalCreationPolicy {},
        )
        .unwrap();

    assert_eq!(proposal_creation_policy, ProposalCreationPolicy::Anyone {});

    // Close out the native proposal and it's deposit as well.
    vote(
        &mut app,
        proposal_single.clone(),
        "ekez",
        native_id,
        Vote::No,
    );
    close_proposal(&mut app, proposal_single.clone(), "ekez", native_id);
    withdraw(
        &mut app,
        pre_propose.clone(),
        core_addr.as_str(),
        Some(UncheckedDenom::Native("ujuno".to_string())),
    );
    let balance = get_balance_native(&app, core_addr.as_str(), "ujuno");
    assert_eq!(balance, Uint128::new(30));
}
