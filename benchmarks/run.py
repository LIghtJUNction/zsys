#!/usr/bin/env python3
"""Paired isolated measurements; keep raw rows, failed runs, and resource-limit skips."""
import argparse
import contextlib
import csv
import hashlib
import io
import json
import math
import os
from pathlib import Path
import platform
import re
import resource
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parent.parent
CASES = ["memory", "cpu", "network", "disk-space", "disk-io", "process-one",
         "process-list", "process-all", "process-one-cpu", "process-all-cpu",
         "process-details", "basic", "cold-memory", "cold-process-all", "cold-network",
         "cold-basic", "enumerate", "network-counters", "disk-io-all"]
STANDALONE = {"enumerate", "network-counters", "disk-io-all"}


def text(path):
    try:
        return Path(path).read_text()
    except OSError:
        return None


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def proc_count():
    return sum(p.name.isdecimal() for p in Path('/proc').iterdir())


def percentile(values, quantile):
    """Nearest-rank percentile; requires a nonempty, sorted sequence."""
    if not values or not 0 < quantile <= 1:
        raise ValueError('invalid percentile input')
    return values[max(0, math.ceil(quantile * len(values)) - 1)]


def summarize(rows, stderr, measured_allocations):
    if not rows:
        raise ValueError('no samples')
    ns = sorted(int(r['ns']) for r in rows)
    if min(ns) <= 0 or min(int(r['units']) for r in rows) <= 0:
        raise ValueError('empty or invalid measured work')
    peak = re.search(r'VmHWM:\s*(\d+)\s+kB', stderr)
    result = dict(samples=len(rows), mean_ns=statistics.mean(ns), median_ns=statistics.median(ns),
                  p95_ns=percentile(ns, .95), p99_ns=percentile(ns, .99), max_ns=max(ns),
                  operations_per_second=1e9/statistics.mean(ns),
                  units_min=min(int(r['units']) for r in rows), units_max=max(int(r['units']) for r in rows),
                  failures=sum(int(r['failed']) for r in rows),
                  process_peak_rss_bytes=int(peak[1])*1024 if peak else None)
    if measured_allocations:
        result.update(allocations_per_op=statistics.mean(int(r['allocations']) for r in rows),
                      allocated_bytes_per_op=statistics.mean(int(r['allocated_bytes']) for r in rows),
                      peak_extra_live_bytes=max(int(r['peak_extra_live_bytes']) for r in rows))
    return result


@contextlib.contextmanager
def sleepers(count):
    children = []
    try:
        for _ in range(count):
            children.append(subprocess.Popen(['sleep', '3600'], stdin=subprocess.DEVNULL,
                                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        yield children
    finally:
        for child in children:
            if child.poll() is None:
                child.terminate()
        for child in children:
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()


def safe_additional():
    """Use at most half of available cgroup memory; never consume the last PID slots."""
    limits = [max(0, (os.sysconf('SC_AVPHYS_PAGES') * os.sysconf('SC_PAGESIZE')) // (4*1024*1024))]
    memory_max, memory_current = text('/sys/fs/cgroup/memory.max'), text('/sys/fs/cgroup/memory.current')
    if memory_max and memory_max.strip().isdecimal() and memory_current:
        limits.append(max(0, (int(memory_max)-int(memory_current)) // (4*1024*1024)))
    pid_max, pid_current = text('/sys/fs/cgroup/pids.max'), text('/sys/fs/cgroup/pids.current')
    if pid_max and pid_max.strip().isdecimal() and pid_current:
        limits.append(max(0, int(pid_max)-int(pid_current)-64))
    nproc = resource.getrlimit(resource.RLIMIT_NPROC)[0]
    if nproc != resource.RLIM_INFINITY:
        limits.append(max(0, nproc-proc_count()-64))
    return min(limits)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', default='benchmark-results')
    parser.add_argument('--samples', type=int, default=1000)
    parser.add_argument('--paced-samples', type=int, default=100)
    parser.add_argument('--allocation-samples', type=int, default=50)
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--cases', nargs='+', choices=CASES, default=CASES)
    parser.add_argument('--scales', nargs='*', type=int, default=[100, 500, 1000, 5000, 10000])
    parser.add_argument('--scale-cases', nargs='+', default=['process-list', 'process-all', 'enumerate'])
    parser.add_argument('--source-commit', help='Verified remote commit matching this source tree, when available')
    parser.add_argument('--skip-build', action='store_true')
    args = parser.parse_args()
    if min(args.samples, args.paced_samples, args.allocation_samples, args.repeats) <= 0:
        parser.error('sample and repetition counts must be positive')
    if any(n < 1 or n > 10000 for n in args.scales):
        parser.error('process scale must be between 1 and 10000')
    if any(c not in CASES for c in args.scale_cases):
        parser.error('unknown scale profile')
    output = (ROOT / args.output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    binaries = {"latency": ROOT/'benchmarks/target/release/zsys-benchmarks',
                "allocations": ROOT/'benchmarks/target-counted/release/zsys-benchmarks'}
    if not args.skip_build:
        for mode in binaries:
            cmd = ['cargo', 'build', '--locked', '--release', '--manifest-path', 'benchmarks/Cargo.toml']
            if mode == 'allocations':
                cmd += ['--features', 'count-alloc', '--target-dir', 'benchmarks/target-counted']
            with (output/f'build-{mode}.log').open('w') as log:
                subprocess.run(cmd, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True)
    scope = command(str(binaries['latency']), 'scope')
    scope_path = output/'scope.tsv'
    scope_path.write_text(scope+'\n')
    delta = subprocess.check_output(['git', 'diff', 'HEAD'], cwd=ROOT)
    metadata = dict(
        time_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
        source_commit=args.source_commit, local_commit=command('git', 'rev-parse', 'HEAD'),
        tracked_diff_sha256=hashlib.sha256(delta).hexdigest(), status=command('git', 'status', '--porcelain'),
        uname=platform.uname()._asdict(), os_release=text('/etc/os-release'),
        cpuinfo=text('/proc/cpuinfo'), meminfo=text('/proc/meminfo'),
        rustc=command('rustc', '-Vv'), cargo=command('cargo', '-V'), sysinfo='0.40.3',
        profile='release; thin LTO; codegen-units=1', rustflags=os.environ.get('RUSTFLAGS', ''),
        binary_sha256={k: hashlib.sha256(p.read_bytes()).hexdigest() for k,p in binaries.items()},
        cgroup={k:text('/sys/fs/cgroup/'+k) for k in ['cpu.max','memory.max','memory.current','pids.max','pids.current']},
        options=vars(args), visible_processes=proc_count(),
        allocation_scope='Separate instrumented binary: Rust alloc/realloc requests only; native C allocations excluded.',
        rss_scope='VmHWM of fresh latency child including binary, collectors, and sample vector.',
        pacing='CPU profiles: sysinfo minimum update interval plus 5ms for both libraries, outside measurement.',
        cold_scope='New collectors plus first sample per iteration, OS page caches NOT flushed; not reboot cold.',
        process_scope='Same visible procfs, tasks disabled; process memory uses each public RSS estimate.',
        comparison_scope='See benchmarks/PROTOCOL.md; standalone profiles are never represented as matched comparisons.')
    (output/'environment.json').write_text(json.dumps(metadata, indent=2))
    summary, skips, pairs = [], [], []
    def save():
        (output/'summary.json').write_text(json.dumps(summary, indent=2))
        (output/'skips.json').write_text(json.dumps(skips, indent=2))
        (output/'pair-validation.json').write_text(json.dumps(pairs, indent=2))
    def measure(case, scale, pid):
        if (case == 'disk-io' and 'device\t' not in scope) or (case == 'disk-space' and 'mount\t' not in scope):
            skips.append(dict(case=case, scale=scale, reason='No nonempty common device/mount scope'))
            save()
            return
        implementations = ['zsys', 'zsys-std'] if case == 'enumerate' else ['zsys'] if case in STANDALONE else ['zsys', 'sysinfo']
        if case in ('process-list', 'process-all'):
            implementations += ['zsys-std']
        paced = case in ('cpu', 'basic') or case.endswith('-cpu')
        for repeat in range(args.repeats):
            order = implementations if repeat % 2 == 0 else list(reversed(implementations))
            for mode, binary in binaries.items():
                sample_count = (min(10, args.allocation_samples) if paced else args.allocation_samples) if mode == 'allocations' else args.paced_samples if paced else args.samples
                for implementation in order:
                    prefix = f'{scale}-{case}-{implementation}-{repeat}-{mode}'
                    start_count = proc_count()
                    cmd = [str(binary), implementation, case, str(sample_count), str(scope_path), str(pid)]
                    try:
                        run = subprocess.run(cmd, cwd=ROOT, text=True, capture_output=True,
                                             timeout=max(60, sample_count*.25+30))
                    except subprocess.TimeoutExpired as exc:
                        skips.append(dict(case=case, scale=scale, implementation=implementation, mode=mode,
                                          repeat=repeat, reason=f'Child timeout: {exc.timeout}s'))
                        save()
                        continue
                    (output/f'{prefix}.csv').write_text(run.stdout)
                    (output/f'{prefix}.stderr').write_text(run.stderr)
                    if run.returncode:
                        skips.append(dict(case=case, scale=scale, implementation=implementation, mode=mode,
                                          repeat=repeat, reason=f'Child failed: return code {run.returncode}', log=prefix+'.stderr'))
                        save()
                        continue
                    rows = list(csv.DictReader(io.StringIO(run.stdout)))
                    if len(rows) != sample_count:
                        raise RuntimeError('wrong sample count')
                    item = dict(case=case, implementation=implementation, repeat=repeat, mode=mode,
                                requested_process_scale=scale, visible_processes_before=start_count,
                                **summarize(rows, run.stderr, mode == 'allocations'))
                    summary.append(item)
                    print(f'{prefix}: median={item["median_ns"]/1000:.3f}us units={item["units_min"]}..{item["units_max"]}', flush=True)
                    save()
            group = [s for s in summary if s['case']==case and s['repeat']==repeat and s['requested_process_scale']==scale and s['mode']=='latency']
            z = next((s for s in group if s['implementation']=='zsys'), None)
            s = next((s for s in group if s['implementation']=='sysinfo'), None)
            if z and s:
                equal_units = z['units_min']==z['units_max']==s['units_min']==s['units_max']
                pairs.append(dict(case=case, scale=scale, repeat=repeat,
                                  equal_unit_counts=equal_units, failure_free=z['failures']==s['failures']==0,
                                  warning=None if equal_units else 'Collection counts changed: do not claim strictly matched per-operation work.'))
                save()
    try:
        with sleepers(1) as target:
            for case in args.cases:
                measure(case, 'host', target[0].pid)
            for requested in args.scales:
                additional = max(0, requested-proc_count())
                safe = safe_additional()
                if additional > safe:
                    skips.append(dict(scale=requested, reason='Safety budget exceeded', additional_needed=additional,
                                      safe_additional=safe, policy='Half available memory at 2MiB/child; 64 PID reserve'))
                    save()
                    continue
                try:
                    with sleepers(additional):
                        for case in args.scale_cases:
                            measure(case, requested, target[0].pid)
                except OSError as exc:
                    skips.append(dict(scale=requested, reason=f'OS denied process creation: {exc}'))
                    save()
    finally:
        save()
    print(json.dumps(dict(output=str(output), measurements=len(summary), skips=len(skips))))


if __name__ == '__main__':
    main()
