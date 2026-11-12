#!/usr/bin/env python3
"""Exercise both binaries and reject fabricated zero-allocation latency reports."""
import argparse
import json
import pathlib
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--latency', type=pathlib.Path, required=True)
    parser.add_argument('--counted', type=pathlib.Path, required=True)
    args = parser.parse_args()
    root = pathlib.Path(__file__).resolve().parent.parent
    for mode, binary in [('latency', args.latency), ('allocation-counting', args.counted)]:
        binary = binary.resolve()
        with tempfile.TemporaryDirectory(prefix='zsys-modes-') as temporary:
            out = pathlib.Path(temporary) / 'run'
            subprocess.run([sys.executable, str(root / 'benchmarks/run.py'), '--binary', str(binary),
                            '--output', str(out), '--samples', '3', '--repeats', '1',
                            '--implementations', 'zsys', '--cases', 'memory', 'cold_process_single'],
                           check=True, stdout=subprocess.DEVNULL)
            report = json.loads(subprocess.check_output(
                [sys.executable, str(root / 'benchmarks/summarize.py'), str(out)], text=True))
            assert not report['failed_runs'], report['failed_runs']
            for result in report['results']:
                assert result['measurement_mode'] == mode
                assert result['samples'] == 3 and result['median_ns'] > 0
                for key in ('allocations_per_op', 'allocated_bytes_per_op', 'peak_extra_live_bytes'):
                    assert (result[key] is None) == (mode == 'latency'), (mode, key, result)
                assert ('retained_heap_before_bytes_range' in result) == (mode == 'allocation-counting')
                if mode == 'allocation-counting' and result['case'] == 'cold_process_single':
                    assert result['allocations_per_op'] > 0 and result['allocated_bytes_per_op'] > 0
    print('latency and allocation binaries: separate modes verified')


if __name__ == '__main__':
    main()
