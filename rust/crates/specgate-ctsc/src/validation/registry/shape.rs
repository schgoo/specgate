use std::fmt::Write as _;

use super::{
    CTSC_VERSION, NamedType, Path, REGISTRY_FORMAT, RegistryDocument, RegistryOperation, TypeRef, ValidationIssue, is_digest, issue,
    located, require, unique, validate_extensions,
};

pub(super) fn validate_shape(document: &RegistryDocument, path: impl AsRef<Path>, issues: &mut Vec<ValidationIssue>) {
    let path = path.as_ref();
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
        &format!("formatVersion must be '{CTSC_VERSION}'"),
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
    let mut location = String::with_capacity("$.imports[].registryId".len() + usize::MAX.ilog10() as usize + 1);
    for (index, import) in document.imports.iter().enumerate() {
        location.clear();
        write!(location, "$.imports[{index}]").expect("writing to a String cannot fail");
        let base_length = location.len();
        location.push_str(".registryId");
        require(
            !import.registry_id.is_empty(),
            path,
            &location,
            "registryId must not be empty",
            issues,
        );
        location.truncate(base_length);
        location.push_str(".version");
        require(!import.version.is_empty(), path, &location, "version must not be empty", issues);
        location.truncate(base_length);
        location.push_str(".digest");
        require(
            is_digest(import.digest.as_str()),
            path,
            &location,
            "digest must be 'sha256:' followed by 64 lowercase hexadecimal characters",
            issues,
        );
        if import.uri.as_ref().is_some_and(String::is_empty) {
            location.truncate(base_length);
            location.push_str(".uri");
            issue(located(path, &location), "uri must not be empty", &mut *issues);
        }
    }

    unique(
        document.components.iter().map(|item| item.id.as_str()),
        path,
        "$.components",
        "id",
        issues,
    );
    // Registry 0.3 Ã‚Â§3.1 orders components by ascending id, compared as a
    // sequence of Unicode code points. Rust `str` comparison is byte-wise over
    // UTF-8, and UTF-8 byte order is identical to code-point order, so this is
    // exactly the comparison the contract specifies.
    if let Some(out_of_order) = document
        .components
        .windows(2)
        .find(|pair| pair[0].id > pair[1].id)
        .map(|pair| pair[1].id.as_str())
    {
        issue(
            located(path, "$.components"),
            format!("component id '{out_of_order}' is out of ascending id order"),
            &mut *issues,
        );
    }
    let mut component_location = String::with_capacity("$.components[]".len() + usize::MAX.ilog10() as usize + 1);
    let mut child_location =
        String::with_capacity(component_location.capacity() + ".dependencies[]".len() + usize::MAX.ilog10() as usize + 1);
    let mut value_location = String::with_capacity(child_location.capacity() + ".observations[].type".len());
    for (component_index, component) in document.components.iter().enumerate() {
        component_location.clear();
        write!(component_location, "$.components[{component_index}]").expect("writing to a String cannot fail");
        let location = component_location.as_str();
        child_location.clear();
        write!(child_location, "{location}.id").expect("writing to a String cannot fail");
        require(
            !component.id.is_empty(),
            path,
            &child_location,
            "component id must not be empty",
            issues,
        );
        child_location.clear();
        write!(child_location, "{location}.dependencies").expect("writing to a String cannot fail");
        unique(
            component.dependencies.iter().map(|item| item.component_id.as_str()),
            path,
            &child_location,
            "componentId",
            issues,
        );
        for (dependency_index, dependency) in component.dependencies.iter().enumerate() {
            child_location.clear();
            write!(child_location, "{location}.dependencies[{dependency_index}]").expect("writing to a String cannot fail");
            let dependency_length = child_location.len();
            child_location.push_str(".componentId");
            require(
                !dependency.component_id.is_empty(),
                path,
                &child_location,
                "componentId must not be empty",
                issues,
            );
            if dependency.registry_id.as_ref().is_some_and(|value| value.is_empty()) {
                child_location.truncate(dependency_length);
                child_location.push_str(".registryId");
                issue(located(path, &child_location), "registryId must not be empty", issues);
            }
        }
        child_location.clear();
        write!(child_location, "{location}.extensions").expect("writing to a String cannot fail");
        validate_extensions(&component.extensions, path, &child_location, issues);
        child_location.clear();
        write!(child_location, "{location}.operations").expect("writing to a String cannot fail");
        unique(
            component.operations.iter().map(|item| item.name.as_str()),
            path,
            &child_location,
            "name",
            issues,
        );
        child_location.clear();
        write!(child_location, "{location}.types").expect("writing to a String cannot fail");
        unique(component.types.iter().map(NamedType::name), path, &child_location, "name", issues);
        for (operation_index, operation) in component.operations.iter().enumerate() {
            child_location.clear();
            write!(child_location, "{location}.operations[{operation_index}]").expect("writing to a String cannot fail");
            validate_operation(operation, path, child_location.as_str(), &mut value_location, issues);
        }
        for (type_index, named_type) in component.types.iter().enumerate() {
            child_location.clear();
            write!(child_location, "{location}.types[{type_index}]").expect("writing to a String cannot fail");
            validate_named(named_type, path, child_location.as_str(), issues);
        }
    }
}

pub(super) fn validate_operation(
    operation: &RegistryOperation,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    value_location: &mut String,
    issues: &mut Vec<ValidationIssue>,
) {
    let path = path.as_ref();
    let location = location.as_ref();
    value_location.clear();
    write!(value_location, "{location}.name").expect("writing to a String cannot fail");
    require(
        !operation.name.is_empty(),
        path,
        value_location.as_str(),
        "operation name must not be empty",
        issues,
    );
    value_location.clear();
    write!(value_location, "{location}.extensions").expect("writing to a String cannot fail");
    validate_extensions(&operation.extensions, path, value_location.as_str(), issues);
    for (label, values) in [
        ("inputs", operation.inputs.as_slice()),
        ("observations", operation.observations.as_slice()),
    ] {
        value_location.clear();
        write!(value_location, "{location}.{label}").expect("writing to a String cannot fail");
        unique(
            values.iter().map(|item| item.name.as_str()),
            path,
            value_location.as_str(),
            "name",
            issues,
        );
        let collection_length = value_location.len();
        for (index, value) in values.iter().enumerate() {
            value_location.truncate(collection_length);
            write!(value_location, "[{index}]").expect("writing to a String cannot fail");
            let item_length = value_location.len();
            value_location.push_str(".name");
            require(
                !value.name.is_empty(),
                path,
                value_location.as_str(),
                "name must not be empty",
                issues,
            );
            value_location.truncate(item_length);
            value_location.push_str(".type");
            validate_type(&value.value_type, path, value_location, issues);
        }
    }
    value_location.clear();
    write!(value_location, "{location}.outcomes.errors").expect("writing to a String cannot fail");
    unique(
        operation.outcomes.errors.iter().map(|item| item.name.as_str()),
        path,
        value_location.as_str(),
        "name",
        issues,
    );
    if operation.outcomes.empty && operation.outcomes.result.is_none() {
        value_location.clear();
        write!(value_location, "{location}.outcomes.empty").expect("writing to a String cannot fail");
        issue(
            located(path, value_location.as_str()),
            "empty: true requires a result outcome",
            issues,
        );
    }
    if let Some(result) = &operation.outcomes.result {
        value_location.clear();
        write!(value_location, "{location}.outcomes.result").expect("writing to a String cannot fail");
        if matches!(result, TypeRef::Primitive { name } if name == "unit") {
            issue(
                located(path, value_location.as_str()),
                "result type must not be primitive unit",
                issues,
            );
        }
        validate_type(result, path, value_location, issues);
    }
    for (index, error) in operation.outcomes.errors.iter().enumerate() {
        value_location.clear();
        write!(value_location, "{location}.outcomes.errors[{index}]").expect("writing to a String cannot fail");
        let error_length = value_location.len();
        value_location.push_str(".name");
        require(
            !error.name.is_empty(),
            path,
            value_location.as_str(),
            "error name must not be empty",
            issues,
        );
        if let Some(value_type) = &error.value_type {
            value_location.truncate(error_length);
            value_location.push_str(".type");
            validate_type(value_type, path, value_location, issues);
        }
    }
}

pub(super) fn validate_named(named_type: &NamedType, path: impl AsRef<Path>, location: impl AsRef<str>, issues: &mut Vec<ValidationIssue>) {
    let path = path.as_ref();
    let location = location.as_ref();
    let mut child_location = String::with_capacity(location.len() + ".extensions".len());
    write!(child_location, "{location}.name").expect("writing to a String cannot fail");
    require(
        !named_type.name().is_empty(),
        path,
        &child_location,
        "type name must not be empty",
        issues,
    );
    match named_type {
        NamedType::Record { fields, extensions, .. } => {
            child_location.clear();
            write!(child_location, "{location}.extensions").expect("writing to a String cannot fail");
            validate_extensions(extensions, path, &child_location, issues);
            child_location.clear();
            write!(child_location, "{location}.fields").expect("writing to a String cannot fail");
            validate_values(fields, path, &child_location, issues);
        }
        NamedType::TaggedUnion { variants, extensions, .. } => {
            child_location.clear();
            write!(child_location, "{location}.extensions").expect("writing to a String cannot fail");
            validate_extensions(extensions, path, &child_location, issues);
            child_location.clear();
            write!(child_location, "{location}.variants").expect("writing to a String cannot fail");
            require(
                !variants.is_empty(),
                path,
                &child_location,
                "tagged union must contain at least one variant",
                issues,
            );
            validate_variants(variants, path, &child_location, issues);
        }
    }
}

pub(super) fn validate_values(
    values: impl AsRef<[crate::validation::model::NamedValue]>,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) {
    let values = values.as_ref();
    let path = path.as_ref();
    let location = location.as_ref();
    unique(values.iter().map(|item| item.name.as_str()), path, location, "name", issues);
    let mut item_location = String::with_capacity(location.len() + "[].name".len() + usize::MAX.ilog10() as usize + 1);
    for (index, value) in values.iter().enumerate() {
        item_location.clear();
        write!(item_location, "{location}[{index}]").expect("writing to a String cannot fail");
        let item_length = item_location.len();
        item_location.push_str(".name");
        require(!value.name.is_empty(), path, &item_location, "name must not be empty", issues);
        item_location.truncate(item_length);
        item_location.push_str(".type");
        validate_type(&value.value_type, path, &mut item_location, issues);
    }
}

pub(super) fn validate_variants(
    variants: impl AsRef<[crate::validation::model::Variant]>,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) {
    let variants = variants.as_ref();
    let path = path.as_ref();
    let location = location.as_ref();
    unique(variants.iter().map(|item| item.name.as_str()), path, location, "name", issues);
    let mut item_location = String::with_capacity(location.len() + "[].payload".len() + usize::MAX.ilog10() as usize + 1);
    for (index, variant) in variants.iter().enumerate() {
        item_location.clear();
        write!(item_location, "{location}[{index}]").expect("writing to a String cannot fail");
        let item_length = item_location.len();
        item_location.push_str(".name");
        require(!variant.name.is_empty(), path, &item_location, "name must not be empty", issues);
        if let Some(payload) = &variant.payload {
            item_location.truncate(item_length);
            item_location.push_str(".payload");
            validate_type(payload, path, &mut item_location, issues);
        }
    }
}

fn validate_type(value_type: &TypeRef, path: impl AsRef<Path>, location: &mut String, issues: &mut Vec<ValidationIssue>) {
    validate_type_at(value_type, path.as_ref(), location, issues);
}

fn validate_type_at(value_type: &TypeRef, path: &Path, location: &mut String, issues: &mut Vec<ValidationIssue>) {
    match value_type {
        TypeRef::Primitive { name } => {
            // Closed primitive vocabulary from the CTSC registry schema; changing it requires a coordinated schema revision.
            const PRIMITIVES: [&str; 10] = ["unit", "string", "bool", "i32", "i64", "u32", "u64", "f32", "f64", "bytes"];
            let base = location.len();
            location.push_str(".name");
            require(
                PRIMITIVES.contains(&name.as_str()),
                path,
                &*location,
                "unknown primitive type",
                issues,
            );
            location.truncate(base);
        }
        TypeRef::Named {
            name,
            component_id,
            registry_id,
        } => {
            let base = location.len();
            location.push_str(".name");
            require(!name.is_empty(), path, &*location, "type name must not be empty", issues);
            location.truncate(base);
            if component_id.as_ref().is_some_and(|value| value.is_empty()) {
                location.push_str(".componentId");
                issue(located(path, &*location), "componentId must not be empty", issues);
                location.truncate(base);
            }
            if registry_id.as_ref().is_some_and(|value| value.is_empty()) {
                location.push_str(".registryId");
                issue(located(path, &*location), "registryId must not be empty", issues);
                location.truncate(base);
            }
            if registry_id.is_some() && component_id.is_none() {
                issue(
                    located(path, &*location),
                    "registryId requires componentId on a named type reference",
                    issues,
                );
            }
        }
        TypeRef::List { items } | TypeRef::Set { items } => {
            let base = location.len();
            location.push_str(".items");
            validate_type_at(items, path, location, issues);
            location.truncate(base);
        }
        TypeRef::Map { keys, values } => {
            let base = location.len();
            location.push_str(".keys");
            validate_type_at(keys, path, location, issues);
            location.truncate(base);
            location.push_str(".values");
            validate_type_at(values, path, location, issues);
            location.truncate(base);
        }
        TypeRef::Tuple { items } => {
            let base = location.len();
            for (index, item) in items.iter().enumerate() {
                write!(location, ".items[{index}]").expect("writing to a String cannot fail");
                validate_type_at(item, path, location, issues);
                location.truncate(base);
            }
        }
        TypeRef::Record { fields } => {
            let base = location.len();
            location.push_str(".fields");
            unique(fields.iter().map(|item| item.name.as_str()), path, &*location, "name", issues);
            let fields_base = location.len();
            for (index, field) in fields.iter().enumerate() {
                write!(location, "[{index}]").expect("writing to a String cannot fail");
                let item_base = location.len();
                location.push_str(".name");
                require(!field.name.is_empty(), path, &*location, "name must not be empty", issues);
                location.truncate(item_base);
                location.push_str(".type");
                validate_type_at(&field.value_type, path, location, issues);
                location.truncate(fields_base);
            }
            location.truncate(base);
        }
        TypeRef::Optional { value } => {
            let base = location.len();
            location.push_str(".value");
            validate_type_at(value, path, location, issues);
            location.truncate(base);
        }
        TypeRef::TaggedUnion { variants } => {
            let base = location.len();
            location.push_str(".variants");
            require(
                !variants.is_empty(),
                path,
                &*location,
                "tagged union must contain at least one variant",
                issues,
            );
            unique(variants.iter().map(|item| item.name.as_str()), path, &*location, "name", issues);
            let variants_base = location.len();
            for (index, variant) in variants.iter().enumerate() {
                write!(location, "[{index}]").expect("writing to a String cannot fail");
                let item_base = location.len();
                location.push_str(".name");
                require(!variant.name.is_empty(), path, &*location, "name must not be empty", issues);
                if let Some(payload) = &variant.payload {
                    location.truncate(item_base);
                    location.push_str(".payload");
                    validate_type_at(payload, path, location, issues);
                }
                location.truncate(variants_base);
            }
            location.truncate(base);
        }
    }
}

pub(super) fn document_uses(document: &RegistryDocument, registry_id: impl AsRef<str>) -> bool {
    let registry_id = registry_id.as_ref();
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
                    .any(|value| type_uses(&value.value_type, registry_id))
                    || operation
                        .outcomes
                        .result
                        .as_ref()
                        .is_some_and(|value| type_uses(value, registry_id))
                    || operation
                        .outcomes
                        .errors
                        .iter()
                        .filter_map(|error| error.value_type.as_ref())
                        .any(|value| type_uses(value, registry_id))
            })
            || component
                .types
                .iter()
                .any(|named_type| type_uses(&named_type.as_type(), registry_id))
    })
}

pub(super) fn type_uses(value_type: &TypeRef, registry_id: impl AsRef<str>) -> bool {
    let registry_id = registry_id.as_ref();
    match value_type {
        TypeRef::Named {
            registry_id: referenced, ..
        } => referenced.as_deref() == Some(registry_id),
        TypeRef::List { items } | TypeRef::Set { items } => type_uses(items, registry_id),
        TypeRef::Map { keys, values } => type_uses(keys, registry_id) || type_uses(values, registry_id),
        TypeRef::Tuple { items } => items.iter().any(|item| type_uses(item, registry_id)),
        TypeRef::Record { fields } => fields.iter().any(|field| type_uses(&field.value_type, registry_id)),
        TypeRef::Optional { value } => type_uses(value, registry_id),
        TypeRef::TaggedUnion { variants } => variants
            .iter()
            .filter_map(|variant| variant.payload.as_ref())
            .any(|payload| type_uses(payload, registry_id)),
        TypeRef::Primitive { .. } => false,
    }
}
