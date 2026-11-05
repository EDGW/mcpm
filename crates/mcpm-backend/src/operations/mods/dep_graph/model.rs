//! Owned operation inputs, graph results and terminal-independent tree models.

use super::super::loading::ScopeInput;
use super::view::filter::FilterInput;
use mcpm_exec::error::PortableError;
use mcpm_lib::local::mcmod::dependency::graph::identifier::DependencyIdentifier;
use mcpm_lib::local::mcmod::dependency::{Dependency, DependencyMatch};
use mcpm_lib::local::modloader::LoaderId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identifier {
    pub loader: String,
    pub id: String,
}
impl Identifier {
    pub(crate) fn into_runtime(self) -> DependencyIdentifier {
        DependencyIdentifier::from_parts(LoaderId::new(self.loader), self.id)
    }
}
impl From<&DependencyIdentifier> for Identifier {
    fn from(value: &DependencyIdentifier) -> Self {
        Self {
            loader: value.loader().as_str().to_owned(),
            id: value.id().to_owned(),
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ExternalInput {
    pub identifier: Identifier,
    pub version: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct BuildInput {
    pub scope: ScopeInput,
    pub external: Vec<ExternalInput>,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum BuildEvent {
    Progress { completed: usize, total: usize },
    Diagnostic(PortableError),
    Finished(BuildOutput),
}
#[derive(Debug, Serialize, Deserialize)]
pub struct BuildOutput {
    /// Original storage codec bytes, suitable for caller-side export without decoding.
    pub snapshot: Vec<u8>,
    pub problems: ViewOutput,
    pub identification_failures: usize,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum GraphInput {
    File(PathBuf),
    Bytes(Vec<u8>),
}
#[derive(Debug, Serialize, Deserialize)]
pub struct InspectInput {
    pub graph: GraphInput,
    pub action: InspectAction,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum InspectAction {
    Test,
    View(ViewInput),
}
#[derive(Debug, Serialize, Deserialize)]
pub enum InspectOutput {
    Test(Statistics),
    View(ViewOutput),
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Statistics {
    pub nodes: usize,
    pub empty_nodes: usize,
    pub external: usize,
    pub primary: usize,
    pub nested: usize,
    pub edges: usize,
    pub identification_failures: usize,
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ViewInput {
    pub targets: Vec<Identifier>,
    pub reverse: bool,
    pub ignore_isolated_root: bool,
    pub filter: FilterInput,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ViewOutput {
    pub trees: Vec<Vec<TreeEntry>>,
    pub identification_failures: usize,
    pub reverse: bool,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct TreeEntry {
    /// Earlier entry in this tree; layout is exclusively a frontend concern.
    pub parent: Option<usize>,
    pub last: bool,
    pub label: TreeLabel,
}
#[derive(Debug, Serialize, Deserialize)]
pub enum TreeLabel {
    Node {
        identifier: Identifier,
        root: bool,
        candidates: CandidateSummary,
        relation: Option<Dependency>,
        state: Option<MatchState>,
        folded: Option<Fold>,
    },
    Candidate {
        candidate: CandidateView,
        relation: Option<Dependency>,
        state: Option<MatchState>,
    },
}
#[derive(Debug, Serialize, Deserialize)]
pub enum CandidateSummary {
    Missing,
    One(CandidateView),
    Multiple(usize),
}
#[derive(Debug, Serialize, Deserialize)]
pub enum CandidateView {
    External { version: Option<String> },
    Primary { version: String },
    Nested { version: String },
}
#[derive(Debug, Serialize, Deserialize)]
pub enum Fold {
    Cycle,
    Shared,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchState {
    Matched,
    Unmatched,
    Unknown,
}
impl From<DependencyMatch> for MatchState {
    fn from(value: DependencyMatch) -> Self {
        match value {
            DependencyMatch::Matched => Self::Matched,
            DependencyMatch::Unmatched => Self::Unmatched,
            DependencyMatch::Unknown => Self::Unknown,
        }
    }
}
