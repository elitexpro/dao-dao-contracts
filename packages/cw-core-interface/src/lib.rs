use cosmwasm_std::{Addr, Binary, CosmosMsg, Empty, WasmMsg};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod voting;

/// The cw-core interface.
#[derive(Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    /// Causes the core module to execute all of MSGS in order. Only
    /// callabale by a proposal module.1
    ExecuteProposalHook { msgs: Vec<CosmosMsg<Empty>> },
}

/// Information about the CosmWasm level admin of a contract. Used in
/// conjunction with `ModuleInstantiateInfo` to instantiate modules.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Admin {
    /// Set the admin to a specified address.
    Address { addr: String },
    /// Set the admin to the address that instantiates the
    /// contract. This is useful for DAOs that instantiate this
    /// contract as part of their creation process and would like to
    /// set themselces as the admin (as in this case the address of
    /// the instantiating contract is not known at message execution
    /// time).
    Instantiator {},
}

/// Information needed to instantiate a module.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct ModuleInstantiateInfo {
    /// Code ID of the contract to be instantiated.
    pub code_id: u64,
    /// Instantiate message to be used to create the contract.
    pub msg: Binary,
    /// CosmWasm level admin of the instantiated contract. See:
    /// <https://docs.cosmwasm.com/docs/1.0/smart-contracts/migration>
    pub admin: Option<Admin>,
    /// Label for the instantiated contract.
    pub label: String,
}

impl ModuleInstantiateInfo {
    pub fn into_wasm_msg(self, contract_address: Addr) -> WasmMsg {
        WasmMsg::Instantiate {
            admin: self.admin.map(|admin| match admin {
                Admin::Address { addr } => addr,
                Admin::Instantiator {} => contract_address.into_string(),
            }),
            code_id: self.code_id,
            msg: self.msg,
            funds: vec![],
            label: self.label,
        }
    }
}

#[cfg(test)]
mod tests {
    use cosmwasm_std::{to_binary, Addr, WasmMsg};

    use crate::{Admin, ModuleInstantiateInfo};

    #[test]
    fn test_module_instantiate_admin_none() {
        let no_admin = ModuleInstantiateInfo {
            code_id: 42,
            msg: to_binary("foo").unwrap(),
            admin: None,
            label: "bar".to_string(),
        };
        assert_eq!(
            no_admin.into_wasm_msg(Addr::unchecked("ekez")),
            WasmMsg::Instantiate {
                admin: None,
                code_id: 42,
                msg: to_binary("foo").unwrap(),
                funds: vec![],
                label: "bar".to_string()
            }
        )
    }

    #[test]
    fn test_module_instantiate_admin_addr() {
        let no_admin = ModuleInstantiateInfo {
            code_id: 42,
            msg: to_binary("foo").unwrap(),
            admin: Some(Admin::Address {
                addr: "core".to_string(),
            }),
            label: "bar".to_string(),
        };
        assert_eq!(
            no_admin.into_wasm_msg(Addr::unchecked("ekez")),
            WasmMsg::Instantiate {
                admin: Some("core".to_string()),
                code_id: 42,
                msg: to_binary("foo").unwrap(),
                funds: vec![],
                label: "bar".to_string()
            }
        )
    }

    #[test]
    fn test_module_instantiate_instantiator_addr() {
        let no_admin = ModuleInstantiateInfo {
            code_id: 42,
            msg: to_binary("foo").unwrap(),
            admin: Some(Admin::Instantiator {}),
            label: "bar".to_string(),
        };
        assert_eq!(
            no_admin.into_wasm_msg(Addr::unchecked("ekez")),
            WasmMsg::Instantiate {
                admin: Some("ekez".to_string()),
                code_id: 42,
                msg: to_binary("foo").unwrap(),
                funds: vec![],
                label: "bar".to_string()
            }
        )
    }
}
