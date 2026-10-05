//! Binding discovery and deterministic CTSC registry publication.

mod failure;
pub use failure::{Error, ErrorKind};

use crate::system::{CommandEnvironment, Discovery};
use specgate::spec_operation;
use specgate_ctsc::registry::encode as encode_registry;
use std::path::{Path, PathBuf};
mod model;
#[doc(inline)]
pub use model::{ComponentName, Params, RegistryId, RegistryVersion, Report, Request, RequestBuilder};
use model::{DiscoverReport, DiscoverRequest};

/// Discover one target and publish compact CTSC registry JSON.
///
/// # Errors
/// Returns a categorized error for request, discovery, encoding, or publication failures.
///
/// # Examples
/// ```no_run
/// use specgate_cli::discover::{ComponentName, Params, Request, RegistryId, RegistryVersion, discover};
/// let request = Request::builder(Params {
///     binding: "binding.yaml".into(),
///     out: "registry.ctsc.json".into(),
///     component: ComponentName::parse("example.math")?,
///     registry_id: RegistryId::parse("urn:ctsc:registry:example.math")?,
///     registry_version: RegistryVersion::parse("1.0.0")?,
/// })
///     .build()?;
/// let report = discover(request)?;
/// assert_eq!(report.component_id.as_str(), "example.math");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[spec_operation("discover")]
pub fn discover(request: DiscoverRequest) -> Result<DiscoverReport, failure::DiscoverError> {
    discover_with(&request, &CommandEnvironment::real(), &Discovery::real())
}

/// Discover through caller-supplied concrete services.
///
/// `system` publishes the registry while `discovery` resolves and inspects the
/// target. Use [`CommandEnvironment::real`] and [`Discovery::real`] in production, or
/// enable `test-util` and queue a result on [`Discovery::fake`] in tests.
///
/// # Examples
/// ```no_run
/// # use specgate_cli::{Discovery, CommandEnvironment};
/// # use specgate_cli::discover::{ComponentName, Params, Request, RegistryId, RegistryVersion, discover_with};
/// # let request = Request::builder(Params { binding: "binding.yaml".into(), out: "registry.json".into(), component: ComponentName::parse("example.math")?, registry_id: RegistryId::parse("id")?, registry_version: RegistryVersion::parse("1")? }).build()?;
/// let report = discover_with(&request, &CommandEnvironment::real(), &Discovery::real())?;
/// assert_eq!(report.component_id.as_str(), "example.math");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Errors
/// Returns request, discovery, registry-encoding, or publication errors.
pub fn discover_with(request: &Request, system: &CommandEnvironment, discovery: &Discovery) -> Result<Report, Error> {
    let target = selected_target(request.target());
    let component = specgate_discovery::identity::ComponentId::from(request.component());
    let discovered = discovery.target(request.binding(), target, &component)?;
    publish_schema(request, &discovered.schema, system)
}

/// Discover and publish multiple components from one shared binding target.
///
/// The batch must use one binding and target; discovery loads that target once,
/// then each request retains its own registry identity, version, and output.
///
/// # Examples
/// ```no_run
/// use specgate_cli::discover::{ComponentName, Params, RegistryId, RegistryVersion, Request, batch};
/// fn request(component: &str, output: &str) -> Result<Request, specgate_cli::discover::Error> {
///     Request::builder(Params {
///         binding: "binding.yaml".into(),
///         out: output.into(),
///         component: ComponentName::parse(component)?,
///         registry_id: RegistryId::parse(format!("registry:{component}"))?,
///         registry_version: RegistryVersion::parse("1")?,
///     }).target("release").build()
/// }
/// let reports = batch([
///     request("example.math", "math.registry.json")?,
///     request("example.text", "text.registry.json")?,
/// ])?;
/// assert_eq!(reports.len(), 2);
/// # Ok::<(), specgate_cli::discover::Error>(())
/// ```
///
/// # Errors
/// Returns a categorized request, discovery, encoding, or publication failure.
pub fn batch(requests: impl IntoIterator<Item = Request>) -> Result<Vec<Report>, Error> {
    batch_with(requests, &CommandEnvironment::real(), &Discovery::real())
}

/// Discover a batch through caller-supplied concrete services.
///
/// All requests must share a binding and target. The concrete discovery service
/// performs one batch lookup and the system service publishes each registry.
/// Tests can inject the same feature-gated fakes accepted by [`discover_with`].
///
/// # Examples
/// ```no_run
/// use specgate_cli::discover::{ComponentName, Params, RegistryId, RegistryVersion, Request, batch_with};
/// use specgate_cli::{CommandEnvironment, Discovery};
/// let make = |component: &str, output: &str| -> Result<Request, specgate_cli::discover::Error> {
///     Request::builder(Params {
///         binding: "binding.yaml".into(),
///         out: output.into(),
///         component: ComponentName::parse(component)?,
///         registry_id: RegistryId::parse(format!("registry:{component}"))?,
///         registry_version: RegistryVersion::parse("1")?,
///     }).target("release").build()
/// };
/// let requests = [make("example.math", "math.json")?, make("example.text", "text.json")?];
/// let reports = batch_with(requests, &CommandEnvironment::real(), &Discovery::real())?;
/// for report in reports { println!("{}", report.output_path.display()); }
/// # Ok::<(), specgate_cli::discover::Error>(())
/// ```
///
/// # Errors
/// Returns request errors for mixed bindings or targets, plus discovery, registry-encoding, or publication errors.
pub fn batch_with(
    requests: impl IntoIterator<Item = Request>,
    system: &CommandEnvironment,
    discovery: &Discovery,
) -> Result<Vec<Report>, Error> {
    let requests = requests.into_iter().collect::<Vec<_>>();
    let Some(first) = requests.first() else { return Ok(Vec::new()) };
    if requests
        .iter()
        .any(|request| request.binding() != first.binding() || request.target() != first.target())
    {
        return Err(Error::request("batched discovery requires one shared binding and target"));
    }
    let target = selected_target(first.target());
    let components = requests
        .iter()
        .map(|request| specgate_discovery::identity::ComponentId::from(request.component()))
        .collect::<Vec<_>>();
    let batch = discovery.batch(first.binding(), target, &components)?;
    requests
        .into_iter()
        .map(|request| {
            let schema = match batch.schema(request.component()) {
                specgate_discovery::output::SchemaLookup::Found(schema) => schema,
                specgate_discovery::output::SchemaLookup::Invalid(reason) => {
                    return Err(Error::discovery(reason.to_string(), std::io::Error::other(reason.to_string())));
                }
                specgate_discovery::output::SchemaLookup::Missing => {
                    return Err(Error::discovery(
                        format!("discovery returned no metadata for component '{}'", request.component()),
                        std::io::Error::other("component metadata missing"),
                    ));
                }
            };
            publish_schema(&request, schema, system)
        })
        .collect()
}

/// Convert the request convention into optional target selection.
///
/// An empty target means "use the binding default"; non-empty names select an
/// explicit target.
fn selected_target(target: &str) -> Option<&str> {
    (!target.is_empty()).then_some(target)
}

fn publish_schema(request: &Request, schema: &specgate_discovery::schema::Schema, system: &CommandEnvironment) -> Result<Report, Error> {
    let schema_json = serde_json::to_string(schema)
        .map_err(|error| Error::encoding(format!("failed to serialize normalized discovery schema: {error}")))?;
    let encoded = encode_registry(
        request.registry_id().to_string(),
        request.registry_version().to_string(),
        specgate_ctsc::registry::Schema::new(&schema_json),
    )
    .map_err(|error| Error::encoding(error.to_string()))?;
    if let Some(parent) = request.out().parent().filter(|parent| !parent.as_os_str().is_empty()) {
        system
            .create_dir_all(parent)
            .map_err(|error| Error::publication(format!("failed to create output directory {}: {error}", parent.display()), error))?;
    }
    system
        .write(request.out(), encoded.registry_json)
        .map_err(|error| Error::publication(format!("failed to write registry to {}: {error}", request.out().display()), error))?;
    Ok(Report {
        component_id: request.component_name(),
        component_event: request.component().to_string(),
        operations: encoded.operation_count,
        types: encoded.type_count,
        output_path: request.out().to_path_buf(),
    })
}

/// Format a discovery result for CLI display.
///
/// ```no_run
/// use specgate_cli::discover::{Error, Report, format_outcome};
/// # fn outcome() -> Result<Report, Error> { todo!("supply the discovery outcome") }
/// print!("{}", format_outcome(&outcome()));
/// ```
#[must_use]
pub fn format_outcome(outcome: &Result<Report, Error>) -> String {
    match outcome {
        Ok(report) => format!(
            "Complete(component={}, operations={}, types={}, output={})\n",
            report.component_id,
            report.operations,
            report.types,
            report.output_path.display()
        ),
        Err(error) => format!("Error({})\n", error.diagnostic()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> PathBuf {
        std::env::current_dir()
            .unwrap()
            .ancestors()
            .find(|path| path.join("rust").join("Cargo.toml").is_file())
            .expect("repository root")
            .to_path_buf()
    }

    fn output_path(label: impl AsRef<str>) -> PathBuf {
        let label = label.as_ref();
        repo_root()
            .join("rust")
            .join("target")
            .join(format!("specgate-cli-{label}-{}.json", std::process::id()))
    }

    fn focused_binding(language: impl AsRef<str>) -> PathBuf {
        let language = language.as_ref();
        repo_root()
            .join("rust")
            .join("crates")
            .join("specgate-cli")
            .join("tests")
            .join("fixtures")
            .join(format!("{language}.binding.yaml"))
    }

    fn fixture_request(component: &str, binding: PathBuf, out: impl Into<PathBuf>) -> Request {
        Request::builder(Params {
            binding,
            out: out.into(),
            component: ComponentName::parse(component).expect("component"),
            registry_id: RegistryId::parse(format!("registry:{component}")).expect("registry id"),
            registry_version: RegistryVersion::parse("1").expect("registry version"),
        })
        .build()
        .expect("request")
    }

    fn clone_batch(batch: &specgate_discovery::output::Batch) -> specgate_discovery::output::Batch {
        use specgate_discovery::output::Component;
        specgate_discovery::output::Batch {
            target: batch.target.clone(),
            cargo_context: batch.cargo_context.clone(),
            registry_json: batch.registry_json.clone(),
            registries: batch.registries.clone(),
            components: batch
                .components
                .iter()
                .map(|(component, discovered)| {
                    (
                        component.clone(),
                        Component {
                            registry_index: discovered.registry_index,
                            schema: Ok(discovered.schema.as_ref().expect("successful fixture schema").clone()),
                        },
                    )
                })
                .collect(),
            present_components: batch.present_components.clone(),
        }
    }

    #[test]
    fn batch_api() {
        use specgate_discovery::discover_batch as discover_components;

        assert!(batch(Vec::<Request>::new()).expect("empty public batch").is_empty());
        let binding = focused_binding("rust");
        let component = specgate_discovery::identity::ComponentId::from("fixture.cli.replay");
        let discovered =
            discover_components(binding.to_str().expect("fixture path"), None, std::slice::from_ref(&component)).expect("fixture batch");

        let discovery = Discovery::fake();
        discovery.push_batch(Ok(clone_batch(&discovered)));
        let system = CommandEnvironment::fake();
        let report = batch_with(
            [fixture_request("fixture.cli.replay", binding.clone(), "published/replay.json")],
            &system,
            &discovery,
        )
        .expect("fake batch publication");
        assert_eq!(report.len(), 1);
        assert!(format_outcome(&Ok(report[0].clone())).starts_with("Complete(component=fixture.cli.replay"));

        let mixed = batch_with(
            [
                fixture_request("fixture.cli.replay", binding.clone(), "one.json"),
                fixture_request("fixture.cli.replay", "different.binding.yaml".into(), "two.json"),
            ],
            &system,
            &discovery,
        )
        .expect_err("mixed bindings");
        assert!(format_outcome(&Err(mixed)).starts_with("Error("));

        discovery.push_batch(Ok(clone_batch(&discovered)));
        let missing = batch_with(
            [fixture_request("fixture.cli.unknown", binding.clone(), "missing.json")],
            &system,
            &discovery,
        )
        .expect_err("missing component metadata");
        assert!(missing.is_discovery());

        discovery.push_batch(Ok(discovered));
        system.fail_next("publication denied");
        let publication = batch_with(
            [fixture_request("fixture.cli.replay", binding, "denied/replay.json")],
            &system,
            &discovery,
        )
        .expect_err("publication failure");
        assert!(publication.is_publication());
    }

    #[test]
    fn batch_parity() {
        use specgate_discovery::schema::Schema;
        use specgate_discovery::{
            discover_batch as discover_components,
            output::{Batch, SchemaLookup},
        };

        fn batch_schema<'a>(batch: &'a Batch, component: &str) -> &'a Schema {
            match batch.schema(component) {
                SchemaLookup::Found(schema) => schema,
                SchemaLookup::Invalid(error) => panic!("{component} normalization failed: {error}"),
                SchemaLookup::Missing => panic!("missing metadata for {component}"),
            }
        }

        let components =
            ["fixture.cli.multiple", "fixture.cli.replay", "fixture.cli.setup"].map(specgate_discovery::identity::ComponentId::from);
        let rust = discover_components(focused_binding("rust").to_str().unwrap(), None, &components).expect("batched Rust discovery");
        assert_eq!(rust.registry_json.len(), 1, "Rust link-time discovery self-reports once");
        assert_eq!(rust.registries.len(), 1);
        for component in &components {
            let discovered = rust
                .components
                .get(component)
                .unwrap_or_else(|| panic!("missing Rust metadata for {component}"));
            assert_eq!(discovered.registry_index, 0, "every Rust component shares one document");
            assert!(discovered.schema.is_ok(), "{component} normalization failed");
        }
        assert!(rust.registry("fixture.cli.replay").is_some());
        assert!(rust.registry_json("fixture.cli.setup").is_some());

        let replay = batch_schema(&rust, "fixture.cli.replay");
        assert_eq!(replay.operations.len(), 2);
        assert!(replay.types.is_empty());
        let setup = batch_schema(&rust, "fixture.cli.setup");
        assert_eq!(setup.operations.len(), 1);
        assert_eq!(setup.types.len(), 1);
        assert_eq!(setup.operations[0].name, "increment");
        assert_eq!(setup.operations[0].inputs.len(), 1);
        assert_eq!(setup.operations[0].inputs[0].name, "initial");
        let multiple = batch_schema(&rust, "fixture.cli.multiple");
        assert_eq!(
            multiple
                .operations
                .iter()
                .map(|operation| operation.name.as_str())
                .collect::<Vec<_>>(),
            vec!["unexercised", "used"]
        );

        let csharp = discover_components(focused_binding("csharp").to_str().unwrap(), None, &components).expect("batched C# discovery");
        assert_eq!(
            csharp.registry_json.len(),
            components.len(),
            "C# reflection emits one document per component"
        );
        for (position, component) in components.iter().enumerate() {
            let discovered = csharp
                .components
                .get(component)
                .unwrap_or_else(|| panic!("missing C# metadata for {component}"));
            assert_eq!(discovered.registry_index, position, "C# documents stay aligned with request order");
            let csharp_schema = discovered.schema.as_ref().expect("C# schema");
            let rust_schema = batch_schema(&rust, component);
            assert_eq!(
                csharp_schema, rust_schema,
                "batched C# discovery must normalize to the Rust canonical for {component}"
            );
        }

        // Rich structured/sum-type discovery remains exhaustively covered by
        // the complete 69-row CTSC golden matrix. This fixture isolates CLI
        // batching, setup folding, source operation identity, and C# parity.
        let schema_json = serde_json::to_string(replay).unwrap();
        let first = encode_registry(
            "urn:ctsc:registry:fixture.cli.replay".to_string(),
            "1.0.0".to_string(),
            specgate_ctsc::registry::Schema::new(&schema_json),
        )
        .unwrap()
        .registry_json;
        let second = encode_registry(
            "urn:ctsc:registry:fixture.cli.replay".to_string(),
            "1.0.0".to_string(),
            specgate_ctsc::registry::Schema::new(&schema_json),
        )
        .unwrap()
        .registry_json;
        assert_eq!(first, second);
        assert!(!first.contains('\n'), "registry JSON must be compact");
        let document: serde_json::Value = serde_json::from_str(&first).unwrap();
        assert_eq!(document["format"], "ctsc.registry");
        assert_eq!(document["formatVersion"], "0.2.0");
    }

    #[test]
    fn error_without_panic() {
        let output = output_path("discover-error");
        let request = Request::builder(Params {
            binding: "missing-binding.yaml".into(),
            out: output.clone(),
            component: ComponentName::parse("fixture.stateless_add").unwrap(),
            registry_id: RegistryId::parse("urn:ctsc:registry:fixture.stateless-add:1").unwrap(),
            registry_version: RegistryVersion::parse("1.0.0").unwrap(),
        })
        .build()
        .unwrap();
        let outcome = discover(request);
        outcome.unwrap_err();
        assert!(!output.exists());
    }

    fn request(binding: impl AsRef<Path>, out: impl AsRef<Path>) -> Result<Request, Error> {
        Request::builder(Params {
            binding: binding.as_ref().into(),
            out: out.as_ref().into(),
            component: ComponentName::parse("example.math")?,
            registry_id: RegistryId::parse("urn:ctsc:registry:example.math")?,
            registry_version: RegistryVersion::parse("1")?,
        })
        .target("rust")
        .build()
    }

    #[test]
    fn request_validation() {
        assert!(request("", "registry.json").unwrap_err().is_request());
        assert!(request("binding.yaml", "").unwrap_err().is_request());
        let empty_component = ComponentName::parse("").unwrap_err();
        assert!(empty_component.is_request());
        let built = request("binding.yaml", "registry.json").unwrap();
        assert_eq!(built.target(), "rust");
        assert_eq!(built.component(), "example.math");
    }
}
