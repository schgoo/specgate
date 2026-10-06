//! Build completed native evidence and encode deterministic CTSC OTLP JSON.
//!
//! A [`Capture`] owns one scenario [`Boundary`] and its completed [`Operation`] spans;
//! the run boundary encloses the scenario, and each operation names its parent span.
//! [`TraceId`] and [`SpanId`] preserve accepted spellings without lexical normalization.
//! Timestamps are nanoseconds: every boundary and operation must end at or after it
//! starts, while event times and parentage are checked during deterministic encoding.
//! [`Metadata`] links the emitted resource to the exact registry artifact. Reference
//! and candidate encoders intentionally allocate disjoint deterministic identities.
//!
//! ```
//! use specgate_ctsc::capture::{
//!     Boundary, BoundarySpan, Capture, CaptureDeps, Completion, Metadata, Operation,
//!     OperationDeps, OperationIdentity, OperationSpan, Registry, SpanId, Status,
//!     Target, TimeInterval, TraceId, Value, encode_reference,
//! };
//! let run_id = SpanId::new("0000000000000001");
//! let scenario_id = SpanId::new("0000000000000002");
//! let operation = Operation::builder(OperationDeps {
//!     order: 0,
//!     span: OperationSpan { span_id: SpanId::new("0000000000000003"), parent_id: scenario_id.clone() },
//!     identity: OperationIdentity { component_id: "example".into(), name: "read".into() },
//!     interval: TimeInterval { start_ns: 2, end_ns: 3 },
//!     status: Status::Ok,
//! }).completion(Completion::Result { order: 0, time_ns: 3, value: Value::String("ok".into()) }).build()?;
//! let capture = Capture::builder(CaptureDeps {
//!     trace_id: TraceId::new("00000000000000000000000000000001"),
//!     scenario_name: "reads".into(),
//!     run: Boundary { span: BoundarySpan { span_id: run_id.clone(), parent_id: None }, interval: TimeInterval { start_ns: 1, end_ns: 4 }, status: Status::Ok },
//!     scenario: Boundary { span: BoundarySpan { span_id: scenario_id, parent_id: Some(run_id) }, interval: TimeInterval { start_ns: 2, end_ns: 3 }, status: Status::Ok },
//! }).operations(vec![operation]).build()?;
//! let metadata = Metadata::new(
//!     "1", Target::new("example", "rust"), Registry::new("registry", "1", "sha256:digest"),
//! );
//! let encoded = encode_reference([capture], &metadata)?;
//! assert_eq!(encoded.span_count, 3);
//! # Ok::<(), specgate_ctsc::capture::Error>(())
//! ```

/// Structured capture encoding errors and stable failure classifications.
mod error;
pub use error::{Error, ErrorKind};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// CTSC Trace Core version and schema URL; changing either changes wire compatibility.
mod ctsc {
    pub(super) const VERSION: &str = "0.2.0";
    pub(super) const SCHEMA: &str = "https://specgate.dev/ctsc/schema/0.2.0";
}
// Deterministic reference/replay identity ranges keep independently encoded evidence disjoint.
mod reference {
    pub(super) const TRACE_ID: &str = "00000000000000000000000000000001";
    pub(super) const START_ID: u64 = 1;
}
mod candidate {
    pub(super) const TRACE_ID: &str = "00000000000000000000000000000002";
    pub(super) const START_ID: u64 = 0x8000_0000_0000_0001;
}
// Logical timestamps begin after the run sentinel timestamp and remain deterministic.
mod time {
    pub(super) const SCENARIO: i64 = 2;
    pub(super) const RUN: i64 = 1;
}
// Trace Core sentinel names, run identities, OTLP enum values, and attribute keys are fixed protocol spellings.
mod run {
    pub(super) const CAPTURE: &str = "specgate.capture";
    pub(super) const REPLAY: &str = "specgate.replay";
}
const KIND_INTERNAL: i32 = 1;
const STATUS_OK: i32 = 1;
const STATUS_ERROR: i32 = 2;
const RUN_NAME: &str = "conformance.run";
const SCENARIO_NAME: &str = "conformance.scenario";
const OPERATION_NAME: &str = "conformance.operation";
const ERROR_EVENT: &str = "conformance.error";
const OBS_EVENT: &str = "conformance.observation";
const RESULT_EVENT: &str = "conformance.result";
const EMPTY_EVENT: &str = "conformance.empty";
const FAULT_EVENT: &str = "conformance.fault";
// Resource/scope identity literals are fixed by CTSC Trace Core and changing them changes wire compatibility.
const TOOL_NAME: &str = "specgate";
const SCOPE_NAME: &str = "specgate.ctsc";
// OTLP span IDs are exactly 8 bytes, rendered as 16 hexadecimal digits on the wire.
const ID_HEX_WIDTH: usize = 16;

mod attribute {
    pub(super) const RUN_ID: &str = "conformance.run.id";
    pub(super) const REGISTRY_ID: &str = "conformance.registry.id";
    pub(super) const REGISTRY_VERSION: &str = "conformance.registry.version";
    pub(super) const REGISTRY_DIGEST: &str = "conformance.registry.digest";
    pub(super) const SCENARIO_NAME: &str = "conformance.scenario.name";
    pub(super) const SCENARIO_INDEX: &str = "conformance.scenario.index";
    pub(super) const OPERATION_NAME: &str = "conformance.operation.name";
    pub(super) const OPERATION_INPUTS: &str = "conformance.operation.inputs";
    pub(super) const ERROR_NAME: &str = "conformance.error.name";
    pub(super) const ERROR_VALUE: &str = "conformance.error.value";
    pub(super) const OBS_NAME: &str = "conformance.observation.name";
    pub(super) const OBS_VALUE: &str = "conformance.observation.value";
    pub(super) const RESULT_VALUE: &str = "conformance.result.value";
    pub(super) const FAULT_TYPE: &str = "conformance.fault.type";
    pub(super) const FAULT_MESSAGE: &str = "conformance.fault.message";
    pub(super) const FAULT_OBSERVER: &str = "conformance.fault.observer";
    pub(super) const COMPONENT_ID: &str = "conformance.component.id";
    pub(super) const VERSION: &str = "conformance.version";
    pub(super) const TOOL_NAME: &str = "conformance.tool.name";
    pub(super) const TOOL_VERSION: &str = "conformance.tool.version";
    pub(super) const TARGET_NAME: &str = "conformance.target.name";
    pub(super) const TARGET_LANGUAGE: &str = "conformance.target.language";
}
/// Raw trace-identity spelling preserved at the capture boundary.
///
/// This is a total semantic wrapper and deliberately performs no lexical
/// validation; artifact validators enforce wire-format constraints later.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TraceId(String);
impl TraceId {
    /// Construct without adding lexical restrictions to accepted capture input.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    /// Borrow the preserved spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<T: Into<String>> From<T> for TraceId {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}
impl std::fmt::Display for TraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Raw span-identity spelling preserved at the capture boundary.
///
/// This is a total semantic wrapper and deliberately performs no lexical
/// validation; artifact validators enforce wire-format constraints later.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SpanId(String);
impl SpanId {
    /// Construct without adding lexical restrictions to accepted capture input.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    /// Borrow the preserved spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<T: Into<String>> From<T> for SpanId {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}
impl std::fmt::Display for SpanId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Crate-owned semantic value accepted at the capture boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[expect(
    clippy::exhaustive_enums,
    reason = "capture wire model variants are part of the stable construction API"
)]
pub enum Value {
    /// UTF-8 text.
    String(String),
    /// Signed integer.
    Integer(i64),
    /// Unsigned integer.
    Unsigned(u64),
    /// Floating-point value.
    Float(f64),
    /// Boolean value.
    Bool(bool),
    /// Ordered sequence.
    List(Vec<Value>),
    /// String-keyed map.
    Map(BTreeMap<String, Value>),
    /// Deterministically ordered values, in encoded order.
    Set(Vec<Value>),
}

/// Terminal span status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[expect(
    clippy::exhaustive_enums,
    reason = "capture wire model variants are part of the stable construction API"
)]
pub enum Status {
    /// Successful completion.
    Ok,
    /// Error or fault completion.
    Error,
}

/// Span relationship for a completed run or scenario boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(clippy::exhaustive_structs, reason = "capture data models support direct construction by callers")]
pub struct BoundarySpan {
    /// Span identifier.
    pub span_id: SpanId,
    /// Optional parent identifier.
    #[serde(rename = "parent_span_id")]
    pub parent_id: Option<SpanId>,
}

/// Closed nanosecond interval shared by capture boundaries and operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[expect(clippy::exhaustive_structs, reason = "capture data models support direct construction by callers")]
pub struct TimeInterval {
    /// Start timestamp.
    #[serde(rename = "start_time_unix_nano")]
    pub start_ns: i64,
    /// End timestamp.
    #[serde(rename = "end_time_unix_nano")]
    pub end_ns: i64,
}

/// Completed run or scenario span boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(clippy::exhaustive_structs, reason = "capture data models support direct construction by callers")]
pub struct Boundary {
    /// Span identity and parent relationship.
    #[serde(flatten)]
    pub span: BoundarySpan,
    /// Completed timestamp interval.
    #[serde(flatten)]
    pub interval: TimeInterval,
    /// Terminal status.
    pub status: Status,
}

/// One operation observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[expect(clippy::exhaustive_structs, reason = "capture data models support direct construction by callers")]
pub struct Observation {
    /// Stable event order.
    pub order: u64,
    /// Event timestamp.
    #[serde(rename = "time_unix_nano")]
    pub time_ns: i64,
    /// Semantic name.
    pub name: String,
    /// Semantic value.
    pub value: Value,
}

/// Operation completion channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[expect(
    clippy::exhaustive_enums,
    reason = "capture wire model variants are part of the stable construction API"
)]
pub enum Completion {
    /// Typed result.
    Result {
        /// Event order.
        order: u64,
        /// Timestamp.
        #[serde(rename = "time_unix_nano")]
        time_ns: i64,
        /// Value.
        value: Value,
    },
    /// Explicit empty completion.
    Empty {
        /// Event order.
        order: u64,
        /// Timestamp.
        #[serde(rename = "time_unix_nano")]
        time_ns: i64,
    },
    /// Declared error.
    Error {
        /// Event order.
        order: u64,
        /// Timestamp.
        #[serde(rename = "time_unix_nano")]
        time_ns: i64,
        /// Error name.
        name: String,
        /// Optional payload.
        value: Option<Value>,
    },
    /// Runtime fault.
    Fault {
        /// Event order.
        order: u64,
        /// Timestamp.
        #[serde(rename = "time_unix_nano")]
        time_ns: i64,
        /// Fault category.
        fault_type: String,
        /// Message.
        message: String,
        /// Observer identity.
        observer: String,
    },
}

/// One completed operation span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[expect(clippy::exhaustive_structs, reason = "capture data models support direct construction by callers")]
pub struct Operation {
    /// Stable operation order.
    pub order: u64,
    /// Span identifier.
    pub span_id: SpanId,
    /// Parent span identifier.
    #[serde(rename = "parent_span_id")]
    pub parent_id: SpanId,
    /// Component identifier.
    pub component_id: String,
    /// Operation name.
    #[expect(clippy::struct_field_names, reason = "the protocol calls this field operation_name")]
    pub operation_name: String,
    /// Start timestamp.
    #[serde(rename = "start_time_unix_nano")]
    pub start_ns: i64,
    /// End timestamp.
    #[serde(rename = "end_time_unix_nano")]
    pub end_ns: i64,
    /// Terminal status.
    pub status: Status,
    /// Inputs by semantic name.
    pub inputs: BTreeMap<String, Value>,
    /// Ordered observations.
    pub observations: Vec<Observation>,
    /// Optional completion.
    pub completion: Option<Completion>,
}

/// Span identity and required parent relationship for an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::exhaustive_structs, reason = "capture data models support direct construction by callers")]
pub struct OperationSpan {
    /// Span identifier.
    pub span_id: SpanId,
    /// Parent span identifier.
    pub parent_id: SpanId,
}

/// Semantic component and operation identity.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::exhaustive_structs, reason = "capture data models support direct construction by callers")]
pub struct OperationIdentity {
    /// Component identifier.
    pub component_id: String,
    /// Operation name.
    pub name: String,
}

/// Required identity and interval data for an operation builder.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::exhaustive_structs, reason = "capture data models support direct construction by callers")]
pub struct OperationDeps {
    /// Stable operation order.
    pub order: u64,
    /// Span identity and parent relationship.
    pub span: OperationSpan,
    /// Semantic operation identity.
    pub identity: OperationIdentity,
    /// Completed timestamp interval.
    pub interval: TimeInterval,
    /// Terminal status.
    pub status: Status,
}

/// Builder for optional operation evidence.
///
/// # Examples
/// ```
/// use specgate_ctsc::capture::{Operation, OperationDeps, OperationIdentity, OperationSpan, SpanId, Status, TimeInterval};
/// let operation = Operation::builder(OperationDeps {
///     order: 0,
///     span: OperationSpan { span_id: SpanId::new("0000000000000001"), parent_id: SpanId::new("0000000000000002") },
///     identity: OperationIdentity { component_id: "example".into(), name: "run".into() },
///     interval: TimeInterval { start_ns: 2, end_ns: 1 },
///     status: Status::Ok,
/// }).build();
/// assert!(operation.is_err());
/// ```
#[derive(Debug)]
pub struct OperationBuilder {
    operation: Operation,
}
impl Operation {
    /// Begin an operation using its required identity and interval data.
    #[must_use]
    pub fn builder(deps: impl Into<OperationDeps>) -> OperationBuilder {
        let deps = deps.into();
        OperationBuilder {
            operation: Self {
                order: deps.order,
                span_id: deps.span.span_id,
                parent_id: deps.span.parent_id,
                component_id: deps.identity.component_id,
                operation_name: deps.identity.name,
                start_ns: deps.interval.start_ns,
                end_ns: deps.interval.end_ns,
                status: deps.status,
                inputs: BTreeMap::new(),
                observations: Vec::new(),
                completion: None,
            },
        }
    }
}
impl OperationBuilder {
    /// Set semantic inputs.
    #[must_use]
    pub fn inputs(mut self, inputs: BTreeMap<String, Value>) -> Self {
        self.operation.inputs = inputs;
        self
    }
    /// Set ordered observations.
    #[must_use]
    pub fn observations(mut self, observations: Vec<Observation>) -> Self {
        self.operation.observations = observations;
        self
    }
    /// Set terminal completion evidence.
    #[must_use]
    pub fn completion(mut self, completion: Completion) -> Self {
        self.operation.completion = Some(completion);
        self
    }
    /// Finish the fully formed operation.
    ///
    /// # Errors
    /// Returns an encoding error when the end timestamp precedes the start timestamp.
    pub fn build(self) -> Result<Operation, Error> {
        if self.operation.end_ns < self.operation.start_ns {
            return Err(format!(
                "operation end timestamp {} precedes start timestamp {}",
                self.operation.end_ns, self.operation.start_ns
            )
            .into());
        }
        Ok(self.operation)
    }
}

/// Completed evidence for one scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[expect(clippy::exhaustive_structs, reason = "capture data models support direct construction by callers")]
pub struct Capture {
    /// Original trace identifier.
    pub trace_id: TraceId,
    /// Scenario name.
    pub scenario_name: String,
    /// Run boundary.
    pub run: Boundary,
    /// Scenario boundary.
    pub scenario: Boundary,
    /// Operation spans.
    pub operations: Vec<Operation>,
}

/// Required identity and span boundaries for constructing a capture.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::exhaustive_structs, reason = "capture data models support direct construction by callers")]
pub struct CaptureDeps {
    /// Original trace identifier.
    pub trace_id: TraceId,
    /// Scenario name.
    pub scenario_name: String,
    /// Run boundary.
    pub run: Boundary,
    /// Scenario boundary.
    pub scenario: Boundary,
}
/// Builder for variable operation evidence in a capture.
#[derive(Debug)]
pub struct CaptureBuilder {
    capture: Capture,
}
impl Capture {
    /// Begin a capture with required identity and boundaries.
    ///
    /// ```
    /// use specgate_ctsc::capture::{Boundary, BoundarySpan, Capture, CaptureDeps, Operation, OperationDeps, OperationIdentity, OperationSpan, SpanId, Status, TimeInterval, TraceId};
    /// let boundary = Boundary { span: BoundarySpan { span_id: SpanId::new("0000000000000001"), parent_id: None }, interval: TimeInterval { start_ns: 1, end_ns: 2 }, status: Status::Ok };
    /// let capture = Capture::builder(CaptureDeps {
    ///     trace_id: TraceId::new("00000000000000000000000000000001"),
    ///     scenario_name: "example".into(),
    ///     run: boundary.clone(),
    ///     scenario: boundary,
    /// });
    /// let operation = Operation::builder(OperationDeps {
    ///     order: 0,
    ///     span: OperationSpan { span_id: SpanId::new("0000000000000002"), parent_id: SpanId::new("0000000000000001") },
    ///     identity: OperationIdentity { component_id: "example".into(), name: "run".into() },
    ///     interval: TimeInterval { start_ns: 1, end_ns: 2 },
    ///     status: Status::Ok,
    /// }).build()?;
    /// let error = capture.operations(vec![operation.clone(), operation]).build()
    ///     .expect_err("duplicate span identities must be rejected");
    /// assert!(error.to_string().contains("unique"));
    /// # Ok::<(), specgate_ctsc::capture::Error>(())
    /// ```
    #[must_use]
    pub fn builder(deps: impl Into<CaptureDeps>) -> CaptureBuilder {
        let deps = deps.into();
        CaptureBuilder {
            capture: Self {
                trace_id: deps.trace_id,
                scenario_name: deps.scenario_name,
                run: deps.run,
                scenario: deps.scenario,
                operations: Vec::new(),
            },
        }
    }
}
impl CaptureBuilder {
    /// Set completed operation evidence.
    #[must_use]
    pub fn operations(mut self, operations: Vec<Operation>) -> Self {
        self.capture.operations = operations;
        self
    }
    /// Finish the capture.
    ///
    /// # Errors
    /// Returns an encoding error when operation span identifiers are duplicated.
    pub fn build(self) -> Result<Capture, Error> {
        let ids = self
            .capture
            .operations
            .iter()
            .map(|operation| operation.span_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if ids.len() != self.capture.operations.len() {
            return Err("capture operation span IDs must be unique".to_string().into());
        }
        Ok(self.capture)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Encoded native capture output.
#[expect(
    clippy::exhaustive_structs,
    reason = "encoding results are an exhaustively serialized CTSC boundary DTO"
)]
pub struct Encoding {
    /// Total number of emitted spans.
    pub span_count: i32,
    /// Compact OTLP JSON document.
    pub otlp_json: String,
}

/// Registry identity embedded into a capture resource.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::exhaustive_structs, reason = "capture data models support direct construction by callers")]
pub struct Registry {
    /// Registry identifier.
    pub id: String,
    /// Registry version.
    pub version: String,
    /// Digest of the exact registry bytes.
    pub digest: String,
}

impl Registry {
    /// Construct identity without imposing lexical validation.
    pub fn new(id: impl Into<String>, version: impl Into<String>, digest: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
            digest: digest.into(),
        }
    }
}

/// Target identity embedded into encoded trace resources.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::exhaustive_structs, reason = "capture data models support direct construction by callers")]
pub struct Target {
    /// Target display name.
    pub name: String,
    /// Target implementation language.
    pub language: String,
}
impl Target {
    /// Construct target identity without imposing lexical validation.
    pub fn new(name: impl Into<String>, language: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            language: language.into(),
        }
    }
}

/// Tool and target metadata embedded into encoded trace resources.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::exhaustive_structs, reason = "capture data models support direct construction by callers")]
pub struct Metadata {
    /// Producing tool version.
    pub tool_version: String,
    /// Target display name.
    pub target_name: String,
    /// Target implementation language.
    pub target_language: String,
    /// Registry identity linked to the trace.
    pub registry: Registry,
}

impl Metadata {
    /// Construct capture metadata without imposing lexical validation.
    pub fn new(tool_version: impl Into<String>, target: Target, registry: Registry) -> Self {
        Self {
            tool_version: tool_version.into(),
            target_name: target.name,
            target_language: target.language,
            registry,
        }
    }
}

/// Encode reference captures into deterministic CTSC OTLP JSON.
///
/// ```
/// use specgate_ctsc::capture::{Metadata, Registry, Target, encode_reference};
/// let metadata = Metadata::new("1", Target::new("target", "rust"), Registry::new("registry", "1", "sha256:digest"));
/// assert!(encode_reference([], &metadata).is_err());
/// ```
///
/// # Errors
/// Returns a structured error for invalid parentage, exhausted identifiers,
/// timestamp overflow, or serialization failure.
pub fn encode_reference(captures: impl AsRef<[Capture]>, metadata: &Metadata) -> Result<Encoding, Error> {
    encoding::encode(captures.as_ref(), metadata, encoding::Identity::REFERENCE)
}

/// Encode candidate replay captures with identity separate from the reference.
///
/// ```
/// use specgate_ctsc::capture::{Metadata, Registry, Target, encode_candidate};
/// let metadata = Metadata::new("1", Target::new("target", "rust"), Registry::new("registry", "1", "sha256:digest"));
/// assert!(encode_candidate([], &metadata).is_err());
/// ```
///
/// # Errors
/// Returns the same structured failures as [`encode_reference`].
pub fn encode_candidate(captures: impl AsRef<[Capture]>, metadata: &Metadata) -> Result<Encoding, Error> {
    encoding::encode(captures.as_ref(), metadata, encoding::Identity::CANDIDATE)
}

mod encoding;
mod otlp;
