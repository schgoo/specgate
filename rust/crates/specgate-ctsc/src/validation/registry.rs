use super::model::{
    CTSC_VERSION, NamedType, RawText, RegistryComponent, RegistryDocument, RegistryOperation, RegistrySet, ResolvedComponent, TypeRef,
};
use super::{DocumentBytes, Loaded, ValidationIssue, is_digest, issue, located, sha256_digest};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

mod shape;
mod uri;
use shape::{document_uses, validate_shape};
use uri::resolve_uri;

// Normative reserved extension namespace; changing it changes accepted registry compatibility.
const RESERVED_NAMESPACE: &str = "conformance.";
// Normative CTSC registry discriminator; changing it changes accepted registry compatibility.
const REGISTRY_FORMAT: &str = "ctsc.registry";

#[derive(Debug)]
struct RegistryFile {
    path: PathBuf,
    digest: RawText,
    document: RegistryDocument,
}

struct Filesystem<'a> {
    inner: FilesystemInner<'a>,
}

enum FilesystemInner<'a> {
    Real,
    Memory(BTreeMap<PathBuf, Vec<u8>>),
    Reader(&'a dyn crate::comparison::DocumentReader),
}

impl Filesystem<'_> {
    fn real() -> Self {
        Self {
            inner: FilesystemInner::Real,
        }
    }

    fn from_documents(root: DocumentBytes<'_>, explicit_imports: &[DocumentBytes<'_>]) -> Self {
        let files = std::iter::once(root)
            .chain(explicit_imports.iter().copied())
            .map(|document| (document.path.to_path_buf(), document.bytes.to_vec()))
            .collect();
        Self {
            inner: FilesystemInner::Memory(files),
        }
    }

    fn from_reader(reader: &dyn crate::comparison::DocumentReader) -> Filesystem<'_> {
        Filesystem {
            inner: FilesystemInner::Reader(reader),
        }
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf, crate::comparison::LoadError> {
        match &self.inner {
            FilesystemInner::Real => crate::comparison::SystemReader::system().canonicalize(path),
            FilesystemInner::Memory(_) => Ok(path.to_path_buf()),
            FilesystemInner::Reader(reader) => reader.canonicalize(path),
        }
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>, crate::comparison::LoadError> {
        match &self.inner {
            FilesystemInner::Real => crate::comparison::SystemReader::system().read(path),
            FilesystemInner::Memory(files) => files
                .get(path)
                .cloned()
                .ok_or_else(|| crate::comparison::LoadError::reading(path, std::io::ErrorKind::NotFound.into())),
            FilesystemInner::Reader(reader) => reader.read(path),
        }
    }
}

pub(crate) fn load_set<P: AsRef<Path>>(root: impl AsRef<Path>, explicit_imports: &[P]) -> Loaded<RegistrySet> {
    load_fs(root.as_ref(), explicit_imports, &Filesystem::real())
}

pub(crate) fn load_reader<P: AsRef<Path>>(
    root: impl AsRef<Path>,
    explicit_imports: &[P],
    reader: &impl crate::comparison::DocumentReader,
) -> Loaded<RegistrySet> {
    load_fs(root.as_ref(), explicit_imports, &Filesystem::from_reader(reader))
}

pub(crate) fn load_bytes(root: DocumentBytes<'_>, explicit_imports: &[DocumentBytes<'_>]) -> Loaded<RegistrySet> {
    let filesystem = Filesystem::from_documents(root, explicit_imports);
    let import_paths = explicit_imports
        .iter()
        .map(|document| document.path.to_path_buf())
        .collect::<Vec<_>>();
    load_fs(root.path, &import_paths, &filesystem)
}

fn load_fs<P: AsRef<Path>>(root: &Path, explicit_imports: &[P], filesystem: &Filesystem<'_>) -> Loaded<RegistrySet> {
    let mut issues = Vec::new();
    let root = canonicalize(root, filesystem);
    let mut inputs = Vec::with_capacity(explicit_imports.len().saturating_add(1));
    inputs.push(root.clone());
    inputs.extend(explicit_imports.iter().map(|path| canonicalize(path.as_ref(), filesystem)));
    let mut explicit_seen = BTreeSet::new();
    for path in &inputs {
        if !explicit_seen.insert(path.clone()) {
            issue(located(path, "$"), "duplicate registry input path", &mut issues);
        }
    }

    let mut files_by_path = BTreeMap::<PathBuf, RegistryFile>::new();
    let mut paths_by_id = BTreeMap::<RawText, Vec<PathBuf>>::new();
    for path in inputs {
        if files_by_path.contains_key(&path) {
            continue;
        }
        if let Some(file) = load_file(&path, true, filesystem, &mut issues) {
            insert_candidate(file, &mut files_by_path, &mut paths_by_id, &mut issues);
        }
    }

    if !files_by_path.contains_key(&root) {
        return Loaded { value: None, issues };
    }

    let mut selected_paths = BTreeSet::from([root.clone()]);
    let mut pending = vec![root.clone()];
    let mut cursor = 0;
    while cursor < pending.len() {
        let path = pending[cursor].clone();
        cursor += 1;
        let document = files_by_path
            .get(&path)
            .expect("pending registry path must have a loaded file")
            .document
            .clone();
        for (index, import) in document.imports.iter().enumerate() {
            let location = format!("$.imports[{index}]");
            let used = document_uses(&document, import.registry_id.as_str());
            if let Some(uri) = import.uri.as_deref() {
                match resolve_uri(&path, uri, filesystem) {
                    Ok(Some(candidate_path)) if !files_by_path.contains_key(&candidate_path) => {
                        if let Some(file) = load_file(&candidate_path, false, filesystem, &mut issues) {
                            insert_candidate(file, &mut files_by_path, &mut paths_by_id, &mut issues);
                        }
                    }
                    Ok(_) => {}
                    Err(error) => issue(located(&path, format!("{location}.uri")), error.to_string(), &mut issues),
                }
            }
            let candidates = paths_by_id.get(import.registry_id.as_str()).cloned().unwrap_or_default();
            let Some(candidate_path) = candidates.first() else {
                if used {
                    issue(
                        located(&path, &location),
                        format!("unresolved import '{}'; supply --import or a local file: URI", import.registry_id),
                        &mut issues,
                    );
                }
                continue;
            };
            if candidates.len() != 1 {
                issue(
                    located(&path, &location),
                    format!("import '{}' has ambiguous candidate registry files", import.registry_id),
                    &mut issues,
                );
                continue;
            }
            let candidate = files_by_path
                .get(candidate_path)
                .expect("registry ID index must reference a loaded file");
            let version_matches = candidate.document.version == import.version;
            let digest_matches = candidate.digest == import.digest;
            require(
                version_matches,
                &path,
                &location,
                "imported registry version does not match referenced version",
                &mut issues,
            );
            require(
                digest_matches,
                &path,
                &location,
                "imported registry digest does not match exact file bytes",
                &mut issues,
            );
            if version_matches && digest_matches && selected_paths.insert(candidate_path.clone()) {
                pending.push(candidate_path.clone());
            }
        }
    }

    let documents = selected_paths
        .iter()
        .map(|path| {
            let file = files_by_path.get(path).expect("selected registry path must have a loaded file");
            (file.document.registry_id.clone(), file.document.clone())
        })
        .collect::<BTreeMap<_, _>>();
    let document_paths = selected_paths
        .iter()
        .map(|path| {
            let file = files_by_path.get(path).expect("selected registry path must have a loaded file");
            (file.document.registry_id.clone(), file.path.clone())
        })
        .collect::<BTreeMap<_, _>>();
    validate_cycles(&documents, &document_paths, &mut issues);
    validate_semantics(&documents, &document_paths, &mut issues);

    let mut components = BTreeMap::new();
    for (registry_id, document) in &documents {
        for component in &document.components {
            if let Some(existing) = components.insert(
                component.id.clone(),
                ResolvedComponent {
                    registry_id: registry_id.clone(),
                    component: component.clone(),
                },
            ) {
                issue(
                    located(&root, "$.components"),
                    format!(
                        "component ID '{}' exists in both '{}' and '{}'",
                        component.id, existing.registry_id, registry_id
                    ),
                    &mut issues,
                );
            }
        }
    }
    let root_file = files_by_path.get(&root).expect("validated root registry must have a loaded file");
    issues.shrink_to_fit();
    Loaded {
        value: Some(RegistrySet {
            root_id: root_file.document.registry_id.clone(),
            root_version: root_file.document.version.clone(),
            root_digest: root_file.digest.clone(),
            components,
        }),
        issues,
    }
}

fn load_file(path: &Path, report_read_error: bool, filesystem: &Filesystem, issues: &mut Vec<ValidationIssue>) -> Option<RegistryFile> {
    let bytes = match filesystem.read(path) {
        Ok(bytes) => bytes,
        Err(error) if report_read_error => {
            issue(located(path, "$"), format!("failed to read file: {error}"), issues);
            return None;
        }
        Err(_) => return None,
    };
    let document = match serde_json::from_slice::<RegistryDocument>(&bytes) {
        Ok(document) => document,
        Err(error) => {
            issue(located(path, "$"), format!("invalid CTSC registry JSON: {error}"), issues);
            return None;
        }
    };
    validate_shape(&document, path, issues);
    Some(RegistryFile {
        path: path.to_path_buf(),
        digest: RawText::from(sha256_digest(&bytes)),
        document,
    })
}

fn insert_candidate(
    file: RegistryFile,
    files_by_path: &mut BTreeMap<PathBuf, RegistryFile>,
    paths_by_id: &mut BTreeMap<RawText, Vec<PathBuf>>,
    issues: &mut Vec<ValidationIssue>,
) {
    let paths = paths_by_id.entry(file.document.registry_id.clone()).or_default();
    if let Some(existing) = paths.first() {
        issue(
            located(&file.path, "$.registryId"),
            format!(
                "registry ID '{}' has ambiguous candidates {} and {}",
                file.document.registry_id,
                existing.display(),
                file.path.display()
            ),
            issues,
        );
    }
    paths.push(file.path.clone());
    files_by_path.insert(file.path.clone(), file);
}

fn validate_cycles(documents: &BTreeMap<RawText, RegistryDocument>, paths: &BTreeMap<RawText, PathBuf>, issues: &mut Vec<ValidationIssue>) {
    fn visit<'a>(
        registry_id: &'a str,
        documents: &'a BTreeMap<RawText, RegistryDocument>,
        visiting: &mut Vec<&'a str>,
        visited: &mut BTreeSet<&'a str>,
        paths: &BTreeMap<RawText, PathBuf>,
        issues: &mut Vec<ValidationIssue>,
    ) {
        if visited.contains(registry_id) {
            return;
        }
        if let Some(position) = visiting.iter().position(|item| *item == registry_id) {
            let mut cycle = Vec::with_capacity(visiting.len() - position + 1);
            cycle.extend_from_slice(&visiting[position..]);
            cycle.push(registry_id);
            issue(
                located(
                    paths.get(registry_id).expect("registry document must have a source path"),
                    "$.imports",
                ),
                format!("registry import cycle: {}", cycle.join(" -> ")),
                issues,
            );
            return;
        }
        visiting.push(registry_id);
        if let Some(document) = documents.get(registry_id) {
            for imported in &document.imports {
                if documents.contains_key(&imported.registry_id) {
                    visit(imported.registry_id.as_str(), documents, visiting, visited, paths, issues);
                }
            }
        }
        visiting.pop();
        visited.insert(registry_id);
    }

    let mut visited = BTreeSet::new();
    let mut visiting = Vec::with_capacity(documents.len());
    for registry_id in documents.keys() {
        visiting.clear();
        visit(registry_id, documents, &mut visiting, &mut visited, paths, issues);
    }
}

fn validate_semantics(
    documents: &BTreeMap<RawText, RegistryDocument>,
    paths: &BTreeMap<RawText, PathBuf>,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut component_owners = BTreeMap::new();
    for (registry_id, document) in documents {
        for component in &document.components {
            if let Some(existing) = component_owners.insert(component.id.as_str(), registry_id.as_str())
                && existing != registry_id.as_str()
            {
                issue(
                    located(
                        paths.get(registry_id).expect("registry document must have a source path"),
                        "$.components",
                    ),
                    format!(
                        "component ID '{}' exists in both '{}' and '{}'",
                        component.id, existing, registry_id
                    ),
                    issues,
                );
            }
        }
    }

    for (registry_id, document) in documents {
        let local_components = document
            .components
            .iter()
            .map(|component| (component.id.as_str(), component))
            .collect::<BTreeMap<_, _>>();
        for (component_index, component) in document.components.iter().enumerate() {
            let base = format!("$.components[{component_index}]");
            for (dependency_index, dependency) in component.dependencies.iter().enumerate() {
                let location = format!("{base}.dependencies[{dependency_index}]");
                let target_registry = dependency.registry_id.as_deref().unwrap_or(registry_id);
                if target_registry != registry_id.as_str() {
                    require(
                        document.imports.iter().any(|import| import.registry_id == target_registry),
                        paths.get(registry_id).expect("registry document must have a source path"),
                        &location,
                        &format!("unknown imported registry '{target_registry}'"),
                        issues,
                    );
                }
                require(
                    documents
                        .get(target_registry)
                        .is_some_and(|target| target.components.iter().any(|item| item.id == dependency.component_id)),
                    paths.get(registry_id).expect("registry document must have a source path"),
                    &location,
                    &format!(
                        "unknown dependency component '{}' in registry '{target_registry}'",
                        dependency.component_id
                    ),
                    issues,
                );
            }
            for (operation_index, operation) in component.operations.iter().enumerate() {
                let operation_location = format!("{base}.operations[{operation_index}]");
                for (index, input) in operation.inputs.iter().enumerate() {
                    resolve_type(
                        &input.value_type,
                        registry_id,
                        component,
                        &local_components,
                        documents,
                        paths.get(registry_id).expect("registry document must have a source path"),
                        &format!("{operation_location}.inputs[{index}].type"),
                        issues,
                    );
                }
                for (index, observation) in operation.observations.iter().enumerate() {
                    resolve_type(
                        &observation.value_type,
                        registry_id,
                        component,
                        &local_components,
                        documents,
                        paths.get(registry_id).expect("registry document must have a source path"),
                        &format!("{operation_location}.observations[{index}].type"),
                        issues,
                    );
                }
                if let Some(result) = &operation.outcomes.result {
                    resolve_type(
                        result,
                        registry_id,
                        component,
                        &local_components,
                        documents,
                        paths.get(registry_id).expect("registry document must have a source path"),
                        &format!("{operation_location}.outcomes.result"),
                        issues,
                    );
                }
                for (index, error) in operation.outcomes.errors.iter().enumerate() {
                    if let Some(value_type) = &error.value_type {
                        resolve_type(
                            value_type,
                            registry_id,
                            component,
                            &local_components,
                            documents,
                            paths.get(registry_id).expect("registry document must have a source path"),
                            &format!("{operation_location}.outcomes.errors[{index}].type"),
                            issues,
                        );
                    }
                }
            }
            for (index, named_type) in component.types.iter().enumerate() {
                resolve_type(
                    &named_type.as_type(),
                    registry_id,
                    component,
                    &local_components,
                    documents,
                    paths.get(registry_id).expect("registry document must have a source path"),
                    &format!("{base}.types[{index}]"),
                    issues,
                );
            }
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "recursive type resolution requires registry, component, path, and diagnostics context"
)]
fn resolve_type(
    value_type: &TypeRef,
    current_registry: &str,
    current_component: &RegistryComponent,
    local_components: &BTreeMap<&str, &RegistryComponent>,
    documents: &BTreeMap<RawText, RegistryDocument>,
    path: &Path,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    match value_type {
        TypeRef::Named {
            name,
            component_id,
            registry_id,
        } => {
            let target_registry = registry_id.as_deref().unwrap_or(current_registry);
            let target_component = component_id.as_deref().unwrap_or(&current_component.id);
            if target_registry != current_registry {
                require(
                    documents
                        .get(current_registry)
                        .is_some_and(|document| document.imports.iter().any(|import| import.registry_id == target_registry)),
                    path,
                    location,
                    &format!("unknown imported registry '{target_registry}'"),
                    issues,
                );
                require(
                    current_component.dependencies.iter().any(|dependency| {
                        dependency.registry_id.as_deref() == Some(target_registry) && dependency.component_id == target_component
                    }),
                    path,
                    location,
                    "missing matching component dependency",
                    issues,
                );
            } else if target_component != current_component.id.as_str() {
                require(
                    current_component
                        .dependencies
                        .iter()
                        .any(|dependency| dependency.registry_id.is_none() && dependency.component_id == target_component),
                    path,
                    location,
                    "missing matching local dependency",
                    issues,
                );
            }
            let target = documents
                .get(target_registry)
                .and_then(|document| document.components.iter().find(|component| component.id == target_component))
                .or_else(|| {
                    (target_registry == current_registry)
                        .then(|| local_components.get(target_component).copied())
                        .flatten()
                });
            let Some(target) = target else {
                issue(
                    located(path, location),
                    format!("unknown component '{target_component}' in registry '{target_registry}'"),
                    issues,
                );
                return;
            };
            require(
                target.types.iter().any(|item| item.name() == name),
                path,
                location,
                &format!("unknown named type '{name}'"),
                issues,
            );
        }
        TypeRef::Primitive { .. } => {}
        TypeRef::List { items } | TypeRef::Set { items } => resolve_type(
            items,
            current_registry,
            current_component,
            local_components,
            documents,
            path,
            &format!("{location}.items"),
            issues,
        ),
        TypeRef::Map { keys, values } => {
            resolve_type(
                keys,
                current_registry,
                current_component,
                local_components,
                documents,
                path,
                &format!("{location}.keys"),
                issues,
            );
            resolve_type(
                values,
                current_registry,
                current_component,
                local_components,
                documents,
                path,
                &format!("{location}.values"),
                issues,
            );
        }
        TypeRef::Tuple { items } => {
            for (index, item) in items.iter().enumerate() {
                resolve_type(
                    item,
                    current_registry,
                    current_component,
                    local_components,
                    documents,
                    path,
                    &format!("{location}.items[{index}]"),
                    issues,
                );
            }
        }
        TypeRef::Record { fields } => {
            for (index, field) in fields.iter().enumerate() {
                resolve_type(
                    &field.value_type,
                    current_registry,
                    current_component,
                    local_components,
                    documents,
                    path,
                    &format!("{location}.fields[{index}].type"),
                    issues,
                );
            }
        }
        TypeRef::Optional { value } => resolve_type(
            value,
            current_registry,
            current_component,
            local_components,
            documents,
            path,
            &format!("{location}.value"),
            issues,
        ),
        TypeRef::TaggedUnion { variants } => {
            for (index, variant) in variants.iter().enumerate() {
                if let Some(payload) = &variant.payload {
                    resolve_type(
                        payload,
                        current_registry,
                        current_component,
                        local_components,
                        documents,
                        path,
                        &format!("{location}.variants[{index}].payload"),
                        issues,
                    );
                }
            }
        }
    }
}

fn canonicalize(path: &Path, filesystem: &Filesystem<'_>) -> PathBuf {
    filesystem.canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn unique<'a>(values: impl Iterator<Item = &'a str>, path: &Path, location: &str, field: &str, issues: &mut Vec<ValidationIssue>) {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            issue(located(path, location), format!("duplicate {field} '{value}'"), issues);
        }
    }
}

fn require(condition: bool, path: &Path, location: &str, message: &str, issues: &mut Vec<ValidationIssue>) {
    if !condition {
        issue(located(path, location), message, issues);
    }
}

fn validate_extensions(extensions: &BTreeMap<String, Value>, path: &Path, location: &str, issues: &mut Vec<ValidationIssue>) {
    for key in extensions.keys() {
        let mut segments = key.split('.');
        let first = segments.next().unwrap_or_default();
        let remaining = segments.collect::<Vec<_>>();
        let valid_first = first.bytes().enumerate().all(|(index, byte)| {
            (index == 0 && byte.is_ascii_lowercase()) || (index > 0 && (byte.is_ascii_lowercase() || byte.is_ascii_digit()))
        });
        let valid_remaining = !remaining.is_empty()
            && remaining.iter().all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-'))
            });
        if first.is_empty() || !valid_first || !valid_remaining {
            issue(
                located(path, location),
                format!("extension key '{key}' must use a lowercase dotted namespace"),
                issues,
            );
        }
        if key.starts_with(RESERVED_NAMESPACE) {
            issue(
                located(path, location),
                format!("extension key '{key}' must use a namespace outside conformance.*"),
                issues,
            );
        }
    }
}

#[cfg(test)]
mod uri_tests {
    use super::{Filesystem, resolve_uri};
    use std::path::{Path, PathBuf};

    fn resolve(base: &Path, uri: &str) -> Result<Option<PathBuf>, super::uri::Error> {
        resolve_uri(base, uri, &Filesystem::real())
    }

    #[test]
    fn non_file_hint() {
        assert_eq!(
            resolve(Path::new("root.json"), "https://example.test/import.json").expect("valid URI"),
            None
        );
    }

    #[test]
    fn relative_file_uri() {
        assert_eq!(
            resolve(Path::new("registries/root.json"), "file:dependency%20registry.json").expect("valid file URI"),
            Some(PathBuf::from("registries/dependency registry.json"))
        );
    }

    #[test]
    fn absolute_file_uri() {
        #[cfg(windows)]
        let cases = [
            (
                "file:///C:/registry%20files/import.json",
                PathBuf::from(r"C:\registry files\import.json"),
            ),
            (
                "file://localhost/C:/registry%20files/import.json",
                PathBuf::from(r"C:\registry files\import.json"),
            ),
        ];
        #[cfg(not(windows))]
        let cases = [
            ("file:///registry%20files/import.json", PathBuf::from("/registry files/import.json")),
            (
                "file://localhost/registry%20files/import.json",
                PathBuf::from("/registry files/import.json"),
            ),
        ];

        for (uri, expected) in cases {
            assert_eq!(
                resolve(Path::new("root.json"), uri).expect("valid file URI"),
                Some(expected),
                "{uri}"
            );
        }
    }

    #[cfg(test)]
    mod loading_tests {
        use crate::comparison::{DocumentReader, LoadError};
        use crate::validation::registry::load_reader;
        use crate::validation::{Loaded, RegistrySet};
        use std::path::{Path, PathBuf};

        struct MemoryReader {
            bytes: Vec<u8>,
        }

        impl DocumentReader for MemoryReader {
            fn read(&self, _path: &Path) -> Result<Vec<u8>, LoadError> {
                Ok(self.bytes.clone())
            }

            fn canonicalize(&self, path: &Path) -> Result<PathBuf, LoadError> {
                Ok(path.to_path_buf())
            }
        }

        fn load(json: &str) -> Loaded<RegistrySet> {
            load_reader(
                Path::new("registry.ctsc.json"),
                &[] as &[&Path],
                &MemoryReader {
                    bytes: json.as_bytes().to_vec(),
                },
            )
        }

        #[test]
        fn injected_reader_loads_a_valid_registry() {
            let loaded = load(
                r#"{"format":"ctsc.registry","formatVersion":"0.3.0","registryId":"urn:ctsc:registry:test","version":"0.1.0","components":[{"id":"test.component","operations":[{"name":"run","inputs":[],"observations":[],"outcomes":{}}],"types":[]}]}"#,
            );

            assert!(loaded.issues.is_empty());
            let registry = loaded.value.expect("valid registry");
            assert_eq!(registry.root_id.as_str(), "urn:ctsc:registry:test");
            assert!(registry.components.contains_key("test.component"));
        }

        #[test]
        fn semantic_validation_reports_unknown_named_types() {
            let loaded = load(
                r#"{"format":"ctsc.registry","formatVersion":"0.3.0","registryId":"urn:ctsc:registry:test","version":"0.1.0","components":[{"id":"test.component","operations":[{"name":"run","inputs":[],"observations":[],"outcomes":{"result":{"kind":"named","name":"Missing"}}}],"types":[]}]}"#,
            );

            assert!(loaded.value.is_some());
            assert_eq!(loaded.issues.len(), 1);
            assert_eq!(
                loaded.issues[0].location,
                "registry.ctsc.json:$.components[0].operations[0].outcomes.result"
            );
            assert_eq!(loaded.issues[0].message, "unknown named type 'Missing'");
        }
    }

    #[test]
    fn uri_diagnostics() {
        for uri in ["file:import.json?version=1", "file:import.json#section"] {
            let error = resolve(Path::new("root.json"), uri).expect_err("query or fragment");
            assert!(error.to_string().contains("must not contain a query or fragment"), "{error}");
        }

        let error = resolve(Path::new("root.json"), "file://registry-host/share/import.json").expect_err("remote authority");
        assert!(error.to_string().contains("network file URI"), "{error}");
        assert!(error.to_string().contains("--import"), "{error}");

        for uri in [
            "file:////registry-host/share/import.json",
            "file:%5C%5Cregistry-host%5Cshare%5Cimport.json",
        ] {
            let error = resolve(Path::new("root.json"), uri).expect_err("UNC path");
            assert!(error.to_string().contains("UNC or network file URI"), "{error}");
            assert!(error.to_string().contains("--import"), "{error}");
        }
    }

    #[test]
    fn invalid_uri() {
        let error = resolve(Path::new("root.json"), "file://[invalid/import.json").expect_err("invalid URI");
        assert!(error.to_string().starts_with("invalid file URI"), "{error}");

        let error = resolve(Path::new("root.json"), "file:import-%FF.json").expect_err("non-UTF-8 path");
        assert!(error.to_string().contains("file URI path is not UTF-8"), "{error}");
    }

    #[test]
    fn read_failure() {
        use super::load_set;

        let missing = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/missing-registry.json");
        let loaded = load_set(&missing, &[] as &[&Path]);
        assert!(loaded.value.is_none());
        assert_eq!(loaded.issues.len(), 1);
        assert_eq!(loaded.issues[0].location, format!("{}:$", missing.display()));
        assert!(loaded.issues[0].message.starts_with("failed to read file:"));
    }
}
