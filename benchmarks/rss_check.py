#!/usr/bin/env python3
"""Compare Linux RSS interfaces on one idle, controlled child; no library sources read."""
import argparse
import json
import os
import pathlib
import platform
import subprocess
import time
p = argparse.ArgumentParser()
p.add_argument('--output', type=pathlib.Path)
a = p.parse_args()
child = subprocess.Popen(['/bin/sleep', '60'])
try:
    time.sleep(.05)
    root = pathlib.Path('/proc') / str(child.pid)
    rows = []
    for _ in range(3):
        stat = (root / 'stat').read_bytes()
        tail = stat[stat.rfind(b')') + 2:].split()
        statm = (root / 'statm').read_text().split()
        row = dict(time_monotonic_ns=time.monotonic_ns(),
                   stat_virtual_bytes=int(tail[20]),
                   stat_resident_bytes=int(tail[21]) * os.sysconf('SC_PAGE_SIZE'),
                   statm_virtual_bytes=int(statm[0]) * os.sysconf('SC_PAGE_SIZE'),
                   statm_resident_bytes=int(statm[1]) * os.sysconf('SC_PAGE_SIZE'))
        for line in (root / 'status').read_text().splitlines():
            if line.startswith('VmRSS:'): row['status_resident_bytes'] = int(line.split()[1]) * 1024
        try:
            for line in (root / 'smaps_rollup').read_text().splitlines():
                if line.startswith('Rss:'): row['smaps_rollup_resident_bytes'] = int(line.split()[1]) * 1024
        except OSError as e:
            row['smaps_rollup_error'] = str(e)
        rows.append(row)
        time.sleep(.05)
    result = dict(kernel=platform.release(), page_size=os.sysconf('SC_PAGE_SIZE'),
                  startup_wait_ms=50, intersample_wait_ms=50, samples=rows,
                  note='Both proc stat RSS and statm RSS can be kernel approximations. A difference prevents treating the interfaces as identical in a benchmark.')
    text = json.dumps(result, indent=2)
    if a.output:
        if a.output.exists(): p.error('refusing to overwrite existing evidence')
        a.output.write_text(text + '\n')
    print(text)
finally:
    child.terminate()
    child.wait()
