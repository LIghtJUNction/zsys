#!/usr/bin/env python3
"""Paired child-process runs. Raw samples and metadata are retained, never edited."""
import argparse
import csv
import json
import os
import pathlib
import platform
import statistics
import subprocess
import time

p = argparse.ArgumentParser()
p.add_argument('--output', default='benchmark-results')
p.add_argument('--samples', type=int, default=1000)
p.add_argument('--cases', nargs='+', default=['memory'])
a = p.parse_args()
root = pathlib.Path(__file__).resolve().parent.parent
out = root / a.output
out.mkdir(parents=True, exist_ok=True)
binary = root / 'benchmarks/target/release/zsys-benchmarks'
def command(*cmd):
    return subprocess.check_output(cmd, text=True, cwd=root).strip()
metadata = {
    'commit': command('git', 'rev-parse', 'HEAD'),
    'time_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
    'uname': platform.uname()._asdict(),
    'os_release': pathlib.Path('/etc/os-release').read_text(),
    'cpuinfo': pathlib.Path('/proc/cpuinfo').read_text(),
    'meminfo': pathlib.Path('/proc/meminfo').read_text(),
    'rustc': command('rustc', '-Vv'),
    'cargo': command('cargo', '-V'),
    'sysinfo': '0.40.3',
    'profile': 'release; thin LTO; codegen-units=1',
    'rustflags': os.environ.get('RUSTFLAGS', ''),
    'samples': a.samples,
    'warmup_per_child': 50,
    'visible_processes': len([x for x in pathlib.Path('/proc').iterdir() if x.name.isdecimal()]),
    'allocation_scope': 'Rust global allocator only; successful alloc/realloc calls and requested bytes',
    'peak_memory_scope': 'VmHWM per fresh child process, including executable and benchmark harness',
    'comparison_scope': 'memory total/available and swap total/free; both refresh every iteration',
}
(out / 'environment.json').write_text(json.dumps(metadata, indent=2))
summary = []
for case in a.cases:
    for repeat in range(3):
        order = ['zsys', 'sysinfo'] if repeat % 2 == 0 else ['sysinfo', 'zsys']
        for impl in order:
            run = subprocess.run([str(binary), impl, case, str(a.samples)], text=True,
                                 capture_output=True, check=True, timeout=180)
            prefix = out / f'{case}-{impl}-{repeat}'
            prefix.with_suffix('.csv').write_text(run.stdout)
            prefix.with_suffix('.stderr').write_text(run.stderr)
            rows = list(csv.DictReader(run.stdout.splitlines()))
            ns = sorted(int(x['ns']) for x in rows)
            def percentile(q):
                import math
                return ns[max(0, math.ceil(q * len(ns)) - 1)]
            item = dict(case=case, implementation=impl, repeat=repeat, samples=len(ns),
                        mean_ns=statistics.mean(ns), median_ns=statistics.median(ns),
                        p95_ns=percentile(.95), p99_ns=percentile(.99), max_ns=max(ns),
                        operations_per_second=1e9/statistics.mean(ns),
                        allocations_per_op=statistics.mean(int(r['allocations']) for r in rows),
                        allocated_bytes_per_op=statistics.mean(int(r['allocated_bytes']) for r in rows),
                        peak_extra_live_bytes=max(int(r['peak_extra_live_bytes']) for r in rows),
                        process_peak_rss=run.stderr.strip())
            summary.append(item)
(out / 'summary.json').write_text(json.dumps(summary, indent=2))
print(json.dumps({'environment': {k:v for k,v in metadata.items() if k not in ('cpuinfo','meminfo')},
                  'results': summary}, indent=2))
