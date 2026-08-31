#!/usr/bin/env python3
"""Validate CTSC registry and OTLP trace documents."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import struct
import sys
from pathlib import Path
from typing import Any

try:
    import jsonschema
    from google.protobuf.json_format import ParseDict, ParseError
    from opentelemetry.proto.trace.v1.trace_pb2 import TracesData
except ImportError as error:
    raise SystemExit(
        "missing validator dependencies; run: "
        "python -m pip install -r docs/ctsc/requirements.txt"
    ) from error


CTSC_SPANS = {
    "conformance.run",
    "conformance.scenario",
    "conformance.operation",
    "conformance.parallel",
}
CTSC_EVENTS = {
    "conformance.observation",
    "conformance.result",
    "conformance.empty",
    "conformance.error",
    "conformance.fault",
}
REQUIRED_RESOURCE_ATTRIBUTES = {
    "conformance.version",
    "conformance.tool.name",
    "conformance.tool.version",
    "conformance.target.name",
    "conformance.target.language",
}
ANY_VALUE_KEYS = {
    "stringValue",
    "boolValue",
    "intValue",
    "doubleValue",
    "bytesValue",
    "arrayValue",
    "kvlistValue",
}
DIGEST_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
TRACE_ID_RE = re.compile(r"^[0-9a-f]{32}$")
SPAN_ID_RE = re.compile(r"^[0-9a-f]{16}$")


class Validator:
    """Accumulate validation failures with document locations."""

    def __init__(self) -> None:
        self.errors: list[str] = []

    def error(self, location: str, message: str) -> None:
        self.errors.append(f"{location}: {message}")

    def require(self, condition: bool, location: str, message: str) -> None:
        if not condition:
            self.error(location, message)


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"{path}: {error}") from error


def attribute_map(
    attributes: Any, location: str, validator: Validator
) -> dict[str, dict[str, Any]]:
    result: dict[str, dict[str, Any]] = {}
    if not isinstance(attributes, list):
        validator.error(location, "attributes must be an array")
        return result
    for index, attribute in enumerate(attributes):
        key = attribute.get("key")
        if not isinstance(key, str):
            validator.error(f"{location}.attributes[{index}]", "attribute key must be a string")
            continue
        if key in result:
            validator.error(location, f"duplicate attribute key {key!r}")
            continue
        value = attribute.get("value")
        if not isinstance(value, dict):
            validator.error(f"{location}.{key}", "attribute value must be an AnyValue")
            continue
        result[key] = value
    return result


def string_attribute(
    attributes: dict[str, dict[str, Any]],
    key: str,
    location: str,
    validator: Validator,
    *,
    required: bool = True,
) -> str | None:
    value = attributes.get(key)
    if value is None:
        if required:
            validator.error(location, f"missing string attribute {key!r}")
        return None
    if set(value) != {"stringValue"} or not isinstance(value["stringValue"], str):
        validator.error(location, f"attribute {key!r} must use stringValue")
        return None
    return value["stringValue"]


def validate_any_value(value: dict[str, Any], location: str, validator: Validator) -> None:
    selected = set(value).intersection(ANY_VALUE_KEYS)
    validator.require(
        len(selected) == 1,
        location,
        "AnyValue must select exactly one concrete value variant",
    )
    if len(selected) != 1:
        return
    kind = next(iter(selected))
    if kind == "doubleValue":
        number = value[kind]
        if (
            isinstance(number, bool)
            or not isinstance(number, (int, float))
            or not math.isfinite(float(number))
        ):
            validator.error(location, "doubleValue must be finite")
    elif kind == "arrayValue":
        array = value[kind]
        if not isinstance(array, dict) or not isinstance(array.get("values", []), list):
            validator.error(location, "arrayValue must contain a values array")
            return
        for index, item in enumerate(array.get("values", [])):
            if not isinstance(item, dict):
                validator.error(f"{location}[{index}]", "array item must be an AnyValue")
            else:
                validate_any_value(item, f"{location}[{index}]", validator)
    elif kind == "kvlistValue":
        kvlist = value[kind]
        if not isinstance(kvlist, dict) or not isinstance(kvlist.get("values", []), list):
            validator.error(location, "kvlistValue must contain a values array")
            return
        seen: set[str] = set()
        for index, item in enumerate(kvlist.get("values", [])):
            item_location = f"{location}.{index}"
            if not isinstance(item, dict) or not isinstance(item.get("key"), str):
                validator.error(item_location, "kvlist item must contain a string key")
                continue
            key = item["key"]
            if key in seen:
                validator.error(location, f"duplicate kvlist key {key!r}")
            seen.add(key)
            child = item.get("value")
            if not isinstance(child, dict):
                validator.error(item_location, "kvlist item must contain an AnyValue")
            else:
                validate_any_value(child, f"{location}.{key}", validator)


def unique_names(
    items: Any, location: str, validator: Validator, field: str = "name"
) -> None:
    if not isinstance(items, list):
        return
    seen: set[str] = set()
    for index, item in enumerate(items):
        if not isinstance(item, dict):
            continue
        name = item.get(field)
        if not isinstance(name, str):
            continue
        if name in seen:
            validator.error(location, f"duplicate {field} {name!r}")
        seen.add(name)


def validate_registry_semantics(document: dict[str, Any], validator: Validator) -> None:
    imports = document.get("imports", [])
    unique_names(imports, "$.imports", validator, "registryId")
    components = document.get("components", [])
    unique_names(components, "$.components", validator, "id")

    local_components = {component.get("id"): component for component in components}
    for component_index, component in enumerate(components):
        location = f"$.components[{component_index}]"
        operations = component.get("operations", [])
        types = component.get("types", [])
        dependencies = component.get("dependencies", [])
        unique_names(operations, f"{location}.operations", validator)
        unique_names(types, f"{location}.types", validator)
        unique_names(dependencies, f"{location}.dependencies", validator, "componentId")

        local_types = {item.get("name") for item in types}
        for operation_index, operation in enumerate(operations):
            op_location = f"{location}.operations[{operation_index}]"
            unique_names(operation.get("inputs", []), f"{op_location}.inputs", validator)
            unique_names(
                operation.get("observations", []),
                f"{op_location}.observations",
                validator,
            )
            unique_names(
                operation.get("outcomes", {}).get("errors", []),
                f"{op_location}.outcomes.errors",
                validator,
            )

        validate_type_declaration_names(component, location, validator)

        for reference, reference_location in walk_type_references(component, location):
            registry_id = reference.get("registryId")
            component_id = reference.get("componentId")
            name = reference.get("name")
            if registry_id is not None:
                if not any(item.get("registryId") == registry_id for item in imports):
                    validator.error(reference_location, f"unknown imported registry {registry_id!r}")
                if not any(
                    dependency.get("registryId") == registry_id
                    and dependency.get("componentId") == component_id
                    for dependency in dependencies
                ):
                    validator.error(reference_location, "missing matching component dependency")
            elif component_id is not None:
                target = local_components.get(component_id)
                if target is None:
                    validator.error(reference_location, f"unknown local component {component_id!r}")
                elif component_id != component.get("id") and not any(
                    dependency.get("componentId") == component_id
                    and dependency.get("registryId") is None
                    for dependency in dependencies
                ):
                    validator.error(reference_location, "missing matching local dependency")
                elif target is not None and name not in {
                    item.get("name") for item in target.get("types", [])
                }:
                    validator.error(reference_location, f"unknown named type {name!r}")
            elif name not in local_types:
                validator.error(reference_location, f"unknown local named type {name!r}")


def walk_type_references(value: Any, location: str):
    if isinstance(value, dict):
        if value.get("kind") == "named":
            yield value, location
        for key, child in value.items():
            yield from walk_type_references(child, f"{location}.{key}")
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from walk_type_references(child, f"{location}[{index}]")


def validate_type_declaration_names(
    value: Any, location: str, validator: Validator
) -> None:
    if isinstance(value, dict):
        if value.get("kind") == "record":
            unique_names(value.get("fields", []), f"{location}.fields", validator)
        elif value.get("kind") == "tagged_union":
            unique_names(value.get("variants", []), f"{location}.variants", validator)
        for key, child in value.items():
            validate_type_declaration_names(child, f"{location}.{key}", validator)
    elif isinstance(value, list):
        for index, child in enumerate(value):
            validate_type_declaration_names(child, f"{location}[{index}]", validator)


def list_field(
    container: dict[str, Any],
    key: str,
    location: str,
    validator: Validator,
) -> list[Any]:
    value = container.get(key, [])
    if not isinstance(value, list):
        validator.error(f"{location}.{key}", f"{key} must be an array")
        return []
    return value


def kvlist_entries(
    value: dict[str, Any], location: str, validator: Validator
) -> dict[str, dict[str, Any]] | None:
    if set(value) != {"kvlistValue"}:
        validator.error(location, "value must use kvlistValue")
        return None
    raw = value["kvlistValue"].get("values", [])
    if not isinstance(raw, list):
        validator.error(location, "kvlistValue values must be an array")
        return None
    entries: dict[str, dict[str, Any]] = {}
    for index, item in enumerate(raw):
        if not isinstance(item, dict) or not isinstance(item.get("key"), str):
            validator.error(f"{location}[{index}]", "invalid kvlist entry")
            continue
        key = item["key"]
        child = item.get("value")
        if not isinstance(child, dict):
            validator.error(f"{location}.{key}", "missing AnyValue")
            continue
        if key in entries:
            validator.error(location, f"duplicate kvlist key {key!r}")
        entries[key] = child
    return entries


def validate_value_type(
    value: dict[str, Any],
    type_ref: dict[str, Any],
    current_component: str,
    components: dict[str, dict[str, Any]],
    location: str,
    validator: Validator,
) -> None:
    kind = type_ref["kind"]
    selected = set(value).intersection(ANY_VALUE_KEYS)
    if len(selected) != 1:
        return
    wire_kind = next(iter(selected))

    if kind == "primitive":
        primitive = type_ref["name"]
        expected_wire = {
            "unit": "kvlistValue",
            "string": "stringValue",
            "bool": "boolValue",
            "i32": "intValue",
            "i64": "intValue",
            "u32": "intValue",
            "u64": "stringValue",
            "f32": "doubleValue",
            "f64": "doubleValue",
            "bytes": "bytesValue",
        }[primitive]
        if wire_kind != expected_wire:
            validator.error(location, f"{primitive} must use {expected_wire}")
            return
        if primitive == "unit":
            entries = kvlist_entries(value, location, validator)
            if entries:
                validator.error(location, "unit must be an empty kvlistValue")
        elif primitive in {"i32", "i64", "u32"}:
            try:
                number = int(value["intValue"])
            except (TypeError, ValueError):
                validator.error(location, "intValue must contain a signed decimal integer")
                return
            bounds = {
                "i32": (-(2**31), 2**31 - 1),
                "i64": (-(2**63), 2**63 - 1),
                "u32": (0, 2**32 - 1),
            }[primitive]
            if not bounds[0] <= number <= bounds[1]:
                validator.error(location, f"value is outside {primitive} range")
        elif primitive == "u64":
            text = value["stringValue"]
            if not isinstance(text, str) or not re.fullmatch(r"0|[1-9][0-9]*", text):
                validator.error(location, "u64 must be a canonical decimal string")
            elif int(text) > 2**64 - 1:
                validator.error(location, "value is outside u64 range")
        elif primitive == "f32":
            number = float(value["doubleValue"])
            try:
                round_trip = struct.unpack("!f", struct.pack("!f", number))[0]
            except (OverflowError, struct.error):
                validator.error(location, "value is outside f32 range")
            else:
                if float(round_trip) != number:
                    validator.error(location, "value is not exactly representable as f32")
        return

    if kind == "named":
        if type_ref.get("registryId") is not None:
            validator.error(location, "imported named types are not supported by this validator")
            return
        component_id = type_ref.get("componentId", current_component)
        component = components.get(component_id)
        if component is None:
            validator.error(location, f"unknown component {component_id!r}")
            return
        definition = next(
            (item for item in component["types"] if item["name"] == type_ref["name"]),
            None,
        )
        if definition is None:
            validator.error(location, f"unknown named type {type_ref['name']!r}")
            return
        validate_value_type(
            value, definition, component_id, components, location, validator
        )
        return

    if kind in {"list", "set"}:
        if wire_kind != "arrayValue":
            validator.error(location, f"{kind} must use arrayValue")
            return
        items = value["arrayValue"].get("values", [])
        for index, item in enumerate(items):
            validate_value_type(
                item,
                type_ref["items"],
                current_component,
                components,
                f"{location}[{index}]",
                validator,
            )
        if kind == "set":
            canonical = [
                canonical_typed_value(
                    item, type_ref["items"], current_component, components
                )
                for item in items
            ]
            if len(canonical) != len(set(canonical)):
                validator.error(location, "set contains duplicate elements")
        return

    if kind == "tuple":
        if wire_kind != "arrayValue":
            validator.error(location, "tuple must use arrayValue")
            return
        values = value["arrayValue"].get("values", [])
        expected = type_ref["items"]
        if len(values) != len(expected):
            validator.error(location, "tuple arity does not match registry type")
            return
        for index, (item, item_type) in enumerate(zip(values, expected, strict=True)):
            validate_value_type(
                item,
                item_type,
                current_component,
                components,
                f"{location}[{index}]",
                validator,
            )
        return

    if kind == "record":
        entries = kvlist_entries(value, location, validator)
        if entries is None:
            return
        fields = {field["name"]: field["type"] for field in type_ref["fields"]}
        if set(entries) != set(fields):
            validator.error(location, "record fields do not match registry type")
            return
        for name, field_type in fields.items():
            validate_value_type(
                entries[name],
                field_type,
                current_component,
                components,
                f"{location}.{name}",
                validator,
            )
        return

    if kind == "tagged_union":
        entries = kvlist_entries(value, location, validator)
        if entries is None:
            return
        if len(entries) != 1:
            validator.error(location, "tagged union must contain exactly one variant")
            return
        name, payload = next(iter(entries.items()))
        variant = next(
            (item for item in type_ref["variants"] if item["name"] == name),
            None,
        )
        if variant is None:
            validator.error(location, f"unknown tagged-union variant {name!r}")
            return
        payload_type = variant.get("payload", {"kind": "primitive", "name": "unit"})
        validate_value_type(
            payload,
            payload_type,
            current_component,
            components,
            f"{location}.{name}",
            validator,
        )
        return

    if kind == "map":
        key_type = type_ref["keys"]
        value_type = type_ref["values"]
        if key_type == {"kind": "primitive", "name": "string"}:
            entries = kvlist_entries(value, location, validator)
            if entries is None:
                return
            for name, item in entries.items():
                validate_value_type(
                    item,
                    value_type,
                    current_component,
                    components,
                    f"{location}.{name}",
                    validator,
                )
            return
        if wire_kind != "arrayValue":
            validator.error(location, "non-string-keyed map must use arrayValue")
            return
        seen_keys: set[Any] = set()
        for index, item in enumerate(value["arrayValue"].get("values", [])):
            entry_location = f"{location}[{index}]"
            entries = kvlist_entries(item, entry_location, validator)
            if entries is None or set(entries) != {"key", "value"}:
                validator.error(entry_location, "map entry must contain key and value")
                continue
            validate_value_type(
                entries["key"],
                key_type,
                current_component,
                components,
                f"{entry_location}.key",
                validator,
            )
            canonical_key = canonical_typed_value(
                entries["key"], key_type, current_component, components
            )
            if canonical_key in seen_keys:
                validator.error(location, "map contains duplicate keys")
            seen_keys.add(canonical_key)
            validate_value_type(
                entries["value"],
                value_type,
                current_component,
                components,
                f"{entry_location}.value",
                validator,
            )


def canonical_typed_value(
    value: dict[str, Any],
    type_ref: dict[str, Any],
    current_component: str,
    components: dict[str, dict[str, Any]],
) -> Any:
    kind = type_ref["kind"]
    if kind == "named":
        if type_ref.get("registryId") is not None:
            return ("external", json.dumps(value, sort_keys=True, separators=(",", ":")))
        component_id = type_ref.get("componentId", current_component)
        component = components[component_id]
        definition = next(
            item for item in component["types"] if item["name"] == type_ref["name"]
        )
        return canonical_typed_value(value, definition, component_id, components)
    if kind == "primitive":
        primitive = type_ref["name"]
        raw = next(iter(value.values()))
        if primitive in {"i32", "i64", "u32", "u64"}:
            raw = int(raw)
        elif primitive in {"f32", "f64"}:
            raw = struct.pack("!d", float(raw))
        elif primitive == "unit":
            raw = ()
        return (primitive, raw)
    if kind in {"list", "tuple"}:
        item_types = (
            type_ref["items"]
            if kind == "tuple"
            else [type_ref["items"]] * len(value["arrayValue"].get("values", []))
        )
        return (
            kind,
            tuple(
                canonical_typed_value(item, item_type, current_component, components)
                for item, item_type in zip(
                    value["arrayValue"].get("values", []), item_types, strict=True
                )
            ),
        )
    if kind == "set":
        items = [
            canonical_typed_value(
                item, type_ref["items"], current_component, components
            )
            for item in value["arrayValue"].get("values", [])
        ]
        return ("set", tuple(sorted(items, key=repr)))
    if kind == "record":
        entries = {
            item["key"]: item["value"]
            for item in value["kvlistValue"].get("values", [])
        }
        fields = {field["name"]: field["type"] for field in type_ref["fields"]}
        return (
            "record",
            tuple(
                (
                    name,
                    canonical_typed_value(
                        entries[name], fields[name], current_component, components
                    ),
                )
                for name in sorted(fields)
            ),
        )
    if kind == "tagged_union":
        item = value["kvlistValue"]["values"][0]
        variant = next(
            variant for variant in type_ref["variants"] if variant["name"] == item["key"]
        )
        payload_type = variant.get("payload", {"kind": "primitive", "name": "unit"})
        return (
            "tagged_union",
            item["key"],
            canonical_typed_value(
                item["value"], payload_type, current_component, components
            ),
        )
    if kind == "map":
        key_type = type_ref["keys"]
        value_type = type_ref["values"]
        if key_type == {"kind": "primitive", "name": "string"}:
            entries = [
                (
                    item["key"],
                    canonical_typed_value(
                        item["value"], value_type, current_component, components
                    ),
                )
                for item in value["kvlistValue"].get("values", [])
            ]
        else:
            entries = []
            for item in value["arrayValue"].get("values", []):
                pair = {entry["key"]: entry["value"] for entry in item["kvlistValue"]["values"]}
                entries.append(
                    (
                        canonical_typed_value(
                            pair["key"], key_type, current_component, components
                        ),
                        canonical_typed_value(
                            pair["value"], value_type, current_component, components
                        ),
                    )
                )
        return ("map", tuple(sorted(entries, key=repr)))
    raise ValueError(f"unsupported registry type kind {kind!r}")


def validate_registry(path: Path) -> tuple[dict[str, Any] | None, Validator]:
    validator = Validator()
    try:
        document = load_json(path)
        schema = load_json(Path(__file__).with_name("ctsc-registry-0.1.schema.json"))
        jsonschema.Draft202012Validator.check_schema(schema)
        schema_errors = list(jsonschema.Draft202012Validator(schema).iter_errors(document))
        for error in schema_errors:
            location = "$" + "".join(
                f"[{part}]" if isinstance(part, int) else f".{part}"
                for part in error.absolute_path
            )
            validator.error(location, error.message)
        if isinstance(document, dict) and not schema_errors:
            validate_registry_semantics(document, validator)
        else:
            if not isinstance(document, dict):
                validator.error("$", "registry document must be an object")
    except (ValueError, jsonschema.SchemaError) as error:
        validator.error(str(path), str(error))
        document = None
    return document, validator


def validate_event(
    event: dict[str, Any],
    span_name: str,
    location: str,
    validator: Validator,
) -> None:
    name = event.get("name")
    if name not in CTSC_EVENTS:
        validator.error(location, f"unsupported CTSC event name {name!r}")
        return
    validator.require(
        event.get("droppedAttributesCount", 0) == 0,
        location,
        "event droppedAttributesCount must be zero",
    )
    attributes = attribute_map(event.get("attributes", []), location, validator)
    operation_events = {
        "conformance.observation",
        "conformance.result",
        "conformance.empty",
        "conformance.error",
    }
    if name in operation_events:
        validator.require(
            span_name == "conformance.operation",
            location,
            f"{name} must belong to a conformance.operation span",
        )

    if name == "conformance.observation":
        string_attribute(attributes, "conformance.observation.name", location, validator)
        value = attributes.get("conformance.observation.value")
        if value is None:
            validator.error(location, "missing conformance.observation.value")
        else:
            validate_any_value(value, f"{location}.value", validator)
    elif name == "conformance.result":
        value = attributes.get("conformance.result.value")
        if value is None:
            validator.error(location, "missing conformance.result.value")
        else:
            validate_any_value(value, f"{location}.value", validator)
    elif name == "conformance.empty":
        if "conformance.result.value" in attributes:
            validator.error(location, "empty event must not contain a result value")
    elif name == "conformance.error":
        string_attribute(attributes, "conformance.error.name", location, validator)
        value = attributes.get("conformance.error.value")
        if value is not None:
            validate_any_value(value, f"{location}.value", validator)
    elif name == "conformance.fault":
        validator.require(
            span_name
            in {"conformance.run", "conformance.scenario", "conformance.operation"},
            location,
            "fault must belong to a run, scenario, or operation span",
        )
        for key in ("conformance.fault.type", "conformance.fault.observer"):
            string_attribute(attributes, key, location, validator)
        for key in ("conformance.fault.message", "conformance.fault.native_type"):
            string_attribute(attributes, key, location, validator, required=False)


def status_is_error(span: dict[str, Any]) -> bool:
    status = span.get("status") or {}
    return status.get("code") in {2, "STATUS_CODE_ERROR"}


def validate_trace(path: Path) -> tuple[list[dict[str, Any]], Validator]:
    validator = Validator()
    batches: list[dict[str, Any]] = []
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        validator.error(str(path), str(error))
        return batches, validator

    documents: list[tuple[str, Any]] = []
    try:
        documents.append((str(path), json.loads(text)))
    except json.JSONDecodeError:
        for line_number, line in enumerate(text.splitlines(), 1):
            if not line:
                validator.error(f"{path}:{line_number}", "blank JSONL line")
                continue
            try:
                documents.append((f"{path}:{line_number}", json.loads(line)))
            except json.JSONDecodeError as error:
                validator.error(f"{path}:{line_number}", f"invalid JSON: {error}")

    for location, batch in documents:
        try:
            ParseDict(batch, TracesData())
            batches.append(batch)
        except (ParseError, ValueError) as error:
            validator.error(location, f"invalid OTLP TracesData: {error}")

    spans: list[tuple[dict[str, Any], dict[str, dict[str, Any]], str]] = []
    for batch_index, batch in enumerate(batches):
        for resource_index, resource_spans in enumerate(
            list_field(batch, "resourceSpans", f"$[{batch_index}]", validator)
        ):
            if not isinstance(resource_spans, dict):
                validator.error(
                    f"$[{batch_index}].resourceSpans[{resource_index}]",
                    "resourceSpans item must be an object",
                )
                continue
            resource_location = (
                f"$[{batch_index}].resourceSpans[{resource_index}].resource"
            )
            resource = resource_spans.get("resource") or {}
            if not isinstance(resource, dict):
                validator.error(resource_location, "resource must be an object")
                resource = {}
            resource_attributes = attribute_map(
                resource.get("attributes", []), resource_location, validator
            )
            for key in REQUIRED_RESOURCE_ATTRIBUTES:
                actual = string_attribute(
                    resource_attributes, key, resource_location, validator
                )
                if key == "conformance.version" and actual is not None:
                    validator.require(
                        actual == "0.1.0",
                        resource_location,
                        "conformance.version must be '0.1.0'",
                    )
            for scope_index, scope_spans in enumerate(
                list_field(resource_spans, "scopeSpans", resource_location, validator)
            ):
                if not isinstance(scope_spans, dict):
                    validator.error(
                        f"{resource_location}.scopeSpans[{scope_index}]",
                        "scopeSpans item must be an object",
                    )
                    continue
                for span_index, span in enumerate(
                    list_field(
                        scope_spans,
                        "spans",
                        f"{resource_location}.scopeSpans[{scope_index}]",
                        validator,
                    )
                ):
                    if not isinstance(span, dict):
                        validator.error(
                            f"{resource_location}.scopeSpans[{scope_index}].spans[{span_index}]",
                            "span must be an object",
                        )
                        continue
                    if span.get("name") in CTSC_SPANS:
                        spans.append(
                            (
                                span,
                                resource_attributes,
                                f"$[{batch_index}].resourceSpans[{resource_index}]"
                                f".scopeSpans[{scope_index}].spans[{span_index}]",
                            )
                        )

    validator.require(bool(spans), str(path), "trace contains no CTSC spans")

    by_id: dict[tuple[str, str], tuple[dict[str, Any], str]] = {}
    for span, _, location in spans:
        trace_id = span.get("traceId")
        span_id = span.get("spanId")
        validator.require(
            isinstance(trace_id, str)
            and bool(TRACE_ID_RE.fullmatch(trace_id))
            and trace_id != "0" * 32,
            location,
            "traceId must be 32 lowercase hexadecimal characters and nonzero",
        )
        validator.require(
            isinstance(span_id, str)
            and bool(SPAN_ID_RE.fullmatch(span_id))
            and span_id != "0" * 16,
            location,
            "spanId must be 16 lowercase hexadecimal characters and nonzero",
        )
        key = (trace_id, span_id)
        if key in by_id:
            validator.error(location, "duplicate span ID within trace")
        by_id[key] = (span, location)

    for span, _, location in spans:
        name = span["name"]
        validator.require(
            span.get("droppedAttributesCount", 0) == 0,
            location,
            "droppedAttributesCount must be zero",
        )
        validator.require(
            span.get("droppedEventsCount", 0) == 0,
            location,
            "droppedEventsCount must be zero",
        )
        validator.require(
            span.get("droppedLinksCount", 0) == 0,
            location,
            "droppedLinksCount must be zero",
        )
        attributes = attribute_map(span.get("attributes", []), location, validator)
        parent_id = span.get("parentSpanId", "")
        parent = by_id.get((span.get("traceId"), parent_id))
        parent_name = parent[0].get("name") if parent else None

        if name == "conformance.run":
            validator.require(not parent_id, location, "run span must be a root span")
            string_attribute(attributes, "conformance.run.id", location, validator)
        elif name == "conformance.scenario":
            validator.require(
                parent_name == "conformance.run",
                location,
                "scenario parent must be a conformance.run span",
            )
            string_attribute(attributes, "conformance.scenario.name", location, validator)
        elif name == "conformance.operation":
            validator.require(
                parent_name
                in {
                    "conformance.scenario",
                    "conformance.operation",
                    "conformance.parallel",
                },
                location,
                "operation parent must be scenario, operation, or parallel",
            )
            string_attribute(attributes, "conformance.component.id", location, validator)
            string_attribute(attributes, "conformance.operation.name", location, validator)
            inputs = attributes.get("conformance.operation.inputs")
            if inputs is None or set(inputs) != {"kvlistValue"}:
                validator.error(location, "operation inputs must use kvlistValue")
            else:
                validate_any_value(inputs, f"{location}.inputs", validator)
        elif name == "conformance.parallel":
            validator.require(
                parent_name in {"conformance.scenario", "conformance.operation"},
                location,
                "parallel parent must be scenario or operation",
            )

        event_names: list[str] = []
        result_names: list[str | None] = []
        for event_index, event in enumerate(
            list_field(span, "events", location, validator)
        ):
            if not isinstance(event, dict):
                validator.error(f"{location}.events[{event_index}]", "event must be an object")
                continue
            validate_event(
                event,
                name,
                f"{location}.events[{event_index}]",
                validator,
            )
            event_name = event.get("name")
            event_names.append(event_name)
            if event_name == "conformance.result":
                event_attributes = attribute_map(
                    event.get("attributes", []),
                    f"{location}.events[{event_index}]",
                    validator,
                )
                result_names.append(
                    string_attribute(
                        event_attributes,
                        "conformance.result.name",
                        f"{location}.events[{event_index}]",
                        validator,
                        required=False,
                    )
                )
        if name == "conformance.operation":
            non_result_terminal = sum(
                event_name
                in {"conformance.empty", "conformance.error", "conformance.fault"}
                for event_name in event_names
            )
            validator.require(
                non_result_terminal <= 1,
                location,
                "operation must not contain multiple non-result completion/failure events",
            )
            validator.require(
                not (result_names and non_result_terminal),
                location,
                "result events cannot be combined with another completion/failure event",
            )
            validator.require(
                result_names.count(None) <= 1,
                location,
                "operation may contain at most one unnamed result",
            )
            named_results = [result_name for result_name in result_names if result_name]
            validator.require(
                len(named_results) == len(set(named_results)),
                location,
                "named results must be unique within an operation",
            )
            if "conformance.error" in event_names or "conformance.fault" in event_names:
                validator.require(
                    status_is_error(span),
                    location,
                    "declared error and fault operations must have ERROR status",
                )
        if name in {"conformance.run", "conformance.scenario"} and (
            "conformance.fault" in event_names
        ):
            validator.require(
                status_is_error(span),
                location,
                "fault-bearing run or scenario must have ERROR status",
            )

    return batches, validator


def validate_full(trace_path: Path, registry_path: Path) -> Validator:
    document, registry_validation = validate_registry(registry_path)
    batches, trace_validation = validate_trace(trace_path)
    validator = Validator()
    validator.errors.extend(registry_validation.errors)
    validator.errors.extend(trace_validation.errors)
    if document is None or validator.errors:
        return validator

    expected_digest = "sha256:" + hashlib.sha256(registry_path.read_bytes()).hexdigest()
    components = {component["id"]: component for component in document["components"]}

    for batch_index, batch in enumerate(batches):
        for resource_index, resource_spans in enumerate(batch.get("resourceSpans") or []):
            location = f"$[{batch_index}].resourceSpans[{resource_index}]"
            resource = resource_spans.get("resource") or {}
            resource_attributes = attribute_map(
                resource.get("attributes", []),
                location,
                validator,
            )
            registry_id = string_attribute(
                resource_attributes, "conformance.registry.id", location, validator
            )
            digest = string_attribute(
                resource_attributes, "conformance.registry.digest", location, validator
            )
            validator.require(
                registry_id == document["registryId"],
                location,
                "trace registry ID does not match registry document",
            )
            validator.require(
                digest == expected_digest,
                location,
                "trace registry digest does not match exact registry bytes",
            )
            for scope_spans in resource_spans.get("scopeSpans") or []:
                for span_index, span in enumerate(scope_spans.get("spans") or []):
                    if span.get("name") != "conformance.operation":
                        continue
                    span_location = f"{location}.operation[{span_index}]"
                    attributes = {
                        item["key"]: item["value"]
                        for item in span.get("attributes", [])
                        if "key" in item and "value" in item
                    }
                    component_id = attributes.get("conformance.component.id", {}).get(
                        "stringValue"
                    )
                    operation_name = attributes.get(
                        "conformance.operation.name", {}
                    ).get("stringValue")
                    component = components.get(component_id)
                    if component is None:
                        validator.error(span_location, f"unknown component {component_id!r}")
                        continue
                    operations = {
                        operation["name"]: operation
                        for operation in component["operations"]
                    }
                    operation = operations.get(operation_name)
                    if operation is None:
                        validator.error(
                            span_location,
                            f"unknown operation {operation_name!r} in {component_id!r}",
                        )
                        continue

                    input_values = kvlist_entries(
                        attributes["conformance.operation.inputs"],
                        f"{span_location}.inputs",
                        validator,
                    )
                    declared_inputs = {
                        item["name"]: item["type"] for item in operation["inputs"]
                    }
                    if input_values is not None:
                        if set(input_values) != set(declared_inputs):
                            validator.error(
                                f"{span_location}.inputs",
                                "input names do not match registry operation",
                            )
                        else:
                            for input_name, input_type in declared_inputs.items():
                                validate_value_type(
                                    input_values[input_name],
                                    input_type,
                                    component_id,
                                    components,
                                    f"{span_location}.inputs.{input_name}",
                                    validator,
                                )

                    declared_observations = {
                        item["name"]: item for item in operation["observations"]
                    }
                    for event_index, event in enumerate(span.get("events") or []):
                        if event.get("name") != "conformance.observation":
                            continue
                        event_attributes = {
                            item["key"]: item["value"]
                            for item in event.get("attributes") or []
                            if "key" in item and "value" in item
                        }
                        observation_name = event_attributes[
                            "conformance.observation.name"
                        ]["stringValue"]
                        declaration = declared_observations.get(observation_name)
                        if declaration is None:
                            validator.error(
                                f"{span_location}.events[{event_index}]",
                                f"observation {observation_name!r} is not declared",
                            )
                            continue
                        validate_value_type(
                            event_attributes["conformance.observation.value"],
                            declaration["type"],
                            component_id,
                            components,
                            f"{span_location}.events[{event_index}].value",
                            validator,
                        )
                    has_fault = any(
                        event.get("name") == "conformance.fault"
                        for event in span.get("events") or []
                    )
                    terminal = [
                        event
                        for event in span.get("events") or []
                        if event.get("name")
                        in {
                            "conformance.result",
                            "conformance.empty",
                            "conformance.error",
                        }
                    ]
                    outcome = operation["outcomes"]
                    if has_fault:
                        continue
                    if not terminal:
                        if "result" in outcome or outcome.get("empty") is True:
                            validator.error(
                                span_location,
                                "unit completion not permitted by registry outcomes",
                            )
                    elif all(
                        event["name"] == "conformance.result" for event in terminal
                    ):
                        if "result" not in outcome:
                            validator.error(span_location, "result outcome not declared")
                        else:
                            for result_index, result_event in enumerate(terminal):
                                result_attributes = {
                                    item["key"]: item["value"]
                                    for item in result_event.get("attributes") or []
                                    if "key" in item and "value" in item
                                }
                                validate_value_type(
                                    result_attributes["conformance.result.value"],
                                    outcome["result"],
                                    component_id,
                                    components,
                                    f"{span_location}.result[{result_index}]",
                                    validator,
                                )
                    elif terminal[0]["name"] == "conformance.empty":
                        if outcome.get("empty") is not True:
                            validator.error(span_location, "empty outcome not declared")
                    elif terminal[0]["name"] == "conformance.error":
                        event_attributes = {
                            item["key"]: item["value"]
                            for item in terminal[0].get("attributes") or []
                            if "key" in item and "value" in item
                        }
                        error_name = event_attributes.get(
                            "conformance.error.name", {}
                        ).get("stringValue")
                        declared = {
                            item["name"]: item for item in outcome.get("errors", [])
                        }
                        declaration = declared.get(error_name)
                        if declaration is None:
                            validator.error(
                                span_location,
                                f"error outcome {error_name!r} not declared",
                            )
                        elif "type" in declaration:
                            if "conformance.error.value" not in event_attributes:
                                validator.error(
                                    span_location,
                                    f"error outcome {error_name!r} requires a value",
                                )
                            else:
                                validate_value_type(
                                    event_attributes["conformance.error.value"],
                                    declaration["type"],
                                    component_id,
                                    components,
                                    f"{span_location}.error",
                                    validator,
                                )
                        elif "conformance.error.value" in event_attributes:
                            validator.error(
                                span_location,
                                f"error outcome {error_name!r} does not declare a value",
                            )
    return validator


def print_result(label: str, validator: Validator) -> bool:
    if validator.errors:
        print(f"{label}: invalid", file=sys.stderr)
        for error in validator.errors:
            print(f"  {error}", file=sys.stderr)
        return False
    print(f"{label}: valid")
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    registry_parser = subparsers.add_parser("registry")
    registry_parser.add_argument("path", type=Path)

    trace_parser = subparsers.add_parser("trace")
    trace_parser.add_argument("path", type=Path)

    full_parser = subparsers.add_parser("full")
    full_parser.add_argument("trace", type=Path)
    full_parser.add_argument("registry", type=Path)

    args = parser.parse_args()

    if args.command == "registry":
        _, result = validate_registry(args.path)
        return 0 if print_result(str(args.path), result) else 1
    if args.command == "trace":
        _, result = validate_trace(args.path)
        return 0 if print_result(str(args.path), result) else 1
    if args.command == "full":
        result = validate_full(args.trace, args.registry)
        return 0 if print_result(str(args.trace), result) else 1
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
