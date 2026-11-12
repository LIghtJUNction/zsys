#!/usr/bin/env python3
"""Independently verify the preserved network before/after samples and pool every repetition."""
import argparse
import csv
import gzip
import hashlib
import io
import json
import math
from pathlib import Path
import statistics


def aggregate(directory):
    records = json.loads((directory/'summary.json').read_text())
    environment = json.loads((directory/'environment.json').read_text())
    if environment.get('network_scope_verified') is not True:
        raise ValueError('network field-set validation did not succeed')
    groups = {}
    for record in records:
        if record['status'] != 'ok':
            raise ValueError('a failed measurement must not disappear from the audit')
        key = record['case'], record['implementation']
        path = directory/f"{key[0]}-{key[1]}-{record['repeat']}.csv.gz"
        raw = gzip.decompress(path.read_bytes())
        if hashlib.sha256(raw).hexdigest() != record['raw_samples_sha256']:
            raise ValueError(f'raw checksum mismatch: {path}')
        rows = list(csv.DictReader(io.StringIO(raw.decode())))
        times = sorted(int(row['ns']) for row in rows)
        if not times or times[0] <= 0 or len(times) != record['samples']:
            raise ValueError('invalid samples')
        expected = dict(mean_ns=statistics.mean(times), median_ns=statistics.median(times),
                        p95_ns=times[math.ceil(.95*len(times))-1], p99_ns=times[math.ceil(.99*len(times))-1],
                        max_ns=times[-1], allocations_per_op=statistics.mean(int(r['allocations']) for r in rows),
                        allocated_bytes_per_op=statistics.mean(int(r['allocated_bytes']) for r in rows))
        for name,value in expected.items():
            if not math.isclose(value,record[name],rel_tol=1e-12):
                raise ValueError(f'summary mismatch: {path}: {name}')
        group = groups.setdefault(key,dict(rows=[],records=[]))
        group['rows'].extend(rows); group['records'].append(record)
    output = []
    for (case,implementation), group in groups.items():
        rows, records = group['rows'], group['records']
        times = sorted(int(r['ns']) for r in rows)
        output.append(dict(dataset=directory.name,case=case,implementation=implementation,
                           samples=len(rows),repetitions=len(records),mean_ns=statistics.mean(times),
                           median_ns=statistics.median(times),p95_ns=times[math.ceil(.95*len(times))-1],
                           p99_ns=times[math.ceil(.99*len(times))-1],max_ns=times[-1],
                           operations_per_second=1e9/statistics.mean(times),
                           allocations_per_op=statistics.mean(int(r['allocations']) for r in rows),
                           allocated_bytes_per_op=statistics.mean(int(r['allocated_bytes']) for r in rows),
                           peak_extra_live_bytes=max(int(r['peak_extra_live_bytes']) for r in rows),
                           process_peak_rss_bytes=max(r['process_peak_rss_kib'] for r in records)*1024,
                           retained_heap_before_bytes=max(r['retained_heap_before_bytes'] for r in records),
                           retained_heap_after_bytes=max(r['retained_heap_after_bytes'] for r in records)))
    return output


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directories',nargs='+',type=Path)
    parser.add_argument('--output',required=True,type=Path)
    args=parser.parse_args()
    result=[row for directory in args.directories for row in aggregate(directory)]
    args.output.mkdir(parents=True,exist_ok=True)
    (args.output/'aggregate.json').write_text(json.dumps(result,indent=2)+'\n')
    with (args.output/'aggregate.csv').open('w',newline='') as target:
        writer=csv.DictWriter(target,fieldnames=list(result[0]))
        writer.writeheader(); writer.writerows(result)
    print(json.dumps(result,indent=2))


if __name__=='__main__': main()
