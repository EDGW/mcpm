//! Complete Host cache-write policy; no cross-machine publication tokens or staged RPC inputs.

use super::super::{
    loading::{ScopeInput, selection::SelectionInput},
    source::Configuration,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Input {
    pub scope: ScopeInput,
    pub selection: SelectionInput,
    pub options: StoreOptions,
    pub identification: Option<IdentificationOptions>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct StoreOptions {
    pub mode: crate::operations::cache::TransferPreference,
    pub force: bool,
    pub parallelism: usize,
    pub metadata: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentificationOptions {
    pub sources: Vec<Configuration>,
    pub force: bool,
}
