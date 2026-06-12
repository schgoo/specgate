use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OperationKind {
    Stateless,
    StateMachine,
    Sequence,
    ErrorMap,
    Structural,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Annotation {
    SpecOperation {
        operation: String,
        kind: OperationKind,
        symbol: String,
    },
    SpecSetup {
        operation: String,
        name: String,
        symbol: String,
        #[serde(default)]
        params: Vec<String>,
        #[serde(default)]
        returns: Option<String>,
    },
    SpecCheckpoint {
        operation: String,
        symbol: String,
    },
    SpecCapture {
        operation: String,
        symbol: String,
        #[serde(default)]
        capture_all: bool,
    },
    SpecMock {
        operation: String,
        #[serde(alias = "mock_name")]
        name: String,
        symbol: String,
    },
}
