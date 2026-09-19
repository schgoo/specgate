use super::model::{
    CTSC_VERSION, NamedType, RegistryComponent, RegistryDocument, RegistryOperation, RegistrySet, ResolvedComponent, TypeRef,
};
use super::{Loaded, ValidationIssue, is_digest, issue, located, read_bytes, sha256_digest};
use percent_encoding::percent_decode_str;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use url::Url;

const REGISTRY_FORMAT: &str = "ctsc.registry";

#[derive(Debug)]
struct RegistryFile {
    path: PathBuf,
    digest: String,
    document: RegistryDocument,
}

pub(crate) fn load_registry_set(root: &Path, explicit_imports: &[PathBuf]) -> Loaded<RegistrySet> {
    let mut issues = Vec::new();
    let root = canonical_or_original(root);
    let mut inputs = vec![root.clone()];
    inputs.extend(explicit_imports.iter().map(|path| canonical_or_original(path)));
    let mut explicit_seen = BTreeSet::new();
    for path in &inputs {
        if !explicit_seen.insert(path.clone()) {
            issue(&mut issues, located(path, "$"), "duplicate registry input path");
        }
    }

    let mut files_by_path = BTreeMap::<PathBuf, RegistryFile>::new();
    let mut paths_by_id = BTreeMap::<String, Vec<PathBuf>>::new();
    for path in inputs {
        if files_by_path.contains_key(&path) {
            continue;
        }
        if let Some(file) = load_registry_file(&path, true, &mut issues) {
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
        let document = files_by_path[&path].document.clone();
        for (index, import) in document.imports.iter().enumerate() {
            let location = format!("$.imports[{index}]");
            let used = registry_import_is_used(&document, &import.registry_id);
            if let Some(uri) = import.uri.as_deref() {
                match resolve_file_uri(&path, uri) {
                    Ok(Some(candidate_path)) if !files_by_path.contains_key(&candidate_path) => {
                        if let Some(file) = load_registry_file(&candidate_path, false, &mut issues) {
                            insert_candidate(file, &mut files_by_path, &mut paths_by_id, &mut issues);
                        }
                    }
                    Ok(_) => {}
                    Err(message) => issue(&mut issues, located(&path, &format!("{location}.uri")), message),
                }
            }
            let candidates = paths_by_id.get(&import.registry_id).cloned().unwrap_or_default();
            let Some(candidate_path) = candidates.first() else {
                if used {
                    issue(
                        &mut issues,
                        located(&path, &location),
                        format!("unresolved import '{}'; supply --import or a local file: URI", import.registry_id),
                    );
                }
                continue;
            };
            if candidates.len() != 1 {
                issue(
                    &mut issues,
                    located(&path, &location),
                    format!("import '{}' has ambiguous candidate registry files", import.registry_id),
                );
                continue;
            }
            let candidate = &files_by_path[candidate_path];
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
            let file = &files_by_path[path];
            (file.document.registry_id.clone(), file.document.clone())
        })
        .collect::<BTreeMap<_, _>>();
    let document_paths = selected_paths
        .iter()
        .map(|path| {
            let file = &files_by_path[path];
            (file.document.registry_id.clone(), file.path.clone())
        })
        .collect::<BTreeMap<_, _>>();
    validate_import_cycles(&documents, &document_paths, &mut issues);
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
                    &mut issues,
                    located(&root, "$.components"),
                    format!(
                        "component ID '{}' exists in both '{}' and '{}'",
                        component.id, existing.registry_id, registry_id
                    ),
                );
            }
        }
    }
    let root_file = &files_by_path[&root];
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

fn load_registry_file(path: &Path, report_read_error: bool, issues: &mut Vec<ValidationIssue>) -> Option<RegistryFile> {
    let bytes = if report_read_error {
        read_bytes(path, issues)?
    } else {
        match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) => return None,
        }
    };
    let document = match serde_json::from_slice::<RegistryDocument>(&bytes) {
        Ok(document) => document,
        Err(error) => {
            issue(issues, located(path, "$"), format!("invalid CTSC registry JSON: {error}"));
            return None;
        }
    };
    validate_shape(&document, path, issues);
    Some(RegistryFile {
        path: path.to_path_buf(),
        digest: sha256_digest(&bytes),
        document,
    })
}

fn insert_candidate(
    file: RegistryFile,
    files_by_path: &mut BTreeMap<PathBuf, RegistryFile>,
    paths_by_id: &mut BTreeMap<String, Vec<PathBuf>>,
    issues: &mut Vec<ValidationIssue>,
) {
    let paths = paths_by_id.entry(file.document.registry_id.clone()).or_default();
    if let Some(existing) = paths.first() {
        issue(
            issues,
            located(&file.path, "$.registryId"),
            format!(
                "registry ID '{}' has ambiguous candidates {} and {}",
                file.document.registry_id,
                existing.display(),
                file.path.display()
            ),
        );
    }
    paths.push(file.path.clone());
    files_by_path.insert(file.path.clone(), file);
}

fn validate_import_cycles(
    documents: &BTreeMap<String, RegistryDocument>,
    paths: &BTreeMap<String, PathBuf>,
    issues: &mut Vec<ValidationIssue>,
) {
    fn visit<'a>(
        registry_id: &'a str,
        documents: &'a BTreeMap<String, RegistryDocument>,
        visiting: &mut Vec<&'a str>,
        visited: &mut BTreeSet<&'a str>,
        paths: &BTreeMap<String, PathBuf>,
        issues: &mut Vec<ValidationIssue>,
    ) {
        if visited.contains(registry_id) {
            return;
        }
        if let Some(position) = visiting.iter().position(|item| *item == registry_id) {
            let mut cycle = visiting[position..].to_vec();
            cycle.push(registry_id);
            issue(
                issues,
                located(&paths[registry_id], "$.imports"),
                format!("registry import cycle: {}", cycle.join(" -> ")),
            );
            return;
        }
        visiting.push(registry_id);
        if let Some(document) = documents.get(registry_id) {
            for imported in &document.imports {
                if documents.contains_key(&imported.registry_id) {
                    visit(&imported.registry_id, documents, visiting, visited, paths, issues);
                }
            }
        }
        visiting.pop();
        visited.insert(registry_id);
    }

    let mut visited = BTreeSet::new();
    for registry_id in documents.keys() {
        visit(registry_id, documents, &mut Vec::new(), &mut visited, paths, issues);
    }
}

fn validate_shape(document: &RegistryDocument, path: &Path, issues: &mut Vec<ValidationIssue>) {
    require(
        document.format == REGISTRY_FORMAT,
        path,
        "$.format",
        "format must be 'ctsc.registry'",
        issues,
    );
    require(
        document.format_version == CTSC_VERSION,
        path,
        "$.formatVersion",
        "formatVersion must be '0.2.0'",
        issues,
    );
    require(
        !document.registry_id.is_empty(),
        path,
        "$.registryId",
        "registryId must not be empty",
        issues,
    );
    require(!document.version.is_empty(), path, "$.version", "version must not be empty", issues);
    require(
        !document.components.is_empty(),
        path,
        "$.components",
        "components must contain at least one component",
        issues,
    );
    validate_extensions(&document.extensions, path, "$.extensions", issues);

    unique(
        document.imports.iter().map(|item| item.registry_id.as_str()),
        path,
        "$.imports",
        "registryId",
        issues,
    );
    for (index, import) in document.imports.iter().enumerate() {
        let location = format!("$.imports[{index}]");
        require(
            !import.registry_id.is_empty(),
            path,
            &format!("{location}.registryId"),
            "registryId must not be empty",
            issues,
        );
        require(
            !import.version.is_empty(),
            path,
            &format!("{location}.version"),
            "version must not be empty",
            issues,
        );
        require(
            is_digest(&import.digest),
            path,
            &format!("{location}.digest"),
            "digest must be 'sha256:' followed by 64 lowercase hexadecimal characters",
            issues,
        );
        if import.uri.as_ref().is_some_and(String::is_empty) {
            issue(&mut *issues, located(path, &format!("{location}.uri")), "uri must not be empty");
        }
    }

    unique(
        document.components.iter().map(|item| item.id.as_str()),
        path,
        "$.components",
        "id",
        issues,
    );
    for (component_index, component) in document.components.iter().enumerate() {
        let location = format!("$.components[{component_index}]");
        require(
            !component.id.is_empty(),
            path,
            &format!("{location}.id"),
            "component id must not be empty",
            issues,
        );
        unique(
            component.dependencies.iter().map(|item| item.component_id.as_str()),
            path,
            &format!("{location}.dependencies"),
            "componentId",
            issues,
        );
        for (dependency_index, dependency) in component.dependencies.iter().enumerate() {
            let dependency_location = format!("{location}.dependencies[{dependency_index}]");
            require(
                !dependency.component_id.is_empty(),
                path,
                &format!("{dependency_location}.componentId"),
                "componentId must not be empty",
                issues,
            );
            if dependency.registry_id.as_ref().is_some_and(String::is_empty) {
                issue(
                    issues,
                    located(path, &format!("{dependency_location}.registryId")),
                    "registryId must not be empty",
                );
            }
        }
        validate_extensions(&component.extensions, path, &format!("{location}.extensions"), issues);
        unique(
            component.operations.iter().map(|item| item.name.as_str()),
            path,
            &format!("{location}.operations"),
            "name",
            issues,
        );
        unique(
            component.types.iter().map(NamedType::name),
            path,
            &format!("{location}.types"),
            "name",
            issues,
        );
        for (operation_index, operation) in component.operations.iter().enumerate() {
            validate_operation(operation, path, &format!("{location}.operations[{operation_index}]"), issues);
        }
        for (type_index, named_type) in component.types.iter().enumerate() {
            validate_named_type(named_type, path, &format!("{location}.types[{type_index}]"), issues);
        }
    }
}

fn validate_operation(operation: &RegistryOperation, path: &Path, location: &str, issues: &mut Vec<ValidationIssue>) {
    require(
        !operation.name.is_empty(),
        path,
        &format!("{location}.name"),
        "operation name must not be empty",
        issues,
    );
    validate_extensions(&operation.extensions, path, &format!("{location}.extensions"), issues);
    for (label, values) in [
        ("inputs", operation.inputs.as_slice()),
        ("observations", operation.observations.as_slice()),
    ] {
        unique(
            values.iter().map(|item| item.name.as_str()),
            path,
            &format!("{location}.{label}"),
            "name",
            issues,
        );
        for (index, value) in values.iter().enumerate() {
            require(
                !value.name.is_empty(),
                path,
                &format!("{location}.{label}[{index}].name"),
                "name must not be empty",
                issues,
            );
            validate_type_shape(&value.value_type, path, &format!("{location}.{label}[{index}].type"), issues);
        }
    }
    unique(
        operation.outcomes.errors.iter().map(|item| item.name.as_str()),
        path,
        &format!("{location}.outcomes.errors"),
        "name",
        issues,
    );
    if operation.outcomes.empty && operation.outcomes.result.is_none() {
        issue(
            issues,
            located(path, &format!("{location}.outcomes.empty")),
            "empty: true requires a result outcome",
        );
    }
    if let Some(result) = &operation.outcomes.result {
        if matches!(result, TypeRef::Primitive { name } if name == "unit") {
            issue(
                issues,
                located(path, &format!("{location}.outcomes.result")),
                "result type must not be primitive unit",
            );
        }
        validate_type_shape(result, path, &format!("{location}.outcomes.result"), issues);
    }
    for (index, error) in operation.outcomes.errors.iter().enumerate() {
        require(
            !error.name.is_empty(),
            path,
            &format!("{location}.outcomes.errors[{index}].name"),
            "error name must not be empty",
            issues,
        );
        if let Some(value_type) = &error.value_type {
            validate_type_shape(value_type, path, &format!("{location}.outcomes.errors[{index}].type"), issues);
        }
    }
}

fn validate_named_type(named_type: &NamedType, path: &Path, location: &str, issues: &mut Vec<ValidationIssue>) {
    require(
        !named_type.name().is_empty(),
        path,
        &format!("{location}.name"),
        "type name must not be empty",
        issues,
    );
    match named_type {
        NamedType::Record { fields, extensions, .. } => {
            validate_extensions(extensions, path, &format!("{location}.extensions"), issues);
            validate_named_values(fields, path, &format!("{location}.fields"), issues);
        }
        NamedType::TaggedUnion { variants, extensions, .. } => {
            validate_extensions(extensions, path, &format!("{location}.extensions"), issues);
            require(
                !variants.is_empty(),
                path,
                &format!("{location}.variants"),
                "tagged union must contain at least one variant",
                issues,
            );
            validate_variants(variants, path, &format!("{location}.variants"), issues);
        }
    }
}

fn validate_named_values(values: &[super::model::NamedValue], path: &Path, location: &str, issues: &mut Vec<ValidationIssue>) {
    unique(values.iter().map(|item| item.name.as_str()), path, location, "name", issues);
    for (index, value) in values.iter().enumerate() {
        require(
            !value.name.is_empty(),
            path,
            &format!("{location}[{index}].name"),
            "name must not be empty",
            issues,
        );
        validate_type_shape(&value.value_type, path, &format!("{location}[{index}].type"), issues);
    }
}

fn validate_variants(variants: &[super::model::Variant], path: &Path, location: &str, issues: &mut Vec<ValidationIssue>) {
    unique(variants.iter().map(|item| item.name.as_str()), path, location, "name", issues);
    for (index, variant) in variants.iter().enumerate() {
        require(
            !variant.name.is_empty(),
            path,
            &format!("{location}[{index}].name"),
            "name must not be empty",
            issues,
        );
        if let Some(payload) = &variant.payload {
            validate_type_shape(payload, path, &format!("{location}[{index}].payload"), issues);
        }
    }
}

fn validate_type_shape(value_type: &TypeRef, path: &Path, location: &str, issues: &mut Vec<ValidationIssue>) {
    match value_type {
        TypeRef::Primitive { name } => {
            const PRIMITIVES: [&str; 10] = ["unit", "string", "bool", "i32", "i64", "u32", "u64", "f32", "f64", "bytes"];
            require(
                PRIMITIVES.contains(&name.as_str()),
                path,
                &format!("{location}.name"),
                "unknown primitive type",
                issues,
            );
        }
        TypeRef::Named {
            name,
            component_id,
            registry_id,
        } => {
            require(
                !name.is_empty(),
                path,
                &format!("{location}.name"),
                "type name must not be empty",
                issues,
            );
            if component_id.as_ref().is_some_and(String::is_empty) {
                issue(
                    issues,
                    located(path, &format!("{location}.componentId")),
                    "componentId must not be empty",
                );
            }
            if registry_id.as_ref().is_some_and(String::is_empty) {
                issue(
                    issues,
                    located(path, &format!("{location}.registryId")),
                    "registryId must not be empty",
                );
            }
            if registry_id.is_some() && component_id.is_none() {
                issue(
                    issues,
                    located(path, location),
                    "registryId requires componentId on a named type reference",
                );
            }
        }
        TypeRef::List { items } | TypeRef::Set { items } => {
            validate_type_shape(items, path, &format!("{location}.items"), issues);
        }
        TypeRef::Map { keys, values } => {
            validate_type_shape(keys, path, &format!("{location}.keys"), issues);
            validate_type_shape(values, path, &format!("{location}.values"), issues);
        }
        TypeRef::Tuple { items } => {
            for (index, item) in items.iter().enumerate() {
                validate_type_shape(item, path, &format!("{location}.items[{index}]"), issues);
            }
        }
        TypeRef::Record { fields } => validate_named_values(fields, path, &format!("{location}.fields"), issues),
        TypeRef::Optional { value } => validate_type_shape(value, path, &format!("{location}.value"), issues),
        TypeRef::TaggedUnion { variants } => {
            require(
                !variants.is_empty(),
                path,
                &format!("{location}.variants"),
                "tagged union must contain at least one variant",
                issues,
            );
            validate_variants(variants, path, &format!("{location}.variants"), issues);
        }
    }
}

fn registry_import_is_used(document: &RegistryDocument, registry_id: &str) -> bool {
    document.components.iter().any(|component| {
        component
            .dependencies
            .iter()
            .any(|dependency| dependency.registry_id.as_deref() == Some(registry_id))
            || component.operations.iter().any(|operation| {
                operation
                    .inputs
                    .iter()
                    .chain(&operation.observations)
                    .any(|value| type_uses_registry(&value.value_type, registry_id))
                    || operation
                        .outcomes
                        .result
                        .as_ref()
                        .is_some_and(|value| type_uses_registry(value, registry_id))
                    || operation
                        .outcomes
                        .errors
                        .iter()
                        .filter_map(|error| error.value_type.as_ref())
                        .any(|value| type_uses_registry(value, registry_id))
            })
            || component
                .types
                .iter()
                .any(|named_type| type_uses_registry(&named_type.as_type(), registry_id))
    })
}

fn type_uses_registry(value_type: &TypeRef, registry_id: &str) -> bool {
    match value_type {
        TypeRef::Named {
            registry_id: referenced, ..
        } => referenced.as_deref() == Some(registry_id),
        TypeRef::List { items } | TypeRef::Set { items } => type_uses_registry(items, registry_id),
        TypeRef::Map { keys, values } => type_uses_registry(keys, registry_id) || type_uses_registry(values, registry_id),
        TypeRef::Tuple { items } => items.iter().any(|item| type_uses_registry(item, registry_id)),
        TypeRef::Record { fields } => fields.iter().any(|field| type_uses_registry(&field.value_type, registry_id)),
        TypeRef::Optional { value } => type_uses_registry(value, registry_id),
        TypeRef::TaggedUnion { variants } => variants
            .iter()
            .filter_map(|variant| variant.payload.as_ref())
            .any(|payload| type_uses_registry(payload, registry_id)),
        TypeRef::Primitive { .. } => false,
    }
}

fn validate_semantics(
    documents: &BTreeMap<String, RegistryDocument>,
    paths: &BTreeMap<String, PathBuf>,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut component_owners = BTreeMap::new();
    for (registry_id, document) in documents {
        for component in &document.components {
            if let Some(existing) = component_owners.insert(component.id.as_str(), registry_id.as_str())
                && existing != registry_id
            {
                issue(
                    issues,
                    located(&paths[registry_id], "$.components"),
                    format!(
                        "component ID '{}' exists in both '{}' and '{}'",
                        component.id, existing, registry_id
                    ),
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
                if target_registry != registry_id {
                    require(
                        document.imports.iter().any(|import| import.registry_id == target_registry),
                        &paths[registry_id],
                        &location,
                        &format!("unknown imported registry '{target_registry}'"),
                        issues,
                    );
                }
                require(
                    documents
                        .get(target_registry)
                        .is_some_and(|target| target.components.iter().any(|item| item.id == dependency.component_id)),
                    &paths[registry_id],
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
                        &paths[registry_id],
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
                        &paths[registry_id],
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
                        &paths[registry_id],
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
                            &paths[registry_id],
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
                    &paths[registry_id],
                    &format!("{base}.types[{index}]"),
                    issues,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_type(
    value_type: &TypeRef,
    current_registry: &str,
    current_component: &RegistryComponent,
    local_components: &BTreeMap<&str, &RegistryComponent>,
    documents: &BTreeMap<String, RegistryDocument>,
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
            } else if target_component != current_component.id {
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
                    issues,
                    located(path, location),
                    format!("unknown component '{target_component}' in registry '{target_registry}'"),
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

fn resolve_file_uri(base: &Path, uri: &str) -> Result<Option<PathBuf>, String> {
    let Some(raw) = uri.strip_prefix("file:") else {
        return Ok(None);
    };
    let parsed = Url::parse(uri).map_err(|error| format!("invalid file URI '{uri}': {error}"))?;
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(format!("file URI must not contain a query or fragment: '{uri}'"));
    }
    if parsed
        .host_str()
        .is_some_and(|authority| !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost"))
    {
        return Err(format!(
            "automatic registry import resolution does not support network file URI '{uri}'; download the registry and supply its local path with --import"
        ));
    }
    let decoded_path = percent_decode_str(parsed.path())
        .decode_utf8()
        .map_err(|error| format!("file URI path is not UTF-8: {error}"))?;
    if decoded_path.starts_with("//") || decoded_path.starts_with(r"\\") {
        return Err(format!(
            "automatic registry import resolution does not support UNC or network file URI '{uri}'; download the registry and supply its local path with --import"
        ));
    }
    let candidate = if raw.starts_with('/') || raw.starts_with("//") {
        parsed
            .to_file_path()
            .map_err(|()| format!("file URI cannot be converted to a local path: '{uri}'"))?
    } else {
        let decoded = percent_decode_str(raw)
            .decode_utf8()
            .map_err(|error| format!("file URI path is not UTF-8: {error}"))?;
        PathBuf::from(decoded.as_ref())
    };
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        base.parent().unwrap_or_else(|| Path::new(".")).join(candidate)
    };
    Ok(Some(canonical_or_original(&candidate)))
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn unique<'a>(values: impl Iterator<Item = &'a str>, path: &Path, location: &str, field: &str, issues: &mut Vec<ValidationIssue>) {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            issue(issues, located(path, location), format!("duplicate {field} '{value}'"));
        }
    }
}

fn require(condition: bool, path: &Path, location: &str, message: &str, issues: &mut Vec<ValidationIssue>) {
    if !condition {
        issue(issues, located(path, location), message);
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
                issues,
                located(path, location),
                format!("extension key '{key}' must use a lowercase dotted namespace"),
            );
        }
        if key.starts_with("conformance.") {
            issue(
                issues,
                located(path, location),
                format!("extension key '{key}' must use a namespace outside conformance.*"),
            );
        }
    }
}
