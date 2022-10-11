use cosmwasm_std::{
    entry_point, to_binary, Addr, Binary, Deps, DepsMut, Empty, Env, MessageInfo, Reply, Response,
    StdResult, Storage, SubMsg, WasmMsg,
};

use cw2::set_contract_version;
use cwd_interface::voting::IsActiveResponse;
use cw_storage_plus::Bound;
use cw_utils::Duration;
use cwd_hooks::Hooks;
use cwd_proposal_hooks::{new_proposal_hooks, proposal_status_changed_hooks};

use cwd_vote_hooks::new_vote_hooks;
use cwd_voting::{
    deposit::{get_deposit_msg, get_return_deposit_msg, DepositRefundPolicy, UncheckedDepositInfo},
    proposal::{DEFAULT_LIMIT, MAX_PROPOSAL_SIZE},
    reply::{mask_proposal_execution_proposal_id, TaggedReplyId},
    status::Status,
    voting::{
        get_total_power, get_voting_power, validate_voting_period, MultipleChoiceVote,
        MultipleChoiceVotes,
    },
};

use crate::{
    msg::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg},
    proposal::{MultipleChoiceProposal, VoteResult},
    query::{ProposalListResponse, ProposalResponse, VoteListResponse, VoteResponse},
    state::{Config, MultipleChoiceOptions, CONFIG, PROPOSAL_COUNT, PROPOSAL_HOOKS, VOTE_HOOKS},
    voting_strategy::VotingStrategy,
    ContractError,
};

use crate::state::{Ballot, VoteInfo, BALLOTS, PROPOSALS};

pub const CONTRACT_NAME: &str = "crates.io:cw-proposal-multiple";
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    msg.voting_strategy.validate()?;

    let dao = info.sender;
    let deposit_info = msg
        .deposit_info
        .map(|info| info.into_checked(deps.as_ref(), dao.clone()))
        .transpose()?;

    let (min_voting_period, max_voting_period) =
        validate_voting_period(msg.min_voting_period, msg.max_voting_period)?;

    let config = Config {
        voting_strategy: msg.voting_strategy,
        min_voting_period,
        max_voting_period,
        only_members_execute: msg.only_members_execute,
        allow_revoting: msg.allow_revoting,
        dao,
        deposit_info,
        close_proposal_on_execution_failure: msg.close_proposal_on_execution_failure,
        open_proposal_submission: msg.open_proposal_submission,
    };

    // Initialize proposal count to zero.
    PROPOSAL_COUNT.save(deps.storage, &0)?;
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::default()
        .add_attribute("action", "instantiate")
        .add_attribute("dao", config.dao))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<Empty>, ContractError> {
    match msg {
        ExecuteMsg::Propose {
            title,
            description,
            choices,
        } => execute_propose(deps, env, info.sender, title, description, choices),
        ExecuteMsg::Vote { proposal_id, vote } => execute_vote(deps, env, info, proposal_id, vote),
        ExecuteMsg::Execute { proposal_id } => execute_execute(deps, env, info, proposal_id),
        ExecuteMsg::Close { proposal_id } => execute_close(deps, env, info, proposal_id),
        ExecuteMsg::UpdateConfig {
            voting_strategy,
            min_voting_period,
            max_voting_period,
            only_members_execute,
            allow_revoting,
            dao,
            deposit_info,
            close_proposal_on_execution_failure,
            open_proposal_submission,
        } => execute_update_config(
            deps,
            info,
            voting_strategy,
            min_voting_period,
            max_voting_period,
            only_members_execute,
            allow_revoting,
            dao,
            deposit_info,
            close_proposal_on_execution_failure,
            open_proposal_submission,
        ),
        ExecuteMsg::AddProposalHook { address } => {
            execute_add_proposal_hook(deps, env, info, address)
        }
        ExecuteMsg::RemoveProposalHook { address } => {
            execute_remove_proposal_hook(deps, env, info, address)
        }
        ExecuteMsg::AddVoteHook { address } => execute_add_vote_hook(deps, env, info, address),
        ExecuteMsg::RemoveVoteHook { address } => {
            execute_remove_vote_hook(deps, env, info, address)
        }
    }
}

pub fn execute_propose(
    deps: DepsMut,
    env: Env,
    sender: Addr,
    title: String,
    description: String,
    options: MultipleChoiceOptions,
) -> Result<Response<Empty>, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let voting_module: Addr = deps
        .querier
        .query_wasm_smart(config.dao.clone(), &cwd_core::msg::QueryMsg::VotingModule {})?;

    // Voting modules are not required to implement this
    // query. Lacking an implementation they are active by default.
    let active_resp: IsActiveResponse = deps
        .querier
        .query_wasm_smart(
            voting_module,
            &cwd_interface::voting::Query::IsActive {},
        )
        .unwrap_or(IsActiveResponse { active: true });

    if !active_resp.active {
        return Err(ContractError::InactiveDao {});
    }

    // Check whether open_propsoal_submissions are disabled
    if !config.open_proposal_submission {
        // If open_proposal_submission is false,
        // Check that the sender is a member of the governance contract.
        let sender_power =
            get_voting_power(deps.as_ref(), sender.clone(), config.dao.clone(), None)?;
        if sender_power.is_zero() {
            return Err(ContractError::MustHaveVotingPower {});
        }
    }

    // Validate options.
    let checked_multiple_choice_options = options.into_checked()?.options;

    let expiration = config.max_voting_period.after(&env.block);
    let total_power = get_total_power(deps.as_ref(), config.dao, None)?;

    let proposal = {
        // Limit mutability to this block.
        let mut proposal = MultipleChoiceProposal {
            title,
            description,
            proposer: sender.clone(),
            start_height: env.block.height,
            min_voting_period: config.min_voting_period.map(|min| min.after(&env.block)),
            expiration,
            voting_strategy: config.voting_strategy,
            total_power,
            status: Status::Open,
            votes: MultipleChoiceVotes::zero(checked_multiple_choice_options.len()),
            allow_revoting: config.allow_revoting,
            deposit_info: config.deposit_info.clone(),
            choices: checked_multiple_choice_options,
            created: env.block.time,
            last_updated: env.block.time,
        };
        // Update the proposal's status. Addresses case where proposal
        // expires on the same block as it is created.
        proposal.update_status(&env.block)?;
        proposal
    };
    let id = advance_proposal_id(deps.storage)?;

    // Limit the size of proposals.
    //
    // The Juno mainnet has a larger limit for data that can be
    // uploaded as part of an execute message than it does for data
    // that can be queried as part of a query. This means that without
    // this check it is possible to create a proposal that can not be
    // queried.
    //
    // The size selected was determined by uploading versions of this
    // contract to the Juno mainnet until queries worked within a
    // reasonable margin of error.
    //
    // `to_vec` is the method used by cosmwasm to convert a struct
    // into it's byte representation in storage.
    let proposal_size = cosmwasm_std::to_vec(&proposal)?.len() as u64;
    if proposal_size > MAX_PROPOSAL_SIZE {
        return Err(ContractError::ProposalTooLarge {
            size: proposal_size,
            max: MAX_PROPOSAL_SIZE,
        });
    }

    PROPOSALS.save(deps.storage, id, &proposal)?;

    let deposit_msg = get_deposit_msg(&config.deposit_info, &env.contract.address, &sender)?;
    let hooks = new_proposal_hooks(PROPOSAL_HOOKS, deps.storage, id, sender.clone())?;
    Ok(Response::default()
        .add_messages(deposit_msg)
        .add_submessages(hooks)
        .add_attribute("action", "propose")
        .add_attribute("sender", sender)
        .add_attribute("proposal_id", id.to_string())
        .add_attribute("status", proposal.status.to_string()))
}

pub fn execute_vote(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    proposal_id: u64,
    vote: MultipleChoiceVote,
) -> Result<Response<Empty>, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut prop = PROPOSALS
        .may_load(deps.storage, proposal_id)?
        .ok_or(ContractError::NoSuchProposal { id: proposal_id })?;

    // Check that this is a valid vote.
    if vote.option_id as usize >= prop.choices.len() {
        return Err(ContractError::InvalidVote {});
    }

    if prop.current_status(&env.block)? != Status::Open {
        return Err(ContractError::NotOpen { id: proposal_id });
    }

    let vote_power = get_voting_power(
        deps.as_ref(),
        info.sender.clone(),
        config.dao,
        Some(prop.start_height),
    )?;
    if vote_power.is_zero() {
        return Err(ContractError::NotRegistered {});
    }

    BALLOTS.update(
        deps.storage,
        (proposal_id, info.sender.clone()),
        |bal| match bal {
            Some(current_ballot) => {
                if prop.allow_revoting {
                    if current_ballot.vote == vote {
                        // Don't allow casting the same vote more than
                        // once. This seems liable to be confusing
                        // behavior.
                        Err(ContractError::AlreadyCast {})
                    } else {
                        // Remove the old vote if this is a re-vote.
                        prop.votes
                            .remove_vote(current_ballot.vote, current_ballot.power)?;
                        Ok(Ballot {
                            power: vote_power,
                            vote,
                        })
                    }
                } else {
                    Err(ContractError::AlreadyVoted {})
                }
            }
            None => Ok(Ballot {
                vote,
                power: vote_power,
            }),
        },
    )?;

    let old_status = prop.status;

    prop.votes.add_vote(vote, vote_power)?;
    prop.update_status(&env.block)?;
    PROPOSALS.save(deps.storage, proposal_id, &prop)?;
    let new_status = prop.status;
    let change_hooks = proposal_status_changed_hooks(
        PROPOSAL_HOOKS,
        deps.storage,
        proposal_id,
        old_status.to_string(),
        new_status.to_string(),
    )?;
    let vote_hooks = new_vote_hooks(
        VOTE_HOOKS,
        deps.storage,
        proposal_id,
        info.sender.to_string(),
        vote.to_string(),
    )?;
    Ok(Response::default()
        .add_submessages(change_hooks)
        .add_submessages(vote_hooks)
        .add_attribute("action", "vote")
        .add_attribute("sender", info.sender)
        .add_attribute("proposal_id", proposal_id.to_string())
        .add_attribute("position", vote.to_string())
        .add_attribute("status", prop.status.to_string()))
}

pub fn execute_execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    proposal_id: u64,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if config.only_members_execute {
        let power = get_voting_power(
            deps.as_ref(),
            info.sender.clone(),
            config.dao.clone(),
            Some(env.block.height),
        )?;
        if power.is_zero() {
            return Err(ContractError::Unauthorized {});
        }
    }

    let mut prop = PROPOSALS
        .may_load(deps.storage, proposal_id)?
        .ok_or(ContractError::NoSuchProposal { id: proposal_id })?;

    // Check here that the proposal is passed. Allow it to be
    // executed even if it is expired so long as it passed during its
    // voting period.
    prop.update_status(&env.block)?;
    let old_status = prop.status;
    if prop.status != Status::Passed {
        return Err(ContractError::NotPassed {});
    }

    prop.status = Status::Executed;
    // Update proposal's last updated timestamp.
    prop.last_updated = env.block.time;

    PROPOSALS.save(deps.storage, proposal_id, &prop)?;

    // Get refund message if deposits are enabled
    let refund_message = match &prop.deposit_info {
        Some(deposit_info) => match deposit_info.refund_policy {
            // If deposits are not refundable, deposit goes to DAO
            DepositRefundPolicy::Never => get_return_deposit_msg(deposit_info, &config.dao)?,
            // Otherwise, refund goes to proposer
            _ => get_return_deposit_msg(deposit_info, &prop.proposer)?,
        },
        None => vec![],
    };

    let vote_result = prop.calculate_vote_result()?;
    match vote_result {
        VoteResult::Tie => Err(ContractError::Tie {}), // We don't anticipate this case as the proposal would not be in passed state, checked above.
        VoteResult::SingleWinner(winning_choice) => {
            let response = match winning_choice.msgs {
                Some(msgs) => {
                    if !msgs.is_empty() {
                        let execute_message = WasmMsg::Execute {
                            contract_addr: config.dao.to_string(),
                            msg: to_binary(&cwd_core::msg::ExecuteMsg::ExecuteProposalHook {
                                msgs,
                            })?,
                            funds: vec![],
                        };
                        match config.close_proposal_on_execution_failure {
                            true => {
                                let masked_proposal_id =
                                    mask_proposal_execution_proposal_id(proposal_id);
                                Response::default().add_submessage(SubMsg::reply_on_error(
                                    execute_message,
                                    masked_proposal_id,
                                ))
                            }
                            false => Response::default().add_message(execute_message),
                        }
                    } else {
                        Response::default()
                    }
                }
                None => Response::default(),
            };

            let hooks = proposal_status_changed_hooks(
                PROPOSAL_HOOKS,
                deps.storage,
                proposal_id,
                old_status.to_string(),
                prop.status.to_string(),
            )?;

            Ok(response
                .add_messages(refund_message)
                .add_submessages(hooks)
                .add_attribute("action", "execute")
                .add_attribute("sender", info.sender)
                .add_attribute("proposal_id", proposal_id.to_string())
                .add_attribute("dao", config.dao))
        }
    }
}

pub fn execute_close(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    proposal_id: u64,
) -> Result<Response<Empty>, ContractError> {
    let mut prop = PROPOSALS.load(deps.storage, proposal_id)?;
    let config = CONFIG.load(deps.storage)?;

    prop.update_status(&env.block)?;
    if prop.status != Status::Rejected {
        return Err(ContractError::WrongCloseStatus {});
    }

    let old_status = prop.status;

    let refund_message = match &prop.deposit_info {
        Some(deposit_info) => match deposit_info.refund_policy {
            // If we are always refunding deposits, funds go to proposer
            DepositRefundPolicy::Always => get_return_deposit_msg(deposit_info, &prop.proposer)?,
            // If no refunds, or failed proposals not refund, funds go to DAO
            _ => get_return_deposit_msg(deposit_info, &config.dao)?,
        },
        None => vec![],
    };

    prop.status = Status::Closed;
    // Update proposal's last updated timestamp.
    prop.last_updated = env.block.time;
    PROPOSALS.save(deps.storage, proposal_id, &prop)?;

    let changed_hooks = proposal_status_changed_hooks(
        PROPOSAL_HOOKS,
        deps.storage,
        proposal_id,
        old_status.to_string(),
        prop.status.to_string(),
    )?;

    Ok(Response::default()
        .add_submessages(changed_hooks)
        .add_attribute("action", "close")
        .add_attribute("sender", info.sender)
        .add_messages(refund_message)
        .add_attribute("proposal_id", proposal_id.to_string()))
}

#[allow(clippy::too_many_arguments)]
pub fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    voting_strategy: VotingStrategy,
    min_voting_period: Option<Duration>,
    max_voting_period: Duration,
    only_members_execute: bool,
    allow_revoting: bool,
    dao: String,
    deposit_info: Option<UncheckedDepositInfo>,
    close_proposal_on_execution_failure: bool,
    open_proposal_submission: bool,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only the DAO may call this method.
    if info.sender != config.dao {
        return Err(ContractError::Unauthorized {});
    }

    voting_strategy.validate()?;

    let dao = deps.api.addr_validate(&dao)?;
    let deposit_info = deposit_info
        .map(|info| info.into_checked(deps.as_ref(), dao.clone()))
        .transpose()?;

    let (min_voting_period, max_voting_period) =
        validate_voting_period(min_voting_period, max_voting_period)?;

    CONFIG.save(
        deps.storage,
        &Config {
            voting_strategy,
            min_voting_period,
            max_voting_period,
            only_members_execute,
            allow_revoting,
            dao,
            deposit_info,
            close_proposal_on_execution_failure,
            open_proposal_submission,
        },
    )?;

    Ok(Response::default()
        .add_attribute("action", "update_config")
        .add_attribute("sender", info.sender))
}

pub fn execute_add_proposal_hook(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if config.dao != info.sender {
        // Only DAO can add hooks
        return Err(ContractError::Unauthorized {});
    }

    let validated_address = deps.api.addr_validate(&address)?;

    add_hook(PROPOSAL_HOOKS, deps.storage, validated_address)?;

    Ok(Response::default()
        .add_attribute("action", "add_proposal_hook")
        .add_attribute("address", address))
}

pub fn execute_remove_proposal_hook(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if config.dao != info.sender {
        // Only DAO can remove hooks
        return Err(ContractError::Unauthorized {});
    }

    let validated_address = deps.api.addr_validate(&address)?;

    remove_hook(PROPOSAL_HOOKS, deps.storage, validated_address)?;

    Ok(Response::default()
        .add_attribute("action", "remove_proposal_hook")
        .add_attribute("address", address))
}

pub fn execute_add_vote_hook(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if config.dao != info.sender {
        // Only DAO can add hooks
        return Err(ContractError::Unauthorized {});
    }

    let validated_address = deps.api.addr_validate(&address)?;

    add_hook(VOTE_HOOKS, deps.storage, validated_address)?;

    Ok(Response::default()
        .add_attribute("action", "add_vote_hook")
        .add_attribute("address", address))
}

pub fn execute_remove_vote_hook(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    address: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if config.dao != info.sender {
        // Only DAO can remove hooks
        return Err(ContractError::Unauthorized {});
    }

    let validated_address = deps.api.addr_validate(&address)?;

    remove_hook(VOTE_HOOKS, deps.storage, validated_address)?;

    Ok(Response::default()
        .add_attribute("action", "remove_vote_hook")
        .add_attribute("address", address))
}

pub fn add_hook(
    hooks: Hooks,
    storage: &mut dyn Storage,
    validated_address: Addr,
) -> Result<(), ContractError> {
    hooks
        .add_hook(storage, validated_address)
        .map_err(ContractError::HookError)?;
    Ok(())
}

pub fn remove_hook(
    hooks: Hooks,
    storage: &mut dyn Storage,
    validate_address: Addr,
) -> Result<(), ContractError> {
    hooks
        .remove_hook(storage, validate_address)
        .map_err(ContractError::HookError)?;
    Ok(())
}

pub fn advance_proposal_id(store: &mut dyn Storage) -> StdResult<u64> {
    let id: u64 = PROPOSAL_COUNT.load(store)? + 1;
    PROPOSAL_COUNT.save(store, &id)?;
    Ok(id)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => query_config(deps),
        QueryMsg::Proposal { proposal_id } => query_proposal(deps, env, proposal_id),
        QueryMsg::ListProposals { start_after, limit } => {
            query_list_proposals(deps, env, start_after, limit)
        }
        QueryMsg::ProposalCount {} => query_proposal_count(deps),
        QueryMsg::GetVote { proposal_id, voter } => query_vote(deps, proposal_id, voter),
        QueryMsg::ListVotes {
            proposal_id,
            start_after,
            limit,
        } => query_list_votes(deps, proposal_id, start_after, limit),
        QueryMsg::Info {} => query_info(deps),
        QueryMsg::ReverseProposals {
            start_before,
            limit,
        } => query_reverse_proposals(deps, env, start_before, limit),
        QueryMsg::ProposalHooks {} => to_binary(&PROPOSAL_HOOKS.query_hooks(deps)?),
        QueryMsg::VoteHooks {} => to_binary(&VOTE_HOOKS.query_hooks(deps)?),
    }
}

pub fn query_config(deps: Deps) -> StdResult<Binary> {
    let config = CONFIG.load(deps.storage)?;
    to_binary(&config)
}

pub fn query_proposal(deps: Deps, env: Env, id: u64) -> StdResult<Binary> {
    let proposal = PROPOSALS.load(deps.storage, id)?;
    to_binary(&proposal.into_response(&env.block, id)?)
}

pub fn query_list_proposals(
    deps: Deps,
    env: Env,
    start_after: Option<u64>,
    limit: Option<u64>,
) -> StdResult<Binary> {
    let min = start_after.map(Bound::exclusive);
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    let props: Vec<ProposalResponse> = PROPOSALS
        .range(deps.storage, min, None, cosmwasm_std::Order::Ascending)
        .take(limit as usize)
        .collect::<Result<Vec<(u64, MultipleChoiceProposal)>, _>>()?
        .into_iter()
        .map(|(id, proposal)| proposal.into_response(&env.block, id))
        .collect::<StdResult<Vec<ProposalResponse>>>()?;

    to_binary(&ProposalListResponse { proposals: props })
}

pub fn query_reverse_proposals(
    deps: Deps,
    env: Env,
    start_before: Option<u64>,
    limit: Option<u64>,
) -> StdResult<Binary> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    let max = start_before.map(Bound::exclusive);
    let props: Vec<ProposalResponse> = PROPOSALS
        .range(deps.storage, None, max, cosmwasm_std::Order::Descending)
        .take(limit as usize)
        .collect::<Result<Vec<(u64, MultipleChoiceProposal)>, _>>()?
        .into_iter()
        .map(|(id, proposal)| proposal.into_response(&env.block, id))
        .collect::<StdResult<Vec<ProposalResponse>>>()?;

    to_binary(&ProposalListResponse { proposals: props })
}

pub fn query_proposal_count(deps: Deps) -> StdResult<Binary> {
    let proposal_count = PROPOSAL_COUNT.load(deps.storage)?;
    to_binary(&proposal_count)
}

pub fn query_vote(deps: Deps, proposal_id: u64, voter: String) -> StdResult<Binary> {
    let voter = deps.api.addr_validate(&voter)?;
    let ballot = BALLOTS.may_load(deps.storage, (proposal_id, voter.clone()))?;
    let vote = ballot.map(|ballot| VoteInfo {
        voter,
        vote: ballot.vote,
        power: ballot.power,
    });
    to_binary(&VoteResponse { vote })
}

pub fn query_list_votes(
    deps: Deps,
    proposal_id: u64,
    start_after: Option<String>,
    limit: Option<u64>,
) -> StdResult<Binary> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    let start_after = start_after
        .map(|addr| deps.api.addr_validate(&addr))
        .transpose()?;
    let min = start_after.map(Bound::<Addr>::exclusive);

    let votes = BALLOTS
        .prefix(proposal_id)
        .range(deps.storage, min, None, cosmwasm_std::Order::Ascending)
        .take(limit as usize)
        .map(|item| {
            let (voter, ballot) = item?;
            Ok(VoteInfo {
                voter,
                vote: ballot.vote,
                power: ballot.power,
            })
        })
        .collect::<StdResult<Vec<_>>>()?;

    to_binary(&VoteListResponse { votes })
}

pub fn query_info(deps: Deps) -> StdResult<Binary> {
    let info = cw2::get_contract_version(deps.storage)?;
    to_binary(&cwd_interface::voting::InfoResponse { info })
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> Result<Response, ContractError> {
    let repl = TaggedReplyId::new(msg.id)?;
    match repl {
        TaggedReplyId::FailedProposalExecution(proposal_id) => {
            PROPOSALS.update(deps.storage, proposal_id, |prop| match prop {
                Some(mut prop) => {
                    prop.status = Status::ExecutionFailed;
                    // Update proposal's last updated timestamp.
                    prop.last_updated = env.block.time;
                    Ok(prop)
                }
                None => Err(ContractError::NoSuchProposal { id: proposal_id }),
            })?;
            Ok(Response::new().add_attribute("proposal execution failed", proposal_id.to_string()))
        }
        TaggedReplyId::FailedProposalHook(idx) => {
            let addr = PROPOSAL_HOOKS.remove_hook_by_index(deps.storage, idx)?;
            Ok(Response::new().add_attribute("removed proposal hook", format!("{addr}:{idx}")))
        }
        TaggedReplyId::FailedVoteHook(idx) => {
            let addr = VOTE_HOOKS.remove_hook_by_index(deps.storage, idx)?;
            Ok(Response::new().add_attribute("removed vote hook", format!("{addr}:{idx}")))
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    // Set contract to version to latest
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::default())
}
