#!/usr/bin/env python3
"""Publish the tested package, or verify that an identical retry already exists."""
import hashlib
import json
import os
import pathlib
import subprocess
import time
import urllib.error
import urllib.request

version = os.environ["RELEASE_VERSION"]
package = pathlib.Path(f"target/package/zsys-{version}.crate")
checksum = hashlib.sha256(package.read_bytes()).hexdigest()
url = "https://index.crates.io/zs/ys/zsys"


def published():
    request = urllib.request.Request(url, headers={"User-Agent": "zsys-release/1.0", "Cache-Control": "no-cache"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            for line in response:
                record = json.loads(line)
                if record["vers"] == version:
                    if record["cksum"] != checksum:
                        raise SystemExit("version already exists with a different package; refusing to tag it")
                    if record["yanked"]:
                        raise SystemExit("published version is yanked; refusing to release it")
                    return True
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
    return False


if published():
    print(f"zsys {version} already contains the identical verified package")
else:
    if not os.environ.get("CARGO_REGISTRY_TOKEN"):
        raise SystemExit("set repository secret CARGO_REGISTRY_TOKEN or CARGO_REGISTY_TOKEN before publishing")
    subprocess.run(["cargo", "publish", "--locked"], check=True)
    for attempt in range(12):
        if published():
            print(f"Verified zsys {version} in the crates.io index")
            break
        time.sleep(5)
    else:
        raise SystemExit("upload returned but index confirmation timed out; rerun this release to verify")
