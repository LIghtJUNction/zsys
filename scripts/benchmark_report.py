#!/usr/bin/env python3
"""Recompute pooled metrics from raw rows, independently of the benchmark runner."""
import argparse
import collections
import csv
import hashlib
import json
import math
from pathlib import Path
import statistics


def metrics(rows):
    values = sorted(int(row['ns']) for row in rows)
    if not values or values[0] <= 0:
        raise ValueError('empty or invalid latency samples')
    units = [int(row['units']) for row in rows]
    if min(units) < 1:
        raise ValueError('empty collection scope')
    return dict(samples=len(rows), mean_ns=statistics.mean(values), median_ns=statistics.median(values),
                p95_ns=values[math.ceil(.95*len(values))-1], p99_ns=values[math.ceil(.99*len(values))-1],
                max_ns=values[-1], operations_per_second=1e9/statistics.mean(values),
                units_min=min(units), units_max=max(units), failures=sum(int(r['failed']) for r in rows))


def aggregate(directory):
    environment = json.loads((directory/'environment.json').read_text())
    entries = json.loads((directory/'summary.json').read_text())
    groups = collections.defaultdict(list)
    hashes = {}
    seen = set()
    for entry in entries:
        scale, case, implementation = entry['requested_process_scale'], entry['case'], entry['implementation']
        filename = f"{scale}-{case}-{implementation}-{entry['repeat']}-{entry['mode']}.csv"
        if filename in seen:
            raise ValueError(f'duplicate measurement: {filename}')
        seen.add(filename)
        raw = directory/filename
        hashes[filename] = hashlib.sha256(raw.read_bytes()).hexdigest()
        with raw.open(newline='') as source:
            rows = list(csv.DictReader(source))
        computed = metrics(rows)
        for key in ('samples','mean_ns','median_ns','p95_ns','p99_ns','max_ns','units_min','units_max','failures'):
            if not math.isclose(computed[key],entry[key],rel_tol=1e-12):
                raise ValueError(f'{filename}: summary mismatch for {key}')
        groups[(str(scale),case,implementation,entry['mode'])].append((entry,rows))
    output = []
    for (scale,case,implementation,mode), repetitions in sorted(groups.items()):
        if mode != 'latency':
            continue
        rows = [r for _, samples in repetitions for r in samples]
        value = metrics(rows)
        value.update(dataset=directory.name, source_commit=environment.get('source_commit'), scale=scale,
                     case=case, implementation=implementation, repetitions=len(repetitions),
                     median_of_repetition_medians_ns=statistics.median(e['median_ns'] for e,_ in repetitions),
                     process_peak_rss_bytes=max(e['process_peak_rss_bytes'] for e,_ in repetitions))
        allocated = [r for _, samples in groups.get((scale,case,implementation,'allocations'),[]) for r in samples]
        if allocated:
            value.update(allocation_samples=len(allocated),
                         allocations_per_op=statistics.mean(int(r['allocations']) for r in allocated),
                         allocated_bytes_per_op=statistics.mean(int(r['allocated_bytes']) for r in allocated),
                         peak_extra_live_bytes=max(int(r['peak_extra_live_bytes']) for r in allocated))
        else:
            value.update(allocation_samples=0,allocations_per_op=None,allocated_bytes_per_op=None,peak_extra_live_bytes=None)
        output.append(value)
    for value in output:
        peers = [p for p in output if p['scale']==value['scale'] and p['case']==value['case'] and p['implementation'] in ('zsys','sysinfo')]
        matched = (len(peers)==2 and all(p['units_min']==p['units_max']==peers[0]['units_min'] and p['failures']==0 for p in peers))
        value['equal_count_failure_free_pair'] = matched
        # Unit count equivalence is necessary, not a proof of identical internal work.
    return output, hashes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory',type=Path)
    parser.add_argument('--output',type=Path,required=True)
    args = parser.parse_args()
    output, hashes = aggregate(args.directory)
    args.output.mkdir(parents=True,exist_ok=True)
    (args.output/'aggregate.json').write_text(json.dumps(output,indent=2)+'\n')
    with (args.output/'aggregate.csv').open('w',newline='') as handle:
        writer = csv.DictWriter(handle,fieldnames=list(output[0]))
        writer.writeheader(); writer.writerows(output)
    (args.output/'raw-sha256.json').write_text(json.dumps(hashes,indent=2)+'\n')
    print(f'Validated {len(hashes)} raw measurements; wrote {len(output)} pooled profiles')


if __name__ == '__main__':
    main()
