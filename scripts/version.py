#!/usr/bin/env python3
"""Read or bump the crate's SemVer and regenerate both resolved lockfiles."""
import argparse
import pathlib
import re
import tomllib


def bumped(version, change):
    match = re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version)
    if match is None:
        raise ValueError("expected a release version in MAJOR.MINOR.PATCH form")
    major, minor, patch = map(int, match.groups())
    if change == "major":
        return f"{major + 1}.0.0"
    if change == "minor":
        return f"{major}.{minor + 1}.0"
    if change == "patch":
        return f"{major}.{minor}.{patch + 1}"
    if change == "none":
        return version
    raise ValueError("unknown version bump")


def update_lock(text, old, new):
    """Change only this path package; a release bump must not upgrade benchmarks."""
    pattern = r'(\[\[package\]\]\s+name = "zsys"\s+version = ")' + re.escape(old) + r'(")'
    updated, count = re.subn(pattern, lambda match: match.group(1) + new + match.group(2), text)
    if count != 1:
        raise ValueError("lockfile must contain exactly one matching zsys path package")
    packages = tomllib.loads(updated)["package"]
    if next(package for package in packages if package["name"] == "zsys")["version"] != new:
        raise ValueError("lockfile version replacement failed")
    return updated


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bump", nargs="?", choices=["none", "patch", "minor", "major"], default="none")
    args = parser.parse_args()
    root = pathlib.Path(__file__).resolve().parent.parent
    manifest = root / "Cargo.toml"
    text = manifest.read_text()
    match = re.search(r'(?m)^version = "([^"]+)"$', text)
    if match is None:
        raise SystemExit("package version missing")
    version = bumped(match.group(1), args.bump)
    if args.bump != "none":
        locks = [(root / name) for name in ("Cargo.lock", "benchmarks/Cargo.lock")]
        updated_locks = [update_lock(path.read_text(), match.group(1), version) for path in locks]
        manifest.write_text(text[:match.start(1)] + version + text[match.end(1):])
        for path, text in zip(locks, updated_locks):
            path.write_text(text)
    print(version)


if __name__ == "__main__":
    main()
