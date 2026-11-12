# zsys 与 sysinfo 0.40.3：可复现性能报告

测量日期：2026-11-12。本文报告真实采样结果，不把研发目标当成已经达到的性能保证。

**在本地相同 RSS 口径的比较中，zsys 的单进程刷新中位数快 2.86 倍，100 个进程快 2.28 倍，5,000 个进程快 1.88 倍。** 稳定进程集合的热刷新没有观测到 Rust 堆分配。优势并非覆盖所有指标：真实间隔 CPU 采样的 zsys P99 为 514.797 µs，sysinfo 为 231.166 µs；500 个及以上进程的完整刷新没有达到 1 ms 目标。以下提供采样范围、全部分布统计和内存计量边界。

## 1. 证据和版本

| 内容 | 位置 |
|---|---|
| 正式本地统计与环境 | [overview.json](benchmarks/evidence/overview.json) |
| 可分析的完整表格，单位为 ns/B/KiB | [final-table.csv](benchmarks/evidence/final-table.csv) |
| 174 份原始 CSV、stderr、环境与每轮汇总 | [final-raw.tar.gz](benchmarks/evidence/final-raw.tar.gz) |
| 正式文件 SHA-256 | [final-checksums.json](benchmarks/evidence/final-checksums.json) |
| 开发阶段原始数据，含旧 RSS 口径和资源限制记录 | [development-raw.tar.gz](benchmarks/evidence/development-raw.tar.gz)、[development-summary.json](benchmarks/evidence/development-summary.json)、[checksums.json](benchmarks/evidence/checksums.json) |
| 纯解析器改动前后样本 | [parser-ablation.csv](benchmarks/evidence/parser-ablation.csv) |
| 独立测试程序 | [benchmarks/src/main.rs](benchmarks/src/main.rs)、[Cargo.toml](benchmarks/Cargo.toml)、[Cargo.lock](benchmarks/Cargo.lock) |
| 本地最终性能源码 | [b469697](https://github.com/LIghtJUNction/zsys/commit/b469697a422d84b5e75cab2f2316f7f21d6fe036)；网络修复后的重测 [89e6c6c](https://github.com/LIghtJUNction/zsys/commit/89e6c6c020869f3e6b37ea393bfc1040bd336d71) |
| 独立 GitHub Benchmark | [run 43198501217](https://github.com/LIghtJUNction/zsys/actions/runs/43198501217)，其结果单独列出，不混入本地样本 |

固定竞争基准为 **sysinfo `=0.40.3`**，禁用默认特性，启用 `system`、`network`、`disk`。锁文件中的该 crate 校验和为 `a86bac2f7d06e6384984118ad727f5694110f0ec44d924d05c8fdec3d030bee6`。sysinfo 仅属于独立 Benchmark 包，核心库没有它或其他第三方运行时依赖。比较使用其公开 API，没有复制 sysinfo 实现。

正式原始压缩包 SHA-256：`20a27c3ab1ba6463cea71063922417fa1a6abd6f670ef3cab8d8e80c04f82864`。压缩包不包含反复复制的二进制；环境文件记录被冻结测试二进制的 SHA-256。开发记录不参与最终加速比计算。

## 2. 环境与方法

| 项目 | 本地正式测量环境 |
|---|---|
| CPU | Intel Xeon Platinum 8573C；环境可见 9 个逻辑 CPU |
| 内存 | `/proc/meminfo` 的 MemTotal = 10,475,802,624 B；这是采样环境可见值 |
| 系统 | Debian GNU/Linux 13，DEBIAN_VERSION_FULL=13.6 |
| 内核 / 架构 | Linux 6.18.57 / x86_64 |
| 工具链 | rustc 1.99.1，cargo 1.99.1；MSRV 1.95 由独立 CI 验证 |
| 构建 | release，thin LTO，codegen-units=1，RUSTFLAGS 为空 |
| 普通进程测试 | 每个采样实际可见 3 个进程；单 PID 测试只刷新自身 |
| 磁盘容量测试 | 双方完全相同的 11 个已发现挂载点；路径清单留在 stderr |
| 网络 | 受限 namespace；可见 loopback；netlink 不可用，zsys 使用原生只读回退 |
| 干扰控制 | 正式采样期间没有主动并行编译或额外压力任务；云端调度和其他租户不可控 |

每个实现、用例和重复轮次在独立子进程中运行，3 轮交替执行 zsys/sysinfo 的先后顺序。热路径每轮先预热 50 次，然后保留 1,000 个样本；CPU 和基础组合每轮先预热 2 次，再保留 100 个样本，采样前等待 210 ms。冷构造用例不预热。规模测试每轮 100 个样本。所有原始样本合并统计，未剔除异常值。P95/P99 采用 nearest-rank，Median 使用通常的中位数定义。

计时覆盖一次同步刷新及其中分配计数器的开销。210 ms 等待不计入刷新耗时。表中的吞吐量是 `1 / 平均刷新耗时`，**不是**带等待的应用采样频率；本协议中真实间隔 CPU 采样约为每秒 4.76 次。报告提供描述性分布，不提供共享云机器上无法保证的实时性承诺。

“冷启动”指在已经运行的测试进程内新建 collector、第一次刷新、再释放 collector；操作系统缓存是热的。它不代表主机重启后的冷缓存，也不包含每个样本的可执行文件启动成本。`cold_basic` 的首次 CPU 刷新只建立基线，没有有效使用率百分比，不能直接与经过两次有效采样的 paced `basic` 视为同一状态。

### 2.1 内容相等的边界

| 用例 | 双方实际比较范围 | 不包含的内容或限制 |
|---|---|---|
| `cpu` | 全部在线逻辑 CPU 和 aggregate 使用率，真实 210 ms 间隔 | CPU 频率是独立操作 |
| `memory` | RAM total/available/free/used，swap total/free | 不读取每个进程的内存 |
| `process_single` / `process_all` | PID、父 PID、名称、状态、启动身份、累计 CPU、驻留及虚拟内存 | 关闭 sysinfo tasks；无命令行、exe、用户、进程 I/O；热调用不用于证明 CPU 百分比精度 |
| `process_inventory` | PID、父 PID、名称和启动身份 | sysinfo 最小公开刷新；zsys 复用完整核心刷新（stat + statm）；不是纯 PID 枚举 |
| `disk` | 同一组固定挂载点的容量刷新 | 发现挂载点和 block I/O 分开测量 |
| `network` | 同一接口集合的计数器、IP/prefix、MAC、MTU、link speed | 完整元数据校验必须通过才计算比值 |
| `basic` | CPU、RAM/swap、全部进程核心字段 | 不包含 network 和 disk；对应两个实现可配平的 System 范围 |
| `full` | `basic` 加完整 network 元数据和同一组磁盘容量 | 进程和网络校验均通过才运行；本地网络校验失败，因此无本地结果 |
| `enumerate` / `enumerate_std` | zsys 原生 / std 目录扫描得到的 PID 列表 | sysinfo 没有纯 PID-only 公共入口，因此没有与它的加速比 |
| `network_counters` / `disk_io` | zsys 单独的接口计数器 / 全部可见块设备与分区 I/O | 没有经过配平的 sysinfo 对照，不能借它们宣称完整 network/disk 的优势 |
| `cpu_hot` / `basic_hot` | 连续调用开销 | sysinfo 的 CPU 最小刷新间隔可能返回缓存；新鲜度不同，不计算加速比 |

注意：Benchmark 的 `basic` 和库中 `System::refresh_basic` 的范围不同。后者还刷新 network counters 和 block I/O。库接口的准确范围见 [README](README.md) 和 [采样语义](docs/semantics.md)。

### 2.2 RSS 口径修正

早期实现从 `/proc/PID/stat` 取 RSS。复核同一个稳定休眠进程时，本环境重复出现 `stat=160 pages`、`statm/status/smaps=215 pages`；sysinfo 的结果匹配 statm。正式实现因此通过同一个已打开的 PID 目录读取 `statm` 与 `stat`，再用 PID + start ticks 管理身份。旧结果保留为开发证据，不能用旧口径的速度宣传正式版本。

`statm` 的 resident 同样是内核近似计数，并不是精确工作集或 PSS。Linux 文档说明这种计数可能不精确，更精确的 smaps 查询成本更高。相同口径的比较不等于声称测到了精确物理占用。参考 [proc_pid_statm(5)](https://man7.org/linux/man-pages/man5/proc_pid_statm.5.html) 与 [Linux procfs 文档](https://docs.kernel.org/filesystems/proc.html)。

每次正式进程测试先检查完整 PID 集合和一个受控休眠子进程的共同字段，再在原始样本中记录实际进程数。PID/start-time 与固定目录句柄防止把新旧 PID 生命周期拼为一个有效样本；进程退出和权限变化仍通过明确状态返回。

## 3. 本地可配平的主要结果

耗时单位 **µs**；倍数 = sysinfo Median / zsys Median，大于 1 表示 zsys 中位数更快。CPU/basic 各 300 个样本，其余各 3,000 个样本。

| 用例 | zsys Median | sysinfo Median | 中位数倍数 | zsys P99 | sysinfo P99 |
|---|---:|---:|---:|---:|---:|
| CPU，真实间隔 | 42.520 | 75.117 | 1.767× | 514.797 | 231.166 |
| Memory | 3.188 | 5.967 | 1.872× | 12.998 | 20.954 |
| 单进程 | 4.281 | 12.225 | 2.856× | 45.359 | 99.836 |
| 3 进程身份清单 | 16.820 | 35.287 | 2.098× | 85.232 | 184.405 |
| 3 进程完整核心刷新 | 16.857 | 40.676 | 2.413× | 90.308 | 211.068 |
| 11 挂载点容量 | 7.001 | 7.771 | 1.110× | 58.890 | 59.394 |
| CPU + RAM + 3 进程 | 158.252 | 242.611 | 1.533× | 1,453.126 | 3,695.481 |
| 新建 + 单进程 + 释放 | 5.178 | 88.996 | 17.187× | 25.709 | 332.060 |
| 新建 + 3 进程 + 释放 | 18.238 | 121.492 | 6.661× | 147.169 | 721.417 |
| 新建 + 基础组合 + 释放 | 31.132 | 126.521 | 4.064× | 146.057 | 449.892 |

CPU 的中位数更低，但 P99 约为 sysinfo 的 2.23 倍，这是本次测量中的明确退步。频繁无等待 CPU 刷新的 zsys 中位数为 6.177 µs，sysinfo 为 0.052 µs；后者可能返回缓存，所以该对比不能用于判断新鲜数据采集能力，也不能据此声称任何一方同等范围更快。

### 3.1 全进程规模

每一轮、每一个正式样本的进程数均为表中的数量，包括测试器和运行脚本相关进程。耗时单位 **ms**，每个实现每个规模 300 个样本。

| 实际进程数 | zsys Median | sysinfo Median | 中位数倍数 | zsys P99 | sysinfo P99 |
|---:|---:|---:|---:|---:|---:|
| 100 | 0.481722 | 1.096929 | 2.277× | 0.873056 | 2.791228 |
| 500 | 2.972964 | 5.807785 | 1.954× | 5.179038 | 7.511952 |
| 1,000 | 6.116223 | 12.157868 | 1.988× | 12.710857 | 22.666462 |
| 5,000 | 43.247219 | 81.204246 | 1.878× | 93.548041 | 165.681167 |

本地没有正式的 10,000 进程结果。开发阶段尝试创建该规模时，在实际可见 6,340 个进程处触发可用内存保护阈值，停止继续创建；该阶段还是旧 RSS 口径，因此既不能称为“10,000 进程跑通”，也不参与正式速度比较。脚本保留失败原因并清理子进程。

### 3.2 独立路径与消融

完整网络路径在本地只返回 loopback；zsys 使用 ioctl/procfs 回退，sysinfo 返回空接口集合，内容校验失败。因此本地只报告 zsys：完整网络 Median **17.525 µs**、P99 **128.021 µs**，每次 **20 次 / 3,441 B** Rust 分配；只刷新计数器 Median **1.706 µs**、P99 **2.640 µs**，测得零热分配。后一个数字不能冒充完整接口信息的耗时。

纯 PID 枚举在 3 个进程时，原生 getdents64 的 Median 为 **3.780 µs**，std 目录扫描为 **10.766 µs**。5,000 个进程时分别是 **1.681 ms** 和 **2.346 ms**。但是加入逐 PID 文件读取后，5,000 个进程的全刷新分别为 **43.247 ms** 和 **43.824 ms**；1,000 个进程的原生路径还略慢于 std（6.116 vs 6.076 ms）。低层接口的优势主要体现在枚举和分配，不应把它扩张成所有规模都显著提速的结论。

解析器消融采用交替顺序，每个测试进程复用其已读入的固定字节缓冲，每批解析 200,000 次后折算成 ns/次。内存解析的批次折算单次耗时中位数由 1,813 ns 降到 1,352.5 ns，约 1.34 倍；CPU aggregate 首行解析由 99 ns 到 100 ns，没有有意义的改善。进程之间重新读取 procfs，不能声称不同版本使用了逐字节完全相同的系统输入。解析微基准不包含文件读取，不能代替端到端结果；最终 Memory Median 仍然高于 3 µs 目标。消融原始样本见本页证据表。

## 4. 分配与内存

分配器统计成功的 Rust `alloc`、`alloc_zeroed`、`realloc` 请求次数和请求字节；realloc 按新请求大小计量。它不统计 C 库分配、栈、映射或分配器元数据。保留堆为预热后的 setup/collector 请求字节，排除存储计时样本的数组。峰值额外活跃堆是一次调用中相对调用前活跃请求字节的最大增量。RSS 是每个独立子进程的 Linux `VmHWM`，包含程序、加载器、测试器和分配器，不是 collector 独占 RSS。

稳定输入的零热分配不等于零内存，也不保证发现新进程、接口、长字段或读取 owned details 时不分配。zsys 为复用而保留容量；例如 Memory 保留 8,212 B，而 sysinfo 保留 1,056 B、每次另申请 16,635 B。小进程表的 zsys 保留量也可能高于 sysinfo。此处同时展示保留和峰值，避免只挑有利指标。

### 4.1 进程规模的内存比较

“分配/次”为平均次数；B 和 KiB 不能混用。所有 zsys 行均为 0 次 / 0 B 热分配。

| 进程数 | zsys 保留堆 B | sysinfo 保留堆 B | sysinfo 分配/次 | sysinfo 请求 B/次 | zsys VmHWM KiB | sysinfo VmHWM KiB |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 29845 | 58323 | 664.00 | 77,800.00 | 1396–1528 | 1652–1720 |
| 500 | 173541 | 430487 | 2,569.06 | 252,597.32 | 1760–1764 | 2464 |
| 1,000 | 310185 | 856978 | 6,420.13 | 494,179.12 | 1824–1952 | 3312–3384 |
| 5,000 | 1151433 | 3430197 | 35,066.69 | 2,391,146.44 | 3068–3200 | 8732–8744 |

### 4.2 长期复用验证

同一个进程 collector 连续运行 60 秒，本地执行 **10,412,032 次**刷新，未观测到热分配、活跃请求堆增长或文件描述符增长。最新源码的 GitHub CI 也执行了同一 60 秒检查；[43198501256](https://github.com/LIghtJUNction/zsys/actions/runs/43198501256) 记录 **3,968,000 次**，这三个增长指标均为零。另有 100 次创建/释放循环验证资源回收。

这些是有界持续运行证据，不是对任意硬件、任意生命周期长度和所有 workload 的无泄漏证明。CI 提供手动 60–3,600 秒和每周运行的 [资源测试](.github/workflows/soak.yml)。资源测试循环吞吐量不与本页独立 Benchmark 的采样吞吐量混用。

## 5. 研发目标验收

原始目标没有指定使用平均、Median 还是 P99。本报告分别判断，避免用中位数掩盖长尾。以下均为 zsys 本地正式数据，时间单位 µs。

| 范围 | 目标 | Mean | Median | P99 | Mean / Median / P99 达标 |
|---|---:|---:|---:|---:|---|
| 9 CPU + aggregate，真实间隔 | 5.000 | 60.939 | 42.520 | 514.797 | 否 / 否 / 否 |
| RAM/swap | 3.000 | 3.648 | 3.188 | 12.998 | 否 / 否 / 否 |
| 完整 loopback 网络 | 30.000 | 27.970 | 17.524 | 128.021 | 是 / 是 / 否 |
| 11 挂载点容量 | 10.000 | 8.462 | 7.001 | 58.890 | 是 / 是 / 否 |
| 全部块设备/分区 I/O | 10.000 | 16.808 | 14.159 | 77.551 | 否 / 否 / 否 |
| 单进程 | 20.000 | 5.512 | 4.281 | 45.359 | 是 / 是 / 否 |
| 3 进程全刷新 | 1,000.000 | 19.281 | 16.857 | 90.308 | 是 / 是 / 是 |
| CPU/RAM/3 进程，不含网络磁盘 | 1,000.000 | 213.791 | 158.252 | 1,453.126 | 是 / 是 / 否 |
| 100 进程全刷新 | 1,000.000 | 522.269 | 481.721 | 873.056 | 是 / 是 / 是 |
| 500 进程全刷新 | 1,000.000 | 3,051.531 | 2,972.963 | 5,179.038 | 否 / 否 / 否 |
| 1000 进程全刷新 | 1,000.000 | 6,469.022 | 6,116.222 | 12,710.857 | 否 / 否 / 否 |
| 5000 进程全刷新 | 1,000.000 | 46,913.629 | 43,247.219 | 93,548.041 | 否 / 否 / 否 |

包含 CPU、RAM、Process、完整 Network 和 Disk capacity 的 `full` 在本地因为网络内容不一致而未形成有效结果；不能用 `basic` 或计数器子集代替完整组合验收。没有任何“所有功能都达到目标”或“全面超越 sysinfo”的结论。

## 6. 复现

需要 Linux、Rust ≥ 1.95、Python ≥ 3.11。使用新的结果目录保留每次原始记录：

```sh
cargo build --locked --release --manifest-path benchmarks/Cargo.toml
python3 benchmarks/run.py --output benchmark-results/run-01 --samples 1000 --paced-samples 100 --repeats 3 --cases memory cpu process_single process_inventory process_all process_all_std enumerate enumerate_std disk disk_io network network_counters basic full cold_process_single cold_process_all cold_basic cpu_hot basic_hot
python3 benchmarks/scale.py --output benchmark-results/scales-01 --counts 100 500 1000 5000 10000 --samples 100 --repeats 3
python3 benchmarks/summarize.py benchmark-results/run-01 --output benchmark-results/run-01/aggregate.json
for run_dir in benchmark-results/scales-01/[0-9]*; do
  test -f "$run_dir/environment.json" || continue
  python3 benchmarks/summarize.py "$run_dir" --output "$run_dir/aggregate.json"
done
python3 benchmarks/rss_check.py
```

完整参数以 `--help` 为准。脚本在可用内存低于 `max(512 MiB, 初始可用内存的一半)` 时停止继续创建进程，也记录操作系统拒绝创建的错误。默认不绕开宿主 PID/内存限制。失败、内容不匹配和不可用用例保留原因，不转写成零耗时。对比依赖必须保持锁文件版本。

校验/解压正式证据：

```sh
sha256sum benchmarks/evidence/final-raw.tar.gz
mkdir -p /tmp/zsys-evidence
# 先核对输出与本页 SHA-256 一致，再解压。
tar -xzf benchmarks/evidence/final-raw.tar.gz -C /tmp/zsys-evidence
for run_dir in /tmp/zsys-evidence/hot /tmp/zsys-evidence/network-fixed /tmp/zsys-evidence/paced /tmp/zsys-evidence/scaling/[0-9]*; do
  test -f "$run_dir/environment.json" || continue
  python3 benchmarks/summarize.py "$run_dir" --output "$run_dir/aggregate.json"
done
```

CPU 计时必须保留采样间隔。若改为缓存模式或只取 aggregate，应明确标注新范围，单独报告；不能把更少内容的结果覆盖本报告。

## 7. 已知瓶颈与下一阶段实验

1. **每 PID 小文件 I/O 主导大规模刷新。** 原生枚举比 std 更省分配，但在完整刷新中收益有限。后续实验可比较受限 FD 缓存、目标 PID 分组刷新及受控批次，必须同时测 FD/内存和 PID 复用正确性。
2. **CPU 的共享环境长尾仍然偏高。** 需要专用主机、固定 CPU affinity、更多轮次和不同内核，区分调度抢占、procfs 生成成本与用户态解析；当前证据不足以给出硬实时保证。
3. **完整网络元数据有分配和系统调用。** counters/details 已分开；下一步评估复用地址缓冲和显式拓扑变更事件。改变刷新策略时要保持同样新鲜度再比对。
4. **小对象的保留缓冲有空间代价。** 按数据规模选择初始容量、可选显式 shrink，以及更紧凑字段布局需要同时测峰值和尾延迟，不能以持续 realloc 换取漂亮的静态大小。
5. **完整跨平台采集仍未实现。** Windows、macOS、FreeBSD、Android、iOS 的原生接口、数据语义和独立测量是后续工作；本次核心构建通过不代表这些平台具有 Linux 采集能力。

## 8. 全部本地分布统计

以下合并所有正式样本；`hot` 中早于网络修复的两行已排除，改用 `network-fixed`。`different_freshness` 和 `zsys_only` 保留观测值，但不参与与 sysinfo 的加速比。时间单位 **µs**，ops/s 不含采样等待。

| 组/用例 | 实现 | n | Mean | Median | P95 | P99 | Max | ops/s |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| hot/memory | zsys | 3000 | 3.648 | 3.188 | 3.330 | 12.998 | 144.818 | 274,141.4 |
| hot/memory | sysinfo | 3000 | 6.751 | 5.967 | 6.337 | 20.954 | 309.941 | 148,127.4 |
| hot/process_single | zsys | 3000 | 5.512 | 4.281 | 4.554 | 45.359 | 240.839 | 181,414.8 |
| hot/process_single | sysinfo | 3000 | 16.754 | 12.225 | 18.934 | 99.836 | 1,684.298 | 59,687.5 |
| hot/process_inventory | zsys | 3000 | 20.097 | 16.820 | 23.020 | 85.232 | 2,042.872 | 49,758.5 |
| hot/process_inventory | sysinfo | 3000 | 42.916 | 35.287 | 87.557 | 184.405 | 1,091.582 | 23,301.6 |
| hot/process_all | zsys | 3000 | 19.281 | 16.857 | 22.013 | 90.308 | 293.651 | 51,864.9 |
| hot/process_all | sysinfo | 3000 | 51.954 | 40.676 | 101.290 | 211.068 | 1,629.464 | 19,247.8 |
| hot/process_all_std | zsys | 3000 | 27.603 | 23.832 | 43.924 | 104.997 | 561.344 | 36,227.5 |
| hot/enumerate | zsys | 3000 | 4.542 | 3.780 | 3.945 | 19.504 | 296.549 | 220,169.9 |
| hot/enumerate_std | zsys | 3000 | 14.087 | 10.766 | 13.321 | 89.843 | 1,147.753 | 70,987.3 |
| hot/disk | zsys | 3000 | 8.462 | 7.001 | 7.237 | 58.890 | 324.783 | 118,173.3 |
| hot/disk | sysinfo | 3000 | 9.219 | 7.771 | 8.027 | 59.394 | 251.136 | 108,467.0 |
| hot/disk_io | zsys | 3000 | 16.808 | 14.159 | 19.216 | 77.551 | 532.474 | 59,495.1 |
| hot/cold_process_single | zsys | 3000 | 6.067 | 5.178 | 5.329 | 25.709 | 500.000 | 164,822.6 |
| hot/cold_process_single | sysinfo | 3000 | 107.814 | 88.996 | 172.822 | 332.060 | 4,613.988 | 9,275.2 |
| hot/cold_process_all | zsys | 3000 | 25.096 | 18.238 | 41.427 | 147.169 | 2,600.941 | 39,846.7 |
| hot/cold_process_all | sysinfo | 3000 | 165.210 | 121.492 | 283.508 | 721.417 | 4,858.538 | 6,052.9 |
| hot/cold_basic | zsys | 3000 | 38.024 | 31.132 | 73.361 | 146.057 | 604.499 | 26,299.2 |
| hot/cold_basic | sysinfo | 3000 | 154.904 | 126.521 | 257.920 | 449.892 | 3,537.933 | 6,455.6 |
| hot/cpu_hot | zsys | 3000 | 7.601 | 6.177 | 6.734 | 53.495 | 237.662 | 131,559.9 |
| hot/cpu_hot | sysinfo | 3000 | 0.059 | 0.052 | 0.054 | 0.059 | 20.532 | 17,064,167.0 |
| hot/basic_hot | zsys | 3000 | 32.453 | 27.154 | 63.307 | 125.696 | 1,292.565 | 30,813.8 |
| hot/basic_hot | sysinfo | 3000 | 57.332 | 47.127 | 112.720 | 228.040 | 566.753 | 17,442.3 |
| network-fixed/network | zsys | 3000 | 27.970 | 17.524 | 78.734 | 128.021 | 501.089 | 35,752.6 |
| network-fixed/network_counters | zsys | 3000 | 1.937 | 1.706 | 1.816 | 2.640 | 159.400 | 516,319.2 |
| paced/cpu | zsys | 300 | 60.939 | 42.520 | 137.969 | 514.797 | 581.471 | 16,409.9 |
| paced/cpu | sysinfo | 300 | 115.524 | 75.117 | 127.236 | 231.166 | 9,445.561 | 8,656.2 |
| paced/basic | zsys | 300 | 213.791 | 158.252 | 278.566 | 1,453.126 | 5,344.132 | 4,677.5 |
| paced/basic | sysinfo | 300 | 362.349 | 242.611 | 367.832 | 3,695.481 | 6,590.887 | 2,759.8 |
| scaling/100/enumerate | zsys | 300 | 18.261 | 17.233 | 17.660 | 36.097 | 111.995 | 54,762.0 |
| scaling/100/enumerate_std | zsys | 300 | 40.704 | 35.392 | 63.149 | 119.317 | 460.841 | 24,567.8 |
| scaling/100/process_all | zsys | 300 | 522.269 | 481.721 | 737.711 | 873.056 | 898.541 | 1,914.7 |
| scaling/100/process_all | sysinfo | 300 | 1,201.142 | 1,096.929 | 1,726.944 | 2,791.228 | 3,555.155 | 832.5 |
| scaling/100/process_all_std | zsys | 300 | 558.528 | 510.147 | 709.018 | 1,179.884 | 3,141.204 | 1,790.4 |
| scaling/1000/enumerate | zsys | 300 | 243.873 | 179.629 | 350.719 | 2,004.551 | 3,276.206 | 4,100.5 |
| scaling/1000/enumerate_std | zsys | 300 | 357.128 | 336.838 | 497.958 | 765.729 | 992.966 | 2,800.1 |
| scaling/1000/process_all | zsys | 300 | 6,469.022 | 6,116.222 | 8,610.703 | 12,710.857 | 18,499.560 | 154.6 |
| scaling/1000/process_all | sysinfo | 300 | 12,648.535 | 12,157.868 | 15,446.409 | 22,666.462 | 30,831.263 | 79.1 |
| scaling/1000/process_all_std | zsys | 300 | 6,768.411 | 6,075.745 | 8,444.019 | 10,206.795 | 87,581.329 | 147.7 |
| scaling/500/enumerate | zsys | 300 | 86.334 | 72.274 | 148.332 | 267.406 | 876.809 | 11,582.9 |
| scaling/500/enumerate_std | zsys | 300 | 160.259 | 138.027 | 261.855 | 368.275 | 703.655 | 6,239.9 |
| scaling/500/process_all | zsys | 300 | 3,051.531 | 2,972.963 | 3,631.045 | 5,179.038 | 7,519.425 | 327.7 |
| scaling/500/process_all | sysinfo | 300 | 5,899.744 | 5,807.785 | 6,746.771 | 7,511.952 | 8,865.580 | 169.5 |
| scaling/500/process_all_std | zsys | 300 | 3,137.994 | 2,983.497 | 3,653.050 | 6,157.765 | 8,815.767 | 318.7 |
| scaling/5000/enumerate | zsys | 300 | 1,818.527 | 1,680.911 | 2,863.807 | 3,328.825 | 5,405.244 | 549.9 |
| scaling/5000/enumerate_std | zsys | 300 | 2,566.966 | 2,345.666 | 3,870.551 | 6,242.447 | 12,515.949 | 389.6 |
| scaling/5000/process_all | zsys | 300 | 46,913.629 | 43,247.219 | 80,089.957 | 93,548.041 | 201,475.801 | 21.3 |
| scaling/5000/process_all | sysinfo | 300 | 86,400.890 | 81,204.246 | 110,347.491 | 165,681.167 | 436,399.050 | 11.6 |
| scaling/5000/process_all_std | zsys | 300 | 45,465.089 | 43,824.365 | 57,076.688 | 67,691.276 | 84,161.843 | 22.0 |

## 9. 全部本地分配与峰值统计

每行与上表的同名样本完全对应。保留堆为调用后范围；调用前范围也保存在 overview.json，正式稳定热用例未增长。冷构造在释放后保留为 0，不代表构造过程中不申请内存。

| 组/用例 | 实现 | 分配/次 | 请求 B/次 | 最大额外活跃 B | 保留堆 B | VmHWM KiB |
|---|---|---:|---:|---:|---:|---:|
| hot/memory | zsys | 0.00 | 0.00 | 0 | 8212 | 1464–1528 |
| hot/memory | sysinfo | 1.00 | 16,635.00 | 16,635 | 1056 | 1476–1544 |
| hot/process_single | zsys | 0.00 | 0.00 | 0 | 4648 | 1480–1544 |
| hot/process_single | sysinfo | 12.00 | 33,326.00 | 33,245 | 6322 | 1588–1652 |
| hot/process_inventory | zsys | 0.00 | 0.00 | 0 | 8771 | 1484–1552 |
| hot/process_inventory | sysinfo | 85.00 | 34,671.00 | 33,694 | 2756 | 1624–1628 |
| hot/process_all | zsys | 0.00 | 0.00 | 0 | 8771 | 1488–1552 |
| hot/process_all | sysinfo | 85.00 | 34,671.00 | 33,694 | 6347 | 1584–1652 |
| hot/process_all_std | zsys | 110.00 | 813.00 | 78 | 4675 | 1496–1560 |
| hot/enumerate | zsys | 0.00 | 0.00 | 0 | 8213 | 1356–1548 |
| hot/enumerate_std | zsys | 110.00 | 813.00 | 78 | 4117 | 1480–1484 |
| hot/disk | zsys | 0.00 | 0.00 | 0 | 1572 | 1612–1616 |
| hot/disk | sysinfo | 22.00 | 723.00 | 94 | 3339 | 1552–1556 |
| hot/disk_io | zsys | 0.00 | 0.00 | 0 | 23738 | 1504–1572 |
| hot/cold_process_single | zsys | 4.00 | 4,648.00 | 4,648 | 0 | 1484–1548 |
| hot/cold_process_single | sysinfo | 78.00 | 92,531.00 | 35,915 | 0 | 1656–1664 |
| hot/cold_process_all | zsys | 8.00 | 8,771.00 | 8,771 | 0 | 1548 |
| hot/cold_process_all | sysinfo | 155.00 | 93,901.00 | 35,982 | 0 | 1612–1672 |
| hot/cold_basic | zsys | 19.00 | 27,897.00 | 26,725 | 0 | 1568–1576 |
| hot/cold_basic | sysinfo | 156.00 | 110,536.00 | 39,573 | 0 | 1632–1636 |
| hot/cpu_hot | zsys | 0.00 | 0.00 | 0 | 11278 | 1532–1536 |
| hot/cpu_hot | sysinfo | 0.00 | 0.00 | 0 | 4647 | 1552–1556 |
| hot/basic_hot | zsys | 0.00 | 0.00 | 0 | 28261 | 1504–1572 |
| hot/basic_hot | sysinfo | 85.00 | 51,299.00 | 33,695 | 6348 | 1572 |
| network-fixed/network | zsys | 20.00 | 3,441.00 | 2,626 | 10750 | 1568–1572 |
| network-fixed/network_counters | zsys | 0.00 | 0.00 | 0 | 10668 | 1548–1552 |
| paced/cpu | zsys | 0.00 | 0.00 | 0 | 11278 | 1476–1480 |
| paced/cpu | sysinfo | 12.00 | 9,671.00 | 9,201 | 4647 | 1492–1496 |
| paced/basic | zsys | 0.00 | 0.00 | 0 | 28261 | 1512–1516 |
| paced/basic | sysinfo | 98.00 | 60,977.00 | 33,694 | 6347 | 1508–1512 |
| scaling/100/enumerate | zsys | 0.00 | 0.00 | 0 | 12805 | 1308–1496 |
| scaling/100/enumerate_std | zsys | 304.00 | 1,294.00 | 78 | 4613 | 1500–1504 |
| scaling/100/process_all | zsys | 0.00 | 0.00 | 0 | 29845 | 1396–1528 |
| scaling/100/process_all | sysinfo | 664.00 | 77,800.00 | 33,695 | 58323 | 1652–1720 |
| scaling/100/process_all_std | zsys | 304.00 | 1,294.00 | 78 | 21653 | 1524–1528 |
| scaling/1000/enumerate | zsys | 0.00 | 0.00 | 0 | 40965 | 1516–1520 |
| scaling/1000/enumerate_std | zsys | 2,104.00 | 9,144.00 | 78 | 8197 | 1420–1548 |
| scaling/1000/process_all | zsys | 0.00 | 0.00 | 0 | 310185 | 1824–1952 |
| scaling/1000/process_all | sysinfo | 6,420.13 | 494,179.12 | 33,712 | 856978 | 3312–3384 |
| scaling/1000/process_all_std | zsys | 2,104.00 | 9,144.00 | 78 | 277417 | 1824–1952 |
| scaling/500/enumerate | zsys | 0.00 | 0.00 | 0 | 38917 | 1328–1524 |
| scaling/500/enumerate_std | zsys | 1,104.00 | 4,294.00 | 78 | 6149 | 1452–1516 |
| scaling/500/process_all | zsys | 0.00 | 0.00 | 0 | 173541 | 1760–1764 |
| scaling/500/process_all | sysinfo | 2,569.06 | 252,597.32 | 33,695 | 430487 | 2464 |
| scaling/500/process_all_std | zsys | 1,104.00 | 4,294.00 | 78 | 140773 | 1612–1740 |
| scaling/5000/enumerate | zsys | 0.00 | 0.00 | 0 | 69637 | 1500–1564 |
| scaling/5000/enumerate_std | zsys | 10,104.00 | 45,790.00 | 78 | 36869 | 1440–1568 |
| scaling/5000/process_all | zsys | 0.00 | 0.00 | 0 | 1151433 | 3068–3200 |
| scaling/5000/process_all | sysinfo | 35,066.69 | 2,391,146.44 | 33,712 | 3430197 | 8732–8744 |
| scaling/5000/process_all_std | zsys | 10,104.00 | 45,790.00 | 78 | 1118665 | 3128–3188 |

## 10. 独立 GitHub 环境

[Benchmark 43198501217](https://github.com/LIghtJUNction/zsys/actions/runs/43198501217) 已成功执行，源码为 clean `89e6c6c020869f3e6b37ea393bfc1040bd336d71`。原始 artifact 的 165 份样本文件、80,300 个样本已经核对 SHA-256 和行数；同一个冻结二进制用于全部远端组。该环境的数据与前述本地环境分开报告，不把两台机器的样本混合成一个百分位。

| 证据 | 文件 |
|---|---|
| 未改字节的 GitHub 原始 artifact | [github-89e6c6c.zip](benchmarks/evidence/github-89e6c6c.zip) |
| 环境、范围校验、失败和完整统计 | [github-89e6c6c-summary.json](benchmarks/evidence/github-89e6c6c-summary.json) |
| 55 行可分析表格 | [github-89e6c6c-table.csv](benchmarks/evidence/github-89e6c6c-table.csv) |
| 文件校验和 | [github-89e6c6c-checksums.json](benchmarks/evidence/github-89e6c6c-checksums.json) |

ZIP 为 523,591 B，SHA-256 `690cc53c9577c1f745c993b312f7e3a3010408d592c92990c67b32b7db1499d0`，与 GitHub artifact API 的 digest 一致。运行使用 AMD EPYC 7763、4 个 vCPU、16,760,348,672 B RAM、Ubuntu 24.04.5、Linux `7.0.0-1017-azure`、Rust 1.99.1，release/thin LTO/codegen-units=1；采样间隔、统计方法和分配计量边界与前述协议相同。

### 10.1 已通过范围校验的比较

Network 和 Process 的内容门禁都通过。`full` 的两个实现每个样本均采到 **163 个 PID**，同样刷新 `/`、`/boot`、`/boot/efi` 三个挂载点。完整 Network 包括 IP/prefix、MAC、MTU、link speed 和流量；不能把该环境的完整采集延迟与本地受限 loopback 或 counters-only 数字替换。

下表单位为 **µs**。CPU/full 各 300 个样本，其他行各 3,000 个样本。

| 范围 | zsys Median | sysinfo Median | 中位数倍数 | zsys P99 | sysinfo P99 |
|---|---:|---:|---:|---:|---:|
| RAM/swap | 9.808 | 15.338 | 1.564× | 18.284 | 37.290 |
| CPU，真实间隔 | 21.580 | 32.416 | 1.502× | 28.492 | 50.273 |
| 完整 Network | 917.941 | 1,191.003 | 1.297× | 1,057.219 | 1,291.553 |
| 3 挂载点容量 | 3.246 | 3.397 | 1.047× | 3.497 | 3.817 |
| 单 PID | 14.777 | 38.461 | 2.603× | 26.839 | 56.194 |
| 168 PID 核心全刷新 | 2,373.688 | 4,904.980 | 2.066× | 2,494.714 | 5,047.773 |
| 163 PID + CPU/RAM/Network/Disk | 3,645.863 | 6,382.846 | 1.751× | 4,224.713 | 7,662.112 |

完整 Network 中位数约快 1.30 倍，`full` 约快 1.75 倍，但 **917.941 µs 的 Network 和 3.646 ms 的 full 均没有达到 30 µs / 1 ms 目标**。远端 CPU P99 优于对照，仍不能覆盖本地 CPU P99 更差的事实。远端 full 的 P99 较低，但最大值为 16.894 ms，对照为 8.104 ms；不存在对所有长尾指标都更优的结论。

### 10.2 规模与失败记录

| 请求档位 | zsys 实际 PID | sysinfo 实际 PID | zsys Median ms | sysinfo Median ms | 可以发布倍率吗 |
|---:|---:|---:|---:|---:|---|
| 100 | 164 | 164 | 2.319203 | 4.745185 | 2.046×，只标为 164 PID |
| 500 | 500 | 500 | 7.330057 | 15.534241 | 2.119× |
| 1,000 | 1000 | 1000 | 14.915010 | 31.954928 | 2.142× |
| 5,000 | 4997–5000 | 4998–5000 | 80.875973 | 171.013421 | 否，实际数量范围不同 |
| 10,000 | 10000 | 9997–10000 | 168.027108 | 346.490234 | 否，实际数量范围不同 |

10,000 档确实完成创建与采样，zsys 的每个样本均为 10,000 个 PID；sysinfo 为 9,997–10,000。原始读数完整保留，但本报告没有为 5,000/10,000 两个漂移档生成严格同范围倍率。请求 100 档时 runner 原有进程已经超过 100，所以只能标为实际 164 PID；它不是 100 PID 成绩。本地固定 100/500/1,000/5,000 的结果仍在第 3 节。

**Workflow success 不代表每个 benchmark repeat 都成功。** 远端 zsys `cold_basic` 第 0 轮遇到部分进程扫描，被公平性检查拒绝；该轮返回 101，原始 stderr 与空样本的 hash 都保留。其余两轮可作为描述性观测，但不计算这个用例的总体加速比。远端 `basic` 的 PID 范围分别为 168–172 与 160–168，也没有给出倍率。高频 CPU 缓存路径仍因新鲜度不同排除。

### 10.3 内存的实际权衡

完整 Network 的每次 Rust 分配次数为 **35 对 134**，但请求字节为 **17,943 B 对 9,129 B**、保留堆为 **12,793 B 对 1,671 B**。因此 zsys 减少了分配次数，却在这条路径上申请并保留更多字节。它的完整 Network 不是零分配，也不是所有内存指标都更低。

实际 runner 上的进程字段发生变化，zsys 的全表/full 路径有小量非零分配。例如 168 PID 全刷新平均 0.001333 次、0.048 B/次，不能把这些舍入成“所有实际工作负载都零分配”。500 PID 保留堆为 175,384–175,408 B，对照 429,158–429,176 B；1,000 PID 为 312,058–312,084 B，对照 855,479–855,498 B。

10,000 档的独立内存观测为：zsys 保留 2,267,808–2,267,833 B、VmHWM 5,716–5,808 KiB；sysinfo 保留 6,862,655–6,862,720 B、VmHWM 16,692–16,824 KiB。它们仍受上述 PID 数量漂移限制，不能称为严格相同对象集合的内存实验。VmHWM 与 Rust 请求字节的计量范围继续按第 4 节解释。

### 10.4 全部远端延迟统计

下面保留所有成功轮次的观测，包括明确不适合生成倍率的用例。`cold_basic/zsys` 只有两轮成功、2,000 个样本；失败并未从结果叙述中消失。时间单位 **µs**。

| 组/用例 | 实现 | n | Mean | Median | P95 | P99 | Max | ops/s |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| main/memory | zsys | 3000 | 10.015 | 9.808 | 10.049 | 18.284 | 321.892 | 99,849.8 |
| main/memory | sysinfo | 3000 | 15.876 | 15.338 | 16.080 | 37.290 | 89.958 | 62,987.4 |
| main/cpu | zsys | 300 | 21.665 | 21.580 | 24.185 | 28.492 | 32.460 | 46,156.6 |
| main/cpu | sysinfo | 300 | 33.214 | 32.416 | 39.623 | 50.273 | 53.269 | 30,107.8 |
| main/cpu_hot | zsys | 3000 | 10.975 | 10.469 | 15.730 | 19.246 | 51.545 | 91,114.5 |
| main/cpu_hot | sysinfo | 3000 | 0.086 | 0.070 | 0.081 | 0.091 | 25.126 | 11,592,948.4 |
| main/network | zsys | 3000 | 926.838 | 917.941 | 1,014.850 | 1,057.219 | 1,186.459 | 1,078.9 |
| main/network | sysinfo | 3000 | 1,190.020 | 1,191.003 | 1,255.847 | 1,291.553 | 4,427.135 | 840.3 |
| main/network_counters | zsys | 3000 | 9.618 | 9.377 | 9.497 | 14.928 | 39.132 | 103,972.6 |
| main/disk | zsys | 3000 | 3.329 | 3.246 | 3.307 | 3.497 | 28.413 | 300,418.5 |
| main/disk | sysinfo | 3000 | 3.497 | 3.397 | 3.467 | 3.817 | 41.456 | 285,992.0 |
| main/disk_io | zsys | 3000 | 19.823 | 19.396 | 19.877 | 38.721 | 55.012 | 50,446.8 |
| main/process_single | zsys | 3000 | 15.247 | 14.777 | 19.706 | 26.839 | 61.413 | 65,588.4 |
| main/process_single | sysinfo | 3000 | 39.230 | 38.461 | 41.627 | 56.194 | 76.902 | 25,490.7 |
| main/process_all | zsys | 3000 | 2,382.474 | 2,373.688 | 2,430.765 | 2,494.714 | 3,199.729 | 419.7 |
| main/process_all | sysinfo | 3000 | 4,914.877 | 4,904.980 | 4,966.894 | 5,047.773 | 6,143.492 | 203.5 |
| main/enumerate | zsys | 3000 | 77.469 | 76.412 | 85.839 | 94.946 | 108.330 | 12,908.4 |
| main/enumerate_std | zsys | 3000 | 92.772 | 91.299 | 101.738 | 113.239 | 143.546 | 10,779.1 |
| main/basic | zsys | 300 | 2,676.348 | 2,668.216 | 2,870.280 | 2,996.624 | 3,989.073 | 373.6 |
| main/basic | sysinfo | 300 | 5,222.052 | 5,240.248 | 5,423.929 | 5,641.326 | 6,705.764 | 191.5 |
| main/basic_hot | zsys | 3000 | 2,325.968 | 2,319.280 | 2,358.032 | 2,411.870 | 3,483.999 | 429.9 |
| main/basic_hot | sysinfo | 3000 | 4,789.756 | 4,788.704 | 4,850.994 | 4,971.247 | 6,886.250 | 208.8 |
| main/cold_process_single | zsys | 3000 | 18.025 | 17.062 | 22.091 | 37.419 | 52.857 | 55,477.6 |
| main/cold_process_single | sysinfo | 3000 | 142.054 | 138.848 | 155.438 | 172.019 | 235.346 | 7,039.6 |
| main/cold_process_all | zsys | 3000 | 2,316.145 | 2,310.117 | 2,350.299 | 2,450.363 | 3,057.568 | 431.8 |
| main/cold_process_all | sysinfo | 3000 | 5,003.155 | 4,983.404 | 5,082.412 | 5,338.691 | 6,576.032 | 199.9 |
| main/cold_basic | sysinfo | 3000 | 5,046.040 | 5,034.102 | 5,111.138 | 5,205.373 | 6,505.814 | 198.2 |
| main/cold_basic | zsys | 2000 | 2,368.677 | 2,358.532 | 2,413.183 | 2,511.396 | 3,564.177 | 422.2 |
| main/full | zsys | 300 | 3,714.052 | 3,645.863 | 3,942.419 | 4,224.713 | 16,893.841 | 269.2 |
| main/full | sysinfo | 300 | 6,412.144 | 6,382.846 | 6,660.035 | 7,662.112 | 8,103.787 | 156.0 |
| scaling/100/enumerate | zsys | 300 | 76.955 | 75.230 | 92.451 | 99.083 | 111.247 | 12,994.7 |
| scaling/100/enumerate_std | zsys | 300 | 92.170 | 89.936 | 111.747 | 117.007 | 121.966 | 10,849.6 |
| scaling/100/process_all | zsys | 300 | 2,342.820 | 2,319.204 | 2,441.855 | 2,938.236 | 3,333.628 | 426.8 |
| scaling/100/process_all | sysinfo | 300 | 4,756.886 | 4,745.185 | 4,799.064 | 4,844.900 | 5,880.158 | 210.2 |
| scaling/100/process_all_std | zsys | 300 | 2,315.350 | 2,309.194 | 2,352.610 | 2,373.769 | 2,382.365 | 431.9 |
| scaling/500/enumerate | zsys | 300 | 208.626 | 200.161 | 263.839 | 277.394 | 289.677 | 4,793.3 |
| scaling/500/enumerate_std | zsys | 300 | 235.864 | 231.920 | 252.978 | 262.807 | 268.728 | 4,239.7 |
| scaling/500/process_all | zsys | 300 | 7,365.332 | 7,330.057 | 7,448.440 | 8,305.456 | 9,146.006 | 135.8 |
| scaling/500/process_all | sysinfo | 300 | 15,567.570 | 15,534.241 | 15,693.652 | 16,320.303 | 17,536.857 | 64.2 |
| scaling/500/process_all_std | zsys | 300 | 7,425.185 | 7,417.900 | 7,486.607 | 7,772.888 | 9,217.078 | 134.7 |
| scaling/1000/enumerate | zsys | 300 | 393.356 | 392.238 | 410.431 | 420.981 | 433.073 | 2,542.2 |
| scaling/1000/enumerate_std | zsys | 300 | 447.937 | 444.009 | 464.381 | 481.312 | 492.533 | 2,232.5 |
| scaling/1000/process_all | zsys | 300 | 15,024.802 | 14,915.010 | 15,110.837 | 19,442.448 | 21,210.436 | 66.6 |
| scaling/1000/process_all | sysinfo | 300 | 31,952.736 | 31,954.928 | 32,450.912 | 33,496.551 | 34,123.233 | 31.3 |
| scaling/1000/process_all_std | zsys | 300 | 14,966.013 | 14,967.517 | 15,141.106 | 15,369.891 | 16,401.326 | 66.8 |
| scaling/5000/enumerate | zsys | 300 | 1,912.633 | 1,907.336 | 1,948.493 | 1,973.960 | 1,999.658 | 522.8 |
| scaling/5000/enumerate_std | zsys | 300 | 2,167.526 | 2,161.923 | 2,194.369 | 2,212.482 | 2,820.760 | 461.4 |
| scaling/5000/process_all | zsys | 300 | 81,341.842 | 80,875.973 | 83,718.263 | 102,957.769 | 117,107.646 | 12.3 |
| scaling/5000/process_all | sysinfo | 300 | 174,598.160 | 171,013.421 | 200,098.170 | 223,495.735 | 241,787.268 | 5.7 |
| scaling/5000/process_all_std | zsys | 300 | 83,937.847 | 81,678.266 | 102,074.617 | 115,862.404 | 117,001.734 | 11.9 |
| scaling/10000/enumerate | zsys | 300 | 4,362.836 | 4,039.340 | 5,341.174 | 5,368.915 | 5,444.364 | 229.2 |
| scaling/10000/enumerate_std | zsys | 300 | 4,548.274 | 4,352.572 | 6,006.420 | 6,058.717 | 6,168.129 | 219.9 |
| scaling/10000/process_all | zsys | 300 | 176,177.413 | 168,027.109 | 215,469.352 | 228,713.588 | 235,486.658 | 5.7 |
| scaling/10000/process_all | sysinfo | 300 | 359,715.015 | 346,490.235 | 435,479.237 | 456,324.882 | 489,370.283 | 2.8 |
| scaling/10000/process_all_std | zsys | 300 | 175,065.131 | 168,641.917 | 214,072.415 | 223,598.749 | 230,953.481 | 5.7 |

### 10.5 全部远端分配与峰值统计

平均次数和字节保留小数，避免把小量非零分配写成零。新对象和字段变化引起的保留量变化，原始前后范围都在 summary 中。

| 组/用例 | 实现 | 分配/次 | 请求 B/次 | 最大额外活跃 B | 保留堆 B | VmHWM KiB |
|---|---|---:|---:|---:|---:|---:|
| main/memory | zsys | 0 | 0 | 0 | 8212 | 2516–2584 |
| main/memory | sysinfo | 1 | 16,635 | 16,635 | 1056 | 2472–2572 |
| main/cpu | zsys | 0 | 0 | 0 | 8974 | 2524–2592 |
| main/cpu | sysinfo | 7 | 9,304.71 | 9,128 | 1244 | 2460 |
| main/cpu_hot | zsys | 0 | 0 | 0 | 8974 | 2628–2648 |
| main/cpu_hot | sysinfo | 0 | 0 | 0 | 1244 | 2520–2584 |
| main/network | zsys | 35 | 17,943 | 5,065 | 12793 | 2616–2660 |
| main/network | sysinfo | 134 | 9,129 | 4,322 | 1671 | 2536–2600 |
| main/network_counters | zsys | 0 | 0 | 0 | 12570 | 2588–2652 |
| main/disk | zsys | 0 | 0 | 0 | 378 | 2564–2580 |
| main/disk | sysinfo | 6 | 51 | 18 | 792 | 2504–2584 |
| main/disk_io | zsys | 0 | 0 | 0 | 15986 | 2596–2656 |
| main/process_single | zsys | 0 | 0 | 0 | 4648 | 2576–2640 |
| main/process_single | sysinfo | 13 | 33,354 | 33,251 | 2921 | 2600–2644 |
| main/process_all | zsys | 0.001333 | 0.048 | 36 | 49156–49179 | 2652–2716 |
| main/process_all | sysinfo | 1,112.048 | 109,485.52 | 33,712 | 110135–110148 | 2756–2824 |
| main/enumerate | zsys | 0 | 0 | 0 | 13317 | 2512–2528 |
| main/enumerate_std | zsys | 462 | 2,069 | 88 | 5125 | 2468–2528 |
| main/basic | zsys | 0.073333 | 2.556667 | 77 | 66375–66402 | 2604–2656 |
| main/basic | sysinfo | 1,111.826667 | 134,623.316667 | 34,906 | 109911–110126 | 2776–2836 |
| main/basic_hot | zsys | 0.004 | 0.164 | 36 | 66178–66196 | 2664–2672 |
| main/basic_hot | sysinfo | 1,080.047333 | 123,509.800333 | 33,712 | 109915 | 2796–2844 |
| main/cold_process_single | zsys | 4 | 4,648 | 4,648 | 0 | 2576–2644 |
| main/cold_process_single | sysinfo | 55 | 68,322 | 35,921 | 0 | 2612–2656 |
| main/cold_process_all | zsys | 180.511667 | 86,623.618667 | 64,789 | 0 | 2708–2780 |
| main/cold_process_all | sysinfo | 1,458 | 553,585.071 | 280,935 | 0 | 2872–2964 |
| main/cold_basic | sysinfo | 1,465 | 570,577.080667 | 281,152 | 0 | 2864–2972 |
| main/cold_basic | zsys | 190 | 103,476.9545 | 81,596 | 0 | 2728–2732 |
| main/full | zsys | 35.036667 | 17,944.44 | 5,142 | 79373–79454 | 2664–2688 |
| main/full | sysinfo | 1,234 | 142,284.36 | 33,712 | 112423–112439 | 2776–2836 |
| scaling/100/enumerate | zsys | 0 | 0 | 0 | 13317 | 2416–2472 |
| scaling/100/enumerate_std | zsys | 454 | 2,065 | 88 | 5125 | 2412–2476 |
| scaling/100/process_all | zsys | 0.006667 | 0.34 | 33 | 49014–49060 | 2580–2652 |
| scaling/100/process_all | sysinfo | 1,093.04 | 107,786.64 | 33,712 | 110000–110024 | 2760 |
| scaling/100/process_all_std | zsys | 454.016667 | 2,065.7 | 88 | 40816–40882 | 2572–2652 |
| scaling/500/enumerate | zsys | 0 | 0 | 0 | 38917 | 2436–2492 |
| scaling/500/enumerate_std | zsys | 1,126 | 5,089 | 88 | 6149 | 2444–2492 |
| scaling/500/process_all | zsys | 0.01 | 0.36 | 18 | 175384–175408 | 2792–2856 |
| scaling/500/process_all | sysinfo | 3,445.16 | 266,536.56 | 33,712 | 429158–429176 | 3444–3524 |
| scaling/500/process_all_std | zsys | 1,126.01 | 5,089.36 | 88 | 142596–142644 | 2772–2836 |
| scaling/1000/enumerate | zsys | 0 | 0 | 0 | 40965 | 2492 |
| scaling/1000/enumerate_std | zsys | 2,126 | 9,589 | 88 | 8197 | 2460–2564 |
| scaling/1000/process_all | zsys | 0.023333 | 0.906667 | 36 | 312058–312084 | 2964–3056 |
| scaling/1000/process_all | sysinfo | 6,945.28 | 502,530.48 | 33,712 | 855479–855498 | 4236–4284 |
| scaling/1000/process_all_std | zsys | 2,126.003333 | 9,589.12 | 88 | 279266–279296 | 2948–3064 |
| scaling/5000/enumerate | zsys | 0 | 0 | 0 | 69637 | 2468–2536 |
| scaling/5000/enumerate_std | zsys | 10,126 | 45,589 | 88 | 36869 | 2476–2604 |
| scaling/5000/process_all | zsys | 0.013333 | 0.356667 | 18 | 1153242–1153330 | 4180–4252 |
| scaling/5000/process_all | sysinfo | 34,943.366667 | 2,389,362.866667 | 33,712 | 3428294–3428394 | 9684–9788 |
| scaling/5000/process_all_std | zsys | 10,122.68 | 45,592.44 | 88 | 1120499–1120553 | 4164–4224 |
| scaling/10000/enumerate | zsys | 0 | 0 | 0 | 102405 | 2492–2624 |
| scaling/10000/enumerate_std | zsys | 20,126 | 108,229 | 88 | 69637 | 2544–2624 |
| scaling/10000/process_all | zsys | 0.006667 | 0.206667 | 18 | 2267808–2267833 | 5716–5808 |
| scaling/10000/process_all | sysinfo | 69,951.843333 | 4,775,108.763333 | 34,919 | 6862655–6862720 | 16692–16824 |
| scaling/10000/process_all_std | zsys | 20,125.083333 | 108,230.766667 | 88 | 2235064–2235085 | 5700–5804 |
