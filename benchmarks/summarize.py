#!/usr/bin/env python3
"""Pool raw samples across paired repeats; never infer ratios across failed scope gates."""
import argparse
import csv
import gzip
import hashlib
import json
import math
import pathlib
import statistics
from collections import defaultdict
p = argparse.ArgumentParser()
p.add_argument('directory', type=pathlib.Path)
p.add_argument('--output', type=pathlib.Path)
a = p.parse_args()
environment = json.loads((a.directory / 'environment.json').read_text())
mode = environment.get('measurement_mode', 'allocation-counting')
summary = json.loads((a.directory / 'summary.json').read_text())
groups = defaultdict(list)
failed = []
for item in summary:
    assert item.get('measurement_mode', mode) == mode, 'mixed measurement modes'
    if item.get('status', 'ok') != 'ok':
        failed.append(item)
        continue
    groups[(item['case'], item['implementation'])].append(item)
results = []
for (case, implementation), runs in groups.items():
    rows = []
    hashes = []
    for run in runs:
        filename = a.directory / f'{case}-{implementation}-{run["repeat"]}.csv'
        raw = gzip.decompress(filename.with_suffix('.csv.gz').read_bytes()) if filename.with_suffix('.csv.gz').exists() else filename.read_bytes()
        digest = hashlib.sha256(raw).hexdigest()
        if run.get('raw_samples_sha256'):
            assert digest == run['raw_samples_sha256'], f'raw data hash mismatch: {filename}'
        hashes.append(dict(repeat=run['repeat'], raw_sha256=digest))
        rows += list(csv.DictReader(raw.decode().splitlines()))
    ns = sorted(int(row['ns']) for row in rows)
    def quantile(q): return ns[max(0, math.ceil(q * len(ns)) - 1)]
    item = dict(case=case, implementation=implementation, scope=runs[0].get('scope', 'unvalidated_legacy'), measurement_mode=mode,
                repeats=len(runs), samples=len(ns), mean_ns=statistics.mean(ns), median_ns=statistics.median(ns),
                p95_ns=quantile(.95), p99_ns=quantile(.99), max_ns=max(ns),
                operations_per_second=1e9/statistics.mean(ns),
                allocations_per_op=statistics.mean(int(row['allocations']) for row in rows) if mode == 'allocation-counting' else None,
                allocated_bytes_per_op=statistics.mean(int(row['allocated_bytes']) for row in rows) if mode == 'allocation-counting' else None,
                peak_extra_live_bytes=max(int(row['peak_extra_live_bytes']) for row in rows) if mode == 'allocation-counting' else None, raw_hashes=hashes)
    for key in ('retained_heap_before_bytes', 'retained_heap_after_bytes', 'process_peak_rss_kib'):
        if all(key in run for run in runs): item[key + '_range'] = [min(run[key] for run in runs), max(run[key] for run in runs)]
    if rows and 'observed_processes' in rows[0]:
        item['observed_processes_range'] = [min(int(row['observed_processes']) for row in rows), max(int(row['observed_processes']) for row in rows)]
    results.append(item)
pairs = []
for case in dict.fromkeys(item['case'] for item in results):
    zsys = next((item for item in results if item['case'] == case and item['implementation'] == 'zsys'), None)
    reference = next((item for item in results if item['case'] == case and item['implementation'] == 'sysinfo'), None)
    if not zsys or not reference: continue
    eligible = all(item['scope'].startswith('matched') for item in (zsys, reference))
    if case.startswith(('process_', 'cold_process_', 'basic', 'cold_basic', 'full')):
        eligible = eligible and environment.get('process_scope_verified', False)
        # Each fresh child has a different self PID, but both must collect equal numbers.
        if 'observed_processes_range' in zsys and 'observed_processes_range' in reference:
            eligible = eligible and zsys['observed_processes_range'] == reference['observed_processes_range']
    if case in ('network', 'full'): eligible = eligible and environment.get('network_scope_verified', False)
    pair = dict(case=case, comparable=eligible, measurement_mode=mode,
                latency_claim_allowed=eligible and mode == 'latency')
    if eligible:
        pair.update(median_speedup=reference['median_ns'] / zsys['median_ns'],
                    mean_speedup=reference['mean_ns'] / zsys['mean_ns'],
                    p99_speedup=reference['p99_ns'] / zsys['p99_ns'])
    else:
        pair['reason'] = 'Different refresh scope/freshness, failed/unperformed parity gate, or unequal observed process counts; no ratio claimed'
    pairs.append(pair)
report = dict(method='All raw samples pooled across repeats; nearest-rank P95/P99; reciprocal mean excludes pacing sleeps; descriptive statistics, not confidence bounds',
              environment_file=str(a.directory / 'environment.json'), source_commit=environment['commit'],
              binary_sha256=environment.get('binary_sha256'), results=results, comparisons=pairs, failed_runs=failed)
text = json.dumps(report, indent=2)
if a.output: a.output.write_text(text + '\n')
else: print(text)
