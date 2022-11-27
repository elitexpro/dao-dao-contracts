use cosmwasm_schema::cw_serde;
use cosmwasm_std::{CosmosMsg, Empty};

/// Default limit for proposal pagination.
pub const DEFAULT_LIMIT: u64 = 30;
pub const MAX_PROPOSAL_SIZE: u64 = 30_000;

/// The contents of a message to create a proposal in the single
/// choice proposal module.
///
/// We break this type out of `ExecuteMsg` because we want pre-propose
/// modules that interact with this contract to be able to get type
/// checking on their propose messages.
///
/// We move this type to this package so that pre-propose modules can
/// import it without importing dao-proposal-single with the library
/// feature which (as it is not additive) cause the execute exports to
/// not be included in wasm builds.
#[cw_serde]
pub struct SingleChoiceProposeMsg {
    /// The title of the proposal.
    pub title: String,
    /// A description of the proposal.
    pub description: String,
    /// The messages that should be executed in response to this
    /// proposal passing.
    pub msgs: Vec<CosmosMsg<Empty>>,
    /// The address creating the proposal. If no pre-propose
    /// module is attached to this module this must always be None
    /// as the proposer is the sender of the propose message. If a
    /// pre-propose module is attached, this must be Some and will
    /// set the proposer of the proposal it creates.
    pub proposer: Option<String>,
}
