#!/usr/bin/env python3
"""Generate the self-host form of the harness spec.

Reads `specs/specgate.harness.spec.yaml` and emits
`test/rust/crates/specgate-fixtures/specs/selfhost_harness.spec.yaml`, the
executable self-host rendering: every `run_spec` case becomes a case that
invokes the `run_spec` operation (the `selfhost.rs` wrapper) on a fixture spec
and asserts the structured `$result`.

Run from the repo root:  python scripts/gen_selfhost_harness_spec.py

The source of truth is the harness spec. When its `run_spec` cases or their
documented `traces:` change, regenerate this file and re-run the (ignored)
`harness_spec_self_hosts` test.
"""
import pathlib
import yaml

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "specs" / "specgate.harness.spec.yaml"
DST = ROOT / "test/rust/crates/specgate-fixtures/specs/selfhost_harness.spec.yaml"

HEADER = (
    "# GENERATED from specs/specgate.harness.spec.yaml by\n"
    "# scripts/gen_selfhost_harness_spec.py — do not edit by hand.\n"
    "# Each case runs the run_spec operation on a fixture spec and asserts the\n"
    "# structured $result. Regenerate after changing the harness spec.\n"
)


def main() -> None:
    doc = yaml.safe_load(SRC.read_text(encoding="utf-8"))
    cases = []
    for c in doc.get("cases", []):
        if c.get("operation") != "run_spec":
            continue
        outcome = (c.get("expected") or {}).get("outcome")
        if not isinstance(outcome, dict):
            continue
        new = {
            "name": c["name"],
            "desc": (c.get("desc") or "").strip(),
            "operation": "run_spec",
            "inputs": c["inputs"],
        }
        if "Error" in outcome:
            # Error reasons are not asserted (variant-only), matching the
            # strength of the dogfood check and avoiding brittle reason strings.
            result_value = {"Error": {}}
        else:
            results = []
            for r in outcome["Complete"].get("results", []):
                results.append({k: r[k] for k in ("name", "status", "traces") if k in r})
            result_value = {"Complete": {"results": results}}
        new["expected"] = [{"$result": result_value}]
        cases.append(new)

    spec = {
        "spec_version": "0.4.0",
        "name": "fixture.selfhost_harness",
        "binding": "binding.yaml",
        "operations": {
            "run_spec": {
                "inputs": {"spec": "string"},
                "outputs": [{"$result": "SelfHostOutcome"}],
            }
        },
        "cases": cases,
    }
    with DST.open("w", encoding="utf-8", newline="\n") as f:
        f.write(HEADER)
        yaml.safe_dump(spec, f, sort_keys=False, default_flow_style=False, width=100)
    print(f"wrote {len(cases)} cases to {DST.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
