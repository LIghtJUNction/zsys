#!/usr/bin/env python3
"""Verify crates.io archive identity before tagging a successful publication."""
import argparse
import hashlib
import json
from pathlib import Path
import time
import urllib.error
import urllib.request


def record(version):
    request = urllib.request.Request(f'https://crates.io/api/v1/crates/zsys/{version}',
                                    headers={'User-Agent':'zsys-release (github.com/LIghtJUNction/zsys)'})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)['version']
    except urllib.error.HTTPError as error:
        if error.code == 404: return None
        raise


def check(version, archive):
    published = record(version)
    if published is None: return False
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    if published.get('checksum') != checksum:
        raise RuntimeError('published version has a different archive; refusing to tag/relabel it')
    return True


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('mode', choices=['check','verify'])
    p.add_argument('version')
    p.add_argument('archive', type=Path)
    a = p.parse_args()
    for attempt in range(24 if a.mode == 'verify' else 1):
        if check(a.version,a.archive):
            print('exists'); return
        if a.mode == 'check':
            print('publish'); return
        time.sleep(5)
    raise SystemExit('publication is not visible on crates.io; no release tag created')

if __name__ == '__main__': main()
