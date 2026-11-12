#!/usr/bin/env python3
"""Run requested process scales sequentially; no parallel load-generator runs."""
import argparse
import json
import pathlib
import subprocess
import sys
p = argparse.ArgumentParser()
p.add_argument('--output', default='benchmark-results/scaling')
p.add_argument('--counts', type=int, nargs='+', default=[100, 500, 1000, 5000, 10000])
p.add_argument('--samples', type=int, default=200)
p.add_argument('--repeats', type=int, default=3)
a = p.parse_args()
root = pathlib.Path(__file__).resolve().parent.parent
for count in a.counts:
    if not 1 <= count <= 10000:
        p.error('count must be within 1..10000')
    subprocess.run([sys.executable, str(root / 'benchmarks/run.py'), '--output', f'{a.output}/{count}',
                    '--processes', str(count), '--samples', str(a.samples), '--repeats', str(a.repeats),
                    '--cases', 'enumerate', 'enumerate_std', 'process_all', 'process_all_std'], check=True)
    meta = json.loads((root / a.output / str(count) / 'environment.json').read_text())
    if meta['spawn_error']:
        # Do not repeatedly hit the same process/memory limit at larger scales.
        pending = [n for n in a.counts if n > count]
        result = dict(status='blocked_by_resources', attempted=count,
                      actual_visible=meta['visible_processes_before_benchmark_child'] + 1,
                      reason=meta['spawn_error'], unattempted=pending)
        (root / a.output / 'resource-limit.json').write_text(json.dumps(result, indent=2))
        print(json.dumps(result), flush=True)
        break
