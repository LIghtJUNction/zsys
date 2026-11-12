#!/usr/bin/env python3
"""Dependency-free, tested stable-version updates. No credentials are read by this script."""
import argparse
import datetime
from pathlib import Path
import re
import subprocess
import tomllib


def version(root):
    data = tomllib.loads((root/'Cargo.toml').read_text())['package']
    if data['name'] != 'zsys':
        raise ValueError('not the zsys package')
    value = data['version']
    if not re.fullmatch(r'\d+\.\d+\.\d+', value):
        raise ValueError('stable numeric version required')
    return value


def next_version(current, kind):
    major, minor, patch = map(int, current.split('.'))
    if kind == 'major':
        major, minor, patch = major+1, 0, 0
    elif kind == 'minor':
        minor, patch = minor+1, 0
    elif kind == 'patch':
        patch += 1
    elif kind != 'none':
        raise ValueError('invalid version bump')
    return f'{major}.{minor}.{patch}'


def update(root, kind):
    old = version(root)
    new = next_version(old, kind)
    replacements = {}
    manifest = root/'Cargo.toml'
    source = manifest.read_text()
    section = re.search(r'(?ms)^\[package\]\n(.*?)(?=^\[|\Z)', source)
    if not section:
        raise ValueError('missing package section')
    body, count = re.subn(r'(?m)^version\s*=\s*"'+re.escape(old)+r'"[ \t]*$', f'version = "{new}"', section[1])
    if count != 1:
        raise ValueError('ambiguous package version')
    replacements[manifest] = source[:section.start(1)]+body+source[section.end(1):]
    for relative in ('Cargo.lock', 'benchmarks/Cargo.lock'):
        path = root/relative
        source = path.read_text()
        entries = [p for p in tomllib.loads(source)['package'] if p['name'] == 'zsys']
        if len(entries) != 1 or entries[0]['version'] != old:
            raise ValueError(f'inconsistent {relative}')
        pattern = r'(?m)(^\[\[package\]\]\nname = "zsys"\nversion = ")'+re.escape(old)+r'(")'
        replacement, count = re.subn(pattern, lambda m: m[1]+new+m[2], source)
        if count != 1:
            raise ValueError(f'ambiguous {relative}')
        replacements[path] = replacement
    # Validate every input and output before writing any file.
    for content in replacements.values():
        tomllib.loads(content)
    if kind != 'none':
        for path, content in replacements.items():
            path.write_text(content)
    return new


def changelog(root, value):
    path = root/'CHANGELOG.md'
    old = path.read_text() if path.exists() else '# Changelog\n'
    if re.search(r'(?m)^## '+re.escape(value)+r'(?:\s|$)', old):
        return
    tags = subprocess.check_output(['git','tag','--list','v*','--sort=-version:refname'], cwd=root, text=True).splitlines()
    previous = next((tag for tag in tags if tag != 'v'+value), None)
    revision = f'{previous}..HEAD' if previous else 'HEAD'
    notes = subprocess.check_output(['git','log',revision,'--no-merges','--format=- %s (%h)'], cwd=root, text=True).strip()
    date = datetime.datetime.now(datetime.timezone.utc).date()
    section = f'\n## {value} - {date}\n\n{notes or "- Initial release."}\n'
    header, separator, rest = old.partition('\n')
    path.write_text(header+separator+section+'\n'+rest.lstrip('\n'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['check','bump','changelog'])
    parser.add_argument('kind', nargs='?', default='none', choices=['none','patch','minor','major'])
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    value = update(root, args.kind if args.command == 'bump' else 'none')
    if args.command == 'changelog':
        changelog(root,value)
    print(value)


if __name__ == '__main__':
    main()
