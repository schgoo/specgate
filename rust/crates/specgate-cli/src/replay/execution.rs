//! Reusable candidate discovery and batched execution.
#[cfg(any(test, feature = "test-util"))]
use super::runner::{execute_in, scratch};
use super::{
    Arc, Candidate, CommandEnvironment, Discovery, Error, ErrorKind, HashMap, Path, PathBuf, Registry, Schema, SchemaLookup, failure,
};
#[cfg(any(test, feature = "test-util"))]
use super::{Plan, Report, build_plan, decode_bundle, manifest_file, read_bundle, reference_file, registry_file, write};

/// One Rust candidate target discovered once and linked against many capture
/// bundles.
///
/// Candidate discovery is the expensive part of replay, so a batch pays it a
/// single time and every planned component reuses the same link-time metadata.
#[derive(Debug, Clone)]
pub(crate) struct Candidates {
    inner: Arc<CandidatesInner>,
}
/// Shared discovered target identity, registry, and per-component schemas.
///
/// Every candidate cloned from this value retains the same Cargo package and
/// runtime source, preserving package identity throughout linking and execution.
#[derive(Debug, Clone)]
pub(crate) struct CandidatesInner {
    pub(super) target_name: specgate_discovery::identity::TargetName,
    pub(super) language: specgate_discovery::binding::Language,
    pub(super) package_name: specgate_discovery::identity::PackageName,
    pub(super) package_version: specgate_discovery::identity::PackageVersion,
    pub(super) package_root: PathBuf,
    pub(super) runtime: specgate_discovery::runner::PackageSource,
    pub(super) raw_registry: Registry,
    pub(super) schemas: HashMap<specgate_discovery::identity::ComponentId, Schema>,
}
impl std::ops::Deref for Candidates {
    type Target = CandidatesInner;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Borrowed candidate-selection inputs for one discovery pass.
pub(super) struct DiscoveryInput<'a> {
    pub(super) binding: &'a Path,
    pub(super) target: Option<&'a specgate_discovery::identity::TargetName>,
    pub(super) components: &'a [specgate_discovery::identity::ComponentId],
}

/// Concrete system boundary and optional pre-discovered metadata used by tests.
pub(super) struct DiscoveryContext<'a> {
    pub(super) system: &'a CommandEnvironment,
    pub(super) discovery: &'a Discovery,
    #[cfg(any(test, feature = "test-util"))]
    pub(super) discovered: Option<Candidates>,
}

impl Candidates {
    /// Resolve and discover through the real environment for internal batch tooling.
    #[cfg(any(test, feature = "test-util"))]
    #[cfg_attr(not(test), expect(dead_code, reason = "test-util support is consumed by external test builds"))]
    pub(crate) fn discover(
        binding: impl AsRef<Path>,
        target: Option<&specgate_discovery::identity::TargetName>,
        components: &[specgate_discovery::identity::ComponentId],
    ) -> Result<Self, Error> {
        let binding = binding.as_ref();
        Self::discover_with(
            &DiscoveryInput {
                binding,
                target,
                components,
            },
            &DiscoveryContext {
                system: &CommandEnvironment::real(),
                discovery: &Discovery::real(),
                discovered: None,
            },
        )
    }
    /// Discover through concrete boundaries, optionally accepting already-resolved metadata.
    pub(super) fn discover_with(input: &DiscoveryInput<'_>, context: &DiscoveryContext<'_>) -> Result<Self, Error> {
        #[cfg(any(test, feature = "test-util"))]
        if let Some(candidate) = &context.discovered {
            return Ok(candidate.clone());
        }
        let components = input.components;
        let discovered = context
            .discovery
            .batch(
                input.binding,
                input.target.map(specgate_discovery::identity::TargetName::as_str),
                components,
            )
            .map_err(|error| Error::wrap(ErrorKind::Linking, "candidate discovery failed", error))?;
        let language = discovered.target.language;
        if language != specgate_discovery::binding::Language::Rust {
            return failure(format!(
                "replay currently supports only Rust candidates; binding language is '{language}'"
            ));
        }
        let target_name = discovered.target.name.clone();
        let cargo = discovered
            .cargo_context
            .clone()
            .ok_or_else(|| "Rust discovery returned no Cargo source context".to_string())?;
        let package_root = cargo.path;
        if !context.system.is_file(package_root.join("Cargo.toml")) {
            return failure(format!(
                "candidate target '{target_name}' is not a Rust package (no Cargo.toml at {})",
                package_root.display()
            ));
        }
        let raw_registry = discovered
            .registries
            .first()
            .cloned()
            .ok_or_else(|| "Rust discovery produced no registry document".to_string())?;
        let mut schemas = HashMap::with_capacity(components.len());
        for component in components.iter().filter(|name| !name.is_empty()) {
            match discovered.schema(component) {
                SchemaLookup::Found(schema) => {
                    schemas.insert((*component).clone(), schema.clone());
                }
                SchemaLookup::Invalid(reason) => return failure(reason.to_string()),
                SchemaLookup::Missing => {
                    return failure(format!("candidate discovery returned no metadata for component '{component}'"));
                }
            }
        }
        schemas.shrink_to_fit();
        Ok(Self {
            inner: Arc::new(CandidatesInner {
                target_name,
                language,
                package_name: cargo.package,
                package_version: cargo.version,
                package_root,
                runtime: cargo.runtime,
                raw_registry,
                schemas,
            }),
        })
    }

    pub(super) fn component(&self, component: impl AsRef<str>) -> Result<Candidate, Error> {
        let component = specgate_discovery::identity::ComponentId::from(component.as_ref());
        let schema = self
            .schemas
            .get(&component)
            .ok_or_else(|| format!("candidate was not discovered for component '{component}'"))?;
        Ok(Candidate {
            inner: Arc::clone(&self.inner),
            schema: schema.clone(),
        })
    }

    /// Statically link one verified capture bundle to this candidate without
    /// generating or building a runner.
    #[cfg(any(test, feature = "test-util"))]
    #[cfg_attr(not(test), expect(dead_code, reason = "test-util support is consumed by external test builds"))]
    pub(crate) fn plan(&self, capture_dir: impl AsRef<Path>) -> Result<Plan, Error> {
        self.plan_with(capture_dir, &CommandEnvironment::real())
    }

    #[cfg(any(test, feature = "test-util"))]
    fn plan_with(&self, capture_dir: impl AsRef<Path>, system: &CommandEnvironment) -> Result<Plan, Error> {
        let capture_dir = capture_dir.as_ref();
        let manifest = read_bundle(capture_dir, manifest_file(), system)?;
        let registry = read_bundle(capture_dir, registry_file(), system)?;
        let reference = read_bundle(capture_dir, reference_file(), system)?;
        let bundle = decode_bundle(&manifest, &registry, &reference)
            .map_err(|source| Error::wrap(ErrorKind::Capture, source.to_string(), source))?;
        let candidate = self.component(&bundle.component_id)?;
        build_plan(&bundle, &candidate)
    }

    /// Execute already-linked plans, reusing one generated runner package and
    /// one Cargo target directory for the whole batch.
    #[cfg(any(test, feature = "test-util"))]
    #[cfg_attr(not(test), expect(dead_code, reason = "test-util support is consumed by external test builds"))]
    pub(crate) fn execute(&self, plans: &[(Plan, PathBuf)]) -> Result<Vec<Report>, Error> {
        self.execute_with(plans, &super::Execution::real(), &CommandEnvironment::real())
    }

    #[cfg(any(test, feature = "test-util"))]
    fn execute_with(
        &self,
        plans: &[(Plan, PathBuf)],
        execution: &super::Execution,
        system: &CommandEnvironment,
    ) -> Result<Vec<Report>, Error> {
        let scratch = scratch(system)?;
        run_batch(plans, scratch.path(), &self.package_name, |plan, out, root| {
            let captures = execute_in(plan, root, execution, system)?;
            write(plan, &captures, out, system)
        })
    }
}
#[cfg(any(test, feature = "test-util"))]
pub(super) fn run_batch<T>(
    plans: &[(Plan, PathBuf)],
    scratch: impl AsRef<Path>,
    package_name: &specgate_discovery::identity::PackageName,
    mut run: impl FnMut(&Plan, &Path, &Path) -> Result<T, Error>,
) -> Result<Vec<T>, Error> {
    let scratch = scratch.as_ref();
    let mut results = Vec::with_capacity(plans.len());
    for (plan, out) in plans {
        assert_eq!(
            plan.target.package_name, *package_name,
            "plan for '{}' must target this candidate package",
            plan.component_id
        );
        results.push(run(plan, out, scratch)?);
    }
    Ok(results)
}
