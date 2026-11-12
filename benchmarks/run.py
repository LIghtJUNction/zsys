#!/usr/bin/env python3
"""Reproducible paired runs: preserve every sample, report errors rather than zeroes."""
import argparse
import csv
import hashlib
import gzip
import json
import math
import os
import pathlib
import platform
import resource
import statistics
import shutil
import tempfile
import subprocess
import sys
import time

SCOPES = {
    'memory': ('matched', 'RAM total/available/free/used and swap total/free; both read every call'),
    'cpu': ('matched', 'Aggregate and all online CPU utilization; >=210 ms between calls, sleep excluded'),
    'cpu_hot': ('different_freshness', 'zsys reads every call; sysinfo may reuse CPU values inside minimum interval; no speedup claim'),
    'process_single': ('matched_core', 'Self PID identity/name/parent/state/start/resident/virtual memory and cumulative CPU; no tasks/I/O/details'),
    'process_inventory': ('matched_inventory', 'PID/parent/name/start identity records; sysinfo minimum public refresh without tasks; zsys also parses its mandatory stat fields'),
    'process_all': ('matched_core', 'All visible process core records; tasks/I/O/command/executable/user excluded; disappearance races possible'),
    'process_all_std': ('zsys_only', 'Same zsys process records with std::fs::read_dir instead of getdents64'),
    'enumerate': ('zsys_only', 'Pure PID list, sorted and deduplicated; sysinfo has no PID-only public collector'),
    'enumerate_std': ('zsys_only', 'Pure PID list via std::fs::read_dir'),
    'network': ('pending_scope_validation', 'All interface counters and metadata; inspect probe and backend availability before comparing'),
    'network_counters': ('zsys_only', 'All /proc/net/dev counters; sysinfo has no counters-only public refresh selector'),
    'disk': ('matched_storage', 'Capacity refresh for exactly the sysinfo-visible fixed mount list; no rediscovery/kind/I/O'),
    'disk_io': ('zsys_only', 'All block device and partition counters; differs from sysinfo mounted disk view'),
    'full': ('matched_full', 'CPU/RAM/swap/process core + complete network + fixed sysinfo-visible mount capacities; >=210ms interval; excludes disk kind/I/O, mount rediscovery and hardware telemetry'),
    'basic': ('matched_core', 'CPU utilization + RAM/swap + all process core records; >=210 ms interval, sleep excluded; excludes network/disk'),
    'basic_hot': ('different_freshness', 'CPU + RAM/swap + all process core records; sysinfo CPU may remain cached'),
    'cold_process_single': ('matched_core', 'Construct, collect and drop one fresh collector for self PID; warm OS caches'),
    'cold_process_all': ('matched_core', 'Construct, collect and drop all-process collectors; warm OS caches'),
    'cold_basic': ('matched_core', 'Construct, collect and drop CPU/RAM/process collectors; warm OS caches; initial CPU usage has no valid delta'),
}

def main():
    p = argparse.ArgumentParser()
    p.add_argument('--output', default='benchmark-results')
    p.add_argument('--samples', type=int, default=1000)
    p.add_argument('--paced-samples', type=int, default=100)
    p.add_argument('--repeats', type=int, default=3)
    p.add_argument('--cases', nargs='+', default=['memory', 'cpu', 'process_single', 'process_all', 'disk', 'network', 'basic', 'full', 'cold_basic'])
    p.add_argument('--implementations', nargs='+', choices=['zsys', 'sysinfo'], default=['zsys', 'sysinfo'])
    p.add_argument('--binary')
    p.add_argument('--processes', type=int, default=0, help='Target this many visible processes, including the benchmark child; uses controlled sleeping children')
    a = p.parse_args()
    if not (1 <= a.samples <= 1_000_000 and 1 <= a.paced_samples <= 1_000_000 and 1 <= a.repeats <= 100):
        p.error('invalid sample/repeat count')
    root = pathlib.Path(__file__).resolve().parent.parent
    out = root / a.output
    out.mkdir(parents=True, exist_ok=True)
    if (out / 'environment.json').exists():
        p.error('output already contains a run; select a new directory to preserve raw evidence')
    binary = pathlib.Path(a.binary) if a.binary else root / 'benchmarks/target/release/zsys-benchmarks'
    binary_snapshot = tempfile.TemporaryDirectory(prefix='zsys-benchmark-')
    frozen_binary = pathlib.Path(binary_snapshot.name) / 'zsys-benchmarks'
    shutil.copyfile(binary, frozen_binary)
    frozen_binary.chmod(0o700)
    binary = frozen_binary
    def command(*cmd):
        return subprocess.check_output(cmd, text=True, cwd=root).strip()
    def read(path):
        try: return pathlib.Path(path).read_text()
        except OSError as e: return f'unavailable: {e}'
    def processes():
        return len([x for x in pathlib.Path('/proc').iterdir() if x.name.isdecimal()])
    children = []
    spawn_error = None
    try:
        desired_children = max(0, a.processes - processes() - 1) if a.processes else 0
        available_start = next(int(line.split()[1]) * 1024 for line in read('/proc/meminfo').splitlines() if line.startswith('MemAvailable:'))
        for i in range(desired_children):
            if i % 32 == 0:
                available_now = next(int(line.split()[1]) * 1024 for line in read('/proc/meminfo').splitlines() if line.startswith('MemAvailable:'))
                if available_now < max(512 * 1024 * 1024, available_start // 2):
                    spawn_error = 'stopped spawning to retain >=512MiB and >=half of starting MemAvailable'
                    break
            try: children.append(subprocess.Popen(['/bin/sleep', '7200'], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
            except OSError as e:
                spawn_error = str(e)
                break
        metadata = {
            'commit': command('git', 'rev-parse', 'HEAD'),
            'working_tree_status': command('git', 'status', '--porcelain'),
            'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
            'time_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
            'uname': platform.uname()._asdict(),
            'os_release': read('/etc/os-release'),
            'cpuinfo': read('/proc/cpuinfo'),
            'meminfo': read('/proc/meminfo'),
            'rustc': command('rustc', '-Vv'),
            'cargo': command('cargo', '-V'),
            'sysinfo': '0.40.3',
            'sysinfo_features': ['system', 'network', 'disk'],
            'sysinfo_default_features': False,
            'profile': 'release; thin LTO; codegen-units=1',
            'rustflags': os.environ.get('RUSTFLAGS', ''),
            'samples': a.samples, 'paced_samples': a.paced_samples, 'repeats': a.repeats,
            'warmup': '50 for hot cases, 2 for >=210ms cases, zero for cold construction cases',
            'visible_processes_before_benchmark_child': processes(),
            'requested_visible_processes': a.processes, 'spawn_requested': desired_children, 'spawned': len(children), 'spawn_error': spawn_error,
            'pids_max': read('/sys/fs/cgroup/pids.max'),
            'memory_max': read('/sys/fs/cgroup/memory.max'),
            'rlimit_nproc': resource.getrlimit(resource.RLIMIT_NPROC),
            'cpu_affinity': sorted(os.sched_getaffinity(0)),
            'allocation_scope': 'Successful Rust global alloc/alloc_zeroed/realloc requested bytes; allocator bookkeeping is included in measured latency; excludes C allocations',
            'peak_memory_scope': 'VmHWM per fresh child includes executable, loader, allocator and harness; per-op peak_extra_live_bytes counts requested Rust heap; not a heap profiler',
            'retained_heap_scope': 'Requested Rust heap retained by setup/collector after warmup and after measurements; excludes sample array, includes path and metadata setup',
            'cold_scope': 'Collector construction + first refresh + drop; process and kernel filesystem caches are warm',
            'process_cpu_scope': 'Cumulative CPU fields refreshed per call; hot interval utilization may have sub-tick quantization and sysinfo global denominator throttling; no usage accuracy claim from hot calls',
            'scopes': {key: SCOPES[key] for key in a.cases},
        }
        process_scope = None
        if any(case.startswith(('process_', 'cold_process_', 'basic', 'cold_basic', 'full')) for case in a.cases):
            validation = subprocess.run([str(binary), 'zsys', 'validate_process', '1'], text=True, capture_output=True)
            process_scope = validation.returncode == 0
            metadata['process_scope_verified'] = process_scope
            metadata['process_scope_validation'] = validation.stdout + validation.stderr
        network_scope = None
        if any(case in ('network', 'full') for case in a.cases):
            validation = subprocess.run([str(binary), 'zsys', 'validate_network', '1'], text=True, capture_output=True)
            network_scope = validation.returncode == 0
            metadata['network_scope_verified'] = network_scope
            metadata['network_scope_validation'] = validation.stdout + validation.stderr
        (out / 'environment.json').write_text(json.dumps(metadata, indent=2))
        summary = []
        for case in a.cases:
            for repeat in range(a.repeats):
                order = a.implementations if repeat % 2 == 0 else list(reversed(a.implementations))
                for impl in order:
                    if impl == 'sysinfo' and SCOPES[case][0] == 'zsys_only': continue
                    n = a.paced_samples if case in ('cpu', 'basic', 'full') else a.samples
                    timeout = 60 + n * (0.3 if case in ('cpu', 'basic', 'full') else .02)
                    invocation = [str(binary), impl, case, str(n)]
                    try:
                        if case == 'full' and (network_scope is not True or process_scope is not True):
                            run = subprocess.CompletedProcess(invocation, 126, '', 'full comparison unavailable: network and process content gates must both pass before timing')
                        else:
                            run = subprocess.run(invocation, text=True, capture_output=True, timeout=timeout)
                    except subprocess.TimeoutExpired as error:
                        def decoded(value): return value.decode(errors='replace') if isinstance(value, bytes) else (value or '')
                        run = subprocess.CompletedProcess(invocation, 124, decoded(error.stdout), decoded(error.stderr) + '\nbenchmark timed out')
                    except OSError as error:
                        run = subprocess.CompletedProcess(invocation, 125, '', str(error))
                    prefix = out / f'{case}-{impl}-{repeat}'
                    prefix.with_suffix('.csv.gz').write_bytes(gzip.compress(run.stdout.encode(), mtime=0))
                    prefix.with_suffix('.stderr').write_text(run.stderr)
                    item = dict(case=case, implementation=impl, repeat=repeat, scope=SCOPES[case][0], raw_samples_sha256=hashlib.sha256(run.stdout.encode()).hexdigest())
                    if case.startswith(('process_', 'cold_process_', 'basic', 'cold_basic', 'full')) and process_scope is False: item['scope'] = 'unmatched_process_metadata'
                    if case in ('network', 'full') and network_scope is False: item['scope'] = 'unmatched_network_metadata'
                    if case == 'network': item['scope'] = 'matched' if network_scope else 'unmatched_network_metadata'
                    if run.returncode:
                        item.update(status='unavailable' if run.returncode == 126 else 'failed', exit_code=run.returncode, reason=run.stderr.strip())
                    else:
                        rows = list(csv.DictReader(run.stdout.splitlines()))
                        ns = sorted(int(x['ns']) for x in rows)
                        def percentile(q): return ns[max(0, math.ceil(q * len(ns)) - 1)]
                        item.update(status='ok', samples=len(ns), mean_ns=statistics.mean(ns), median_ns=statistics.median(ns),
                                    p95_ns=percentile(.95), p99_ns=percentile(.99), max_ns=max(ns),
                                    operations_per_second=1e9/statistics.mean(ns),
                                    allocations_per_op=statistics.mean(int(r['allocations']) for r in rows),
                                    allocated_bytes_per_op=statistics.mean(int(r['allocated_bytes']) for r in rows),
                                    peak_extra_live_bytes=max(int(r['peak_extra_live_bytes']) for r in rows))
                        for line in run.stderr.splitlines():
                            if line.startswith('VmHWM:'): item['process_peak_rss_kib'] = int(line.split()[1])
                            elif '=' in line:
                                key, value = line.split('=', 1)
                                if key in ('retained_heap_before_bytes','retained_heap_after_bytes','warmup','interval_ns','mount_count','process_count'):
                                    item[key] = int(value)
                        if rows and 'observed_processes' in rows[0]:
                            item['min_observed_processes'] = min(int(row['observed_processes']) for row in rows)
                            item['max_observed_processes'] = max(int(row['observed_processes']) for row in rows)
                        item['mount_paths'] = [line[6:] for line in run.stderr.splitlines() if line.startswith('mount=')]
                    summary.append(item)
                    (out / 'summary.json').write_text(json.dumps(summary, indent=2))
                    print(json.dumps(item), flush=True)
    finally:
        for child in children: child.terminate()
        for child in children: child.wait()
        binary_snapshot.cleanup()

if __name__ == '__main__':
    main()
