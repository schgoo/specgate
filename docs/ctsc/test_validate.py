"""Literal locations in the reference linked validator (no new wire format)."""

import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from validate import Validator, validate_linked, validate_value_type


class LiteralLocations(unittest.TestCase):
    def test_literal_names(self):
        ordinary = ["ordinary", "_field2", "AZaz09"]
        quoted = [
            "some", "none", "ok", "err", "Some", "None", "Ok", "Err", "result", "unit",
            "a.b", "a[0]", "a b", "", "0", "1a", 'q"\\', "\n\t\0", "😀",
            "한글", "café", "\u0345", "\u05b0", "a\u0345", "a\u05b0",
        ]
        for name in ordinary + quoted:
            location = f"$.{name}" if name in ordinary else f"$[{json.dumps(name, ensure_ascii=False)}]"
            value_type = {"kind": "record", "fields": [
                {"name": name, "type": {"kind": "primitive", "name": "i32"}}
            ]}
            for value, expected in [({"intValue": "7"}, []),
                                    ({"boolValue": True}, [f"{location}: i32 must use intValue"])]:
                with self.subTest(name=name, value=value):
                    validator = Validator()
                    wire = {"kvlistValue": {"values": [{"key": name, "value": value}]}}
                    validate_value_type(wire, value_type, "demo", {}, "$", validator)
                    self.assertEqual(validator.errors, expected)

    def test_linked_input(self):
        corpus = Path(__file__).parent / "corpus" / "linked" / "valid"
        registry = json.loads((corpus / "registry.json").read_text(encoding="utf-8"))
        trace = json.loads((corpus / "trace.otlp.json").read_text(encoding="utf-8"))
        registry["components"][0]["operations"][0]["inputs"][0]["name"] = "a.b"
        resource = trace["resourceSpans"][0]
        operation = resource["scopeSpans"][0]["spans"][2]
        inputs = next(a["value"] for a in operation["attributes"]
                      if a["key"] == "conformance.operation.inputs")
        inputs["kvlistValue"]["values"][0]["key"] = "a.b"
        with tempfile.TemporaryDirectory() as directory:
            registry_path, trace_path = Path(directory) / "registry.json", Path(directory) / "trace.json"
            registry_path.write_text(json.dumps(registry), encoding="utf-8")
            for attribute in resource["resource"]["attributes"]:
                if attribute["key"] == "conformance.registry.digest":
                    attribute["value"]["stringValue"] = "sha256:" + hashlib.sha256(registry_path.read_bytes()).hexdigest()
            trace_path.write_text(json.dumps(trace), encoding="utf-8")
            self.assertEqual(validate_linked(trace_path, registry_path).errors, [])
            inputs["kvlistValue"]["values"][0]["value"] = {"boolValue": True}
            trace_path.write_text(json.dumps(trace), encoding="utf-8")
            self.assertEqual(validate_linked(trace_path, registry_path).errors, [
                '$[0].resourceSpans[0].operation[2].inputs["a.b"]: collection must use arrayValue'
            ])

    def test_typed_findings(self):
        integer = {"kind": "primitive", "name": "i32"}
        bad = {"boolValue": True}

        def kv(name, value):
            return {"kvlistValue": {"values": [{"key": name, "value": value}]}}

        def record(name, child=integer):
            return {"kind": "record", "fields": [{"name": name, "type": child}]}

        cases = [
            (record("a.b"), kv("a.b", bad), '$["a.b"]'),
            (record("a", record("b")), kv("a", kv("b", bad)), "$.a.b"),
            (record("0"), kv("0", bad), '$["0"]'),
            ({"kind": "tuple", "items": [integer]},
             {"arrayValue": {"values": [bad]}}, "$[0]"),
            ({"kind": "map", "ordered": False,
              "keys": {"kind": "primitive", "name": "string"}, "values": integer},
             kv('q"\\', bad), '$["q\\"\\\\"]'),
            ({"kind": "tagged_union", "variants": [{"name": "Some", "payload": integer}]},
             kv("Some", bad), '$["Some"]'),
            ({"kind": "optional", "value": integer}, kv("Some", bad), "$.Some"),
        ]
        for value_type, value, location in cases:
            with self.subTest(location=location):
                validator = Validator()
                validate_value_type(value, value_type, "demo", {}, "$", validator)
                self.assertEqual(validator.errors, [f"{location}: i32 must use intValue"])


if __name__ == "__main__":
    unittest.main()
