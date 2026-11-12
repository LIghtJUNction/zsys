#!/usr/bin/env python3
"""Verify the runtime dependency contract and release/lockfile consistency."""
import json
import pathlib
import subprocess
import tomllib

root = pathlib.Path(__file__).resolve().parent.parent
manifest = tomllib.loads((root / "Cargo.toml").read_text())
version = manifest["package"]["version"]
metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--locked", "--format-version", "1", "--all-features"], cwd=root, text=True))
package = next(p for p in metadata["packages"] if p["name"] == "zsys")
runtime = [d for d in package["dependencies"] if d["kind"] != "dev"]
if runtime:
    raise SystemExit(f"runtime/build dependencies violate the zero-dependency contract: {runtime}")
for filename in ("Cargo.lock", "benchmarks/Cargo.lock"):
    lock = tomllib.loads((root / filename).read_text())
    own = [p for p in lock["package"] if p["name"] == "zsys"]
    if len(own) != 1 or own[0]["version"] != version:
        raise SystemExit(f"{filename} disagrees with Cargo.toml")
bench = tomllib.loads((root / "benchmarks/Cargo.toml").read_text())
if bench["dependencies"]["sysinfo"]["version"] != "=0.40.3":
    raise SystemExit("comparison must remain pinned to sysinfo =0.40.3")
print(f"zsys {version}: no runtime/build dependencies; manifests and lockfiles agree")
