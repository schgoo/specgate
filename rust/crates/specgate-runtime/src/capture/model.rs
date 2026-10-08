//! Public native-capture protocol models and validating builders.
//!
//! Parse protocol IDs before assembling a [`Config`], then use
//! [`OperationSpan::builder`] when constructing completed evidence directly.
//!
//! # Examples
//!
//! ```
//! use specgate_runtime::capture::{Config, ConfigDeps, SpanId, TraceId, finish, start};
//!
//! let config = Config::builder(ConfigDeps {
//!     scenario_name: "addition".into(),
//!     trace_id: TraceId::parse("11111111111111111111111111111111")?,
//!     run_id: SpanId::parse("1111111111111101")?,
//!     scenario_id: SpanId::parse("1111111111111102")?,
//! })
//! .start_time(1_000)
//! .clock_step(10)
//! .build()?;
//!
//! start(config)?;
//! assert_eq!(finish()?.scenario_name, "addition");
//! # Ok::<(), specgate_runtime::CaptureError>(())
//! ```

use super::*;

// ---------------------------------------------------------------------------
// Native synchronous operation capture.
// ---------------------------------------------------------------------------

macro_rules! capture_id {
    ($name:ident, $length:expr, $label:literal) => {
        #[doc = concat!("Validated ", $label, ".")]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Validate and construct a lowercase hexadecimal protocol identity.
            ///
            /// # Errors
            /// Returns a configuration error when the text has the wrong width, contains non-hexadecimal text, or is all zeroes.
            pub fn parse(value: impl AsRef<str>) -> Result<Self, CaptureError> {
                Self::from_owned(value.as_ref().to_owned())
            }
            fn from_owned(value: String) -> Result<Self, CaptureError> {
                validate_hex_id($label, &value, $length)?;
                Ok(Self(value))
            }
            /// Borrow the lowercase hexadecimal protocol identity.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
        impl std::ops::Deref for $name {
            type Target = str;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }
        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }
        impl TryFrom<&str> for $name {
            type Error = CaptureError;
            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }
        impl TryFrom<String> for $name {
            type Error = CaptureError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::from_owned(value)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                Self::try_from(value).map_err(serde::de::Error::custom)
            }
        }
    };
}
capture_id!(TraceId, TRACE_HEX_LEN, "trace ID");
capture_id!(SpanId, SPAN_HEX_LEN, "span ID");

/// Deterministic configuration for one thread-local native capture session.
///
/// Construct configurations with [`Config::builder`], which
/// validates protocol identities before exposing a value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Config {
    pub(super) scenario_name: String,
    pub(super) trace_id: TraceId,
    pub(super) run_span_id: SpanId,
    pub(super) scenario_span_id: SpanId,
    #[serde(rename = "operation_span_ids")]
    pub(super) operation_ids: Vec<SpanId>,
    #[serde(rename = "start_time_unix_nano")]
    pub(super) start_ns: i64,
    #[serde(rename = "clock_step_unix_nano")]
    pub(super) step_ns: i64,
}

/// Required identities for [`Config::builder`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::exhaustive_structs, reason = "builder dependencies are an explicit capture protocol")]
pub struct ConfigDeps {
    /// Stable scenario name.
    pub scenario_name: String,
    /// Nonzero lowercase hexadecimal trace identifier.
    pub trace_id: TraceId,
    /// Run span identifier.
    pub run_id: SpanId,
    /// Scenario span identifier.
    pub scenario_id: SpanId,
}

/// Validating builder for native capture configuration.
#[derive(Debug)]
pub struct ConfigBuilder {
    config: Config,
}
/// Default deterministic capture clock origin.
const DEFAULT_START_NS: i64 = 0;
/// Default deterministic capture clock increment.
const DEFAULT_STEP_NS: i64 = 1;
impl Config {
    /// Start a builder with required semantic and span identities.
    #[must_use]
    pub fn builder(deps: impl Into<ConfigDeps>) -> ConfigBuilder {
        let deps = deps.into();
        ConfigBuilder {
            config: Self {
                scenario_name: deps.scenario_name,
                trace_id: deps.trace_id,
                run_span_id: deps.run_id,
                scenario_span_id: deps.scenario_id,
                operation_ids: Vec::new(),
                // Capture clocks start at the Unix epoch and advance one nanosecond by
                // default; changing these protocol defaults alters deterministic bytes.
                start_ns: DEFAULT_START_NS,
                step_ns: DEFAULT_STEP_NS,
            },
        }
    }
}
impl ConfigBuilder {
    /// Set deterministic operation span identifiers.
    #[must_use]
    pub fn operation_ids(mut self, ids: impl Into<Vec<SpanId>>) -> Self {
        self.config.operation_ids = ids.into();
        self
    }
    /// Set the initial logical timestamp.
    #[must_use]
    pub const fn start_time(mut self, value: i64) -> Self {
        self.config.start_ns = value;
        self
    }
    /// Set the logical clock step.
    #[must_use]
    pub const fn clock_step(mut self, value: i64) -> Self {
        self.config.step_ns = value;
        self
    }
    /// Validate and build the configuration.
    ///
    /// # Errors
    /// Returns an error when an identifier or logical clock value is invalid.
    pub fn build(self) -> Result<Config, CaptureError> {
        validate_config(&self.config)?;
        Ok(self.config)
    }
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct UncheckedConfig {
            scenario_name: String,
            trace_id: TraceId,
            run_span_id: SpanId,
            scenario_span_id: SpanId,
            operation_span_ids: Vec<SpanId>,
            start_time_unix_nano: i64,
            clock_step_unix_nano: i64,
        }
        let unchecked = UncheckedConfig::deserialize(deserializer)?;
        let config = Self {
            scenario_name: unchecked.scenario_name,
            trace_id: unchecked.trace_id,
            run_span_id: unchecked.run_span_id,
            scenario_span_id: unchecked.scenario_span_id,
            operation_ids: unchecked.operation_span_ids,
            start_ns: unchecked.start_time_unix_nano,
            step_ns: unchecked.clock_step_unix_nano,
        };
        validate_config(&config).map_err(serde::de::Error::custom)?;
        Ok(config)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::exhaustive_structs,
    reason = "environment activation is an exhaustively serialized private protocol"
)]
/// Environment activation payload for one isolated capture process.
pub struct EnvConfig {
    /// Native capture session configuration.
    pub capture: Config,
    /// Snapshot sidecar destination.
    pub sidecar_path: PathBuf,
}

/// A completed run or scenario span boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::exhaustive_structs,
    reason = "native span boundaries are exhaustively serialized capture evidence"
)]
pub struct SpanBoundary {
    /// Run or scenario span identifier.
    pub span_id: SpanId,
    /// Optional parent run span identifier.
    #[serde(rename = "parent_span_id")]
    pub parent_id: Option<SpanId>,
    /// Start timestamp in Unix nanoseconds.
    #[serde(rename = "start_time_unix_nano")]
    pub start_ns: i64,
    /// End timestamp in Unix nanoseconds.
    #[serde(rename = "end_time_unix_nano")]
    pub end_ns: i64,
    /// Terminal span status.
    pub status: Status,
}

/// Terminal status for a native span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[expect(clippy::exhaustive_enums, reason = "CTSC span status is a closed two-state protocol")]
pub enum Status {
    /// Successful completion.
    Ok,
    /// Error or fault completion.
    Error,
}

/// One native observation captured while an operation scope is active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::exhaustive_structs,
    reason = "native observations are exhaustively serialized capture evidence"
)]
pub struct Observation {
    /// Stable event order within the capture.
    pub order: u64,
    /// Event timestamp in Unix nanoseconds.
    #[serde(rename = "time_unix_nano")]
    pub time_ns: i64,
    /// Semantic observation name.
    pub name: String,
    /// Semantic observation value.
    #[serde(with = "crate::value::wire")]
    pub value: Value,
}

/// The semantic completion recorded at an operation's actual return boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[expect(clippy::exhaustive_enums, reason = "native completion is a closed CTSC terminal-channel protocol")]
pub enum Completion {
    /// Typed result completion.
    Result {
        /// Stable event order.
        order: u64,
        /// Completion timestamp in Unix nanoseconds.
        #[serde(rename = "time_unix_nano")]
        time_ns: i64,
        /// Semantic result value.
        #[serde(with = "crate::value::wire")]
        value: Value,
    },
    /// Explicit empty completion.
    Empty {
        /// Stable event order.
        order: u64,
        /// Completion timestamp in Unix nanoseconds.
        #[serde(rename = "time_unix_nano")]
        time_ns: i64,
    },
    /// Declared error completion.
    Error {
        /// Stable event order.
        order: u64,
        /// Completion timestamp in Unix nanoseconds.
        #[serde(rename = "time_unix_nano")]
        time_ns: i64,
        /// Declared error name.
        name: String,
        #[serde(skip_serializing_if = "Option::is_none", with = "crate::value::wire::optional")]
        /// Optional semantic error payload.
        value: Option<Value>,
    },
    /// Unwind fault completion.
    Fault {
        /// Stable event order.
        order: u64,
        /// Completion timestamp in Unix nanoseconds.
        #[serde(rename = "time_unix_nano")]
        time_ns: i64,
        /// Stable fault category.
        fault_type: String,
        /// Native fault message.
        message: String,
        /// Fault observer identity.
        observer: String,
    },
}

/// One completed native operation span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OperationSpan {
    /// Stable operation order.
    pub order: u64,
    /// Operation span identifier.
    pub span_id: SpanId,
    /// Parent scenario or operation span identifier.
    #[serde(rename = "parent_span_id")]
    pub parent_id: SpanId,
    /// Owning component identifier.
    pub component_id: ComponentId,
    /// Semantic operation name.
    pub operation_name: OperationName,
    /// Start timestamp in Unix nanoseconds.
    #[serde(rename = "start_time_unix_nano")]
    pub start_ns: i64,
    /// End timestamp in Unix nanoseconds.
    #[serde(rename = "end_time_unix_nano")]
    pub end_ns: i64,
    /// Terminal operation status.
    pub status: Status,
    /// Semantic inputs keyed by name.
    #[serde(with = "crate::value::wire::map")]
    pub inputs: BTreeMap<String, Value>,
    /// Ordered observations.
    pub observations: Vec<Observation>,
    /// Recorded semantic completion.
    pub completion: Option<Completion>,
}

/// Required identities and start state for [`OperationSpan::builder`].
#[derive(Debug, Clone)]
#[expect(
    clippy::exhaustive_structs,
    reason = "capture construction uses this fixed dependency set as one typed builder input"
)]
pub struct SpanDeps {
    /// Stable operation order.
    pub order: u64,
    /// Operation span identifier.
    pub span_id: SpanId,
    /// Parent scenario or operation span identifier.
    pub parent_id: SpanId,
    /// Owning component identifier.
    pub component_id: ComponentId,
    /// Semantic operation name.
    pub operation_name: OperationName,
    /// Start timestamp in Unix nanoseconds.
    pub start_ns: i64,
}

/// Controlled builder for a completed native operation span.
#[derive(Debug)]
pub struct SpanBuilder {
    span: OperationSpan,
}
impl OperationSpan {
    /// Begin a native operation span.
    ///
    /// Inputs and events start empty. Completion starts absent and must be set
    /// before [`SpanBuilder::build`].
    ///
    /// # Examples
    /// ```
    /// use specgate_runtime::capture::{
    ///     Completion, OperationSpan, SpanDeps, SpanId,
    /// };
    /// use specgate_runtime::{ComponentId, OperationName};
    ///
    /// let span = OperationSpan::builder(SpanDeps {
    ///     order: 0,
    ///     span_id: SpanId::parse("1111111111111101")?,
    ///     parent_id: SpanId::parse("1111111111111102")?,
    ///     component_id: ComponentId::from("example.component"),
    ///     operation_name: OperationName::from("run"),
    ///     start_ns: 1,
    /// })
    /// .end_time(2)
    /// .completion(Completion::Empty {
    ///     order: 1,
    ///     time_ns: 2,
    /// })
    /// .build()?;
    ///
    /// assert_eq!(span.end_ns, 2);
    /// # Ok::<(), specgate_runtime::CaptureError>(())
    /// ```
    #[must_use]
    pub fn builder(deps: impl Into<SpanDeps>) -> SpanBuilder {
        let deps = deps.into();
        SpanBuilder {
            span: Self {
                order: deps.order,
                span_id: deps.span_id,
                parent_id: deps.parent_id,
                component_id: deps.component_id,
                operation_name: deps.operation_name,
                start_ns: deps.start_ns,
                end_ns: deps.start_ns,
                status: Status::Ok,
                inputs: BTreeMap::new(),
                observations: Vec::new(),
                completion: None,
            },
        }
    }
}
impl SpanBuilder {
    /// Set the terminal timestamp.
    #[must_use]
    pub const fn end_time(mut self, value: i64) -> Self {
        self.span.end_ns = value;
        self
    }
    /// Set the terminal status.
    #[must_use]
    pub fn status(mut self, value: Status) -> Self {
        self.span.status = value;
        self
    }
    /// Set captured semantic inputs.
    #[must_use]
    pub fn inputs(mut self, value: BTreeMap<String, Value>) -> Self {
        self.span.inputs = value;
        self
    }
    /// Set ordered observations.
    #[must_use]
    pub fn observations(mut self, value: Vec<Observation>) -> Self {
        self.span.observations = value;
        self
    }
    /// Set the semantic completion channel.
    #[must_use]
    pub fn completion(mut self, value: Completion) -> Self {
        self.span.completion = Some(value);
        self
    }
    /// Validate terminal timing and completion consistency, then finish the span.
    ///
    /// # Errors
    /// Returns a configuration error when the terminal timestamp precedes the
    /// start, completion is absent, or status disagrees with completion kind.
    pub fn build(self) -> Result<OperationSpan, CaptureError> {
        if self.span.end_ns < self.span.start_ns {
            return Err("native operation end timestamp precedes its start timestamp".to_string().into());
        }
        let completion_is_error = match self.span.completion.as_ref() {
            Some(Completion::Error { .. } | Completion::Fault { .. }) => true,
            Some(Completion::Result { .. } | Completion::Empty { .. }) => false,
            None => return Err("native operation span requires a terminal completion".to_string().into()),
        };
        if completion_is_error != (self.span.status == Status::Error) {
            return Err("native operation status disagrees with its completion kind".to_string().into());
        }
        Ok(self.span)
    }
}

/// Completed native evidence for one deterministic run and scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::exhaustive_structs,
    reason = "native captures are exhaustively serialized runtime evidence"
)]
pub struct Capture {
    /// Capture trace identifier.
    pub trace_id: TraceId,
    /// Scenario name.
    pub scenario_name: String,
    /// Run span boundary.
    pub run: SpanBoundary,
    /// Scenario span boundary.
    pub scenario: SpanBoundary,
    /// Completed operation spans.
    pub operations: Vec<OperationSpan>,
}
