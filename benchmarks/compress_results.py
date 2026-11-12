#!/usr/bin/env python3
"""Losslessly gzip completed raw CSV files and verify before replacing them."""
import argparse
import gzip
import hashlib
import pathlib
p = argparse.ArgumentParser()
p.add_argument('directory', type=pathlib.Path)
a = p.parse_args()
for raw in a.directory.rglob('*.csv'):
    data = raw.read_bytes()
    encoded = gzip.compress(data, mtime=0)
    assert gzip.decompress(encoded) == data
    target = raw.with_suffix('.csv.gz')
    if target.exists():
        assert gzip.decompress(target.read_bytes()) == data
    else:
        target.write_bytes(encoded)
    assert hashlib.sha256(gzip.decompress(target.read_bytes())).digest() == hashlib.sha256(data).digest()
    raw.unlink()
