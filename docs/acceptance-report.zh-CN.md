# zsys 0.1.0 候选版本验收报告

**记录日期：2026-11-12（UTC）。状态：Linux 工程与本地性能验收已形成证据，发布闭环尚待完成。**

zsys 已具备实际可用的 Linux CPU、内存、进程、磁盘、网络采集及可选硬件信息，核心和 Linux 后端没有第三方运行时 crate。指定原生代码提交的两次 GitHub CI 均有 11 个 job 成功，本地公平基准已覆盖 100、500、1,000、5,000 个实际进程。现有结果支持部分路径的延迟与分配优势，同时也显示 CPU P99、固定缓冲占用和大规模刷新目标仍有不足。crates.io 和 GitHub Release 尚未确认成功，故本报告不宣布最终验收全部完成。[CI 证据](#3-测试ci-与资源验证)、[基准证据](#4-基准环境方法与公平性)、[发布状态](#9-版本发布与交付位置)分别给出可核查依据。

## 1. 交付范围与验收边界

| 验收项 | 当前状态 | 证据与边界 |
|---|---|---|
| Linux 核心功能实际可用 | 已实现并测试 | 独立 collector、`System`、真实 Linux smoke 与 fixture |
| CPU／Memory／Process／Disk／Network | 已实现 | 能力和失败语义见下表及 [API 文档](semantics.md) |
| 核心 `no_std` | 已编译验证 | Rust 1.95、bare-metal、WASM；不包含原生 OS 采集 |
| 运行时第三方依赖为零 | 已校验 | [Cargo.toml](../Cargo.toml)、[包检查](../scripts/check-package.py) |
| 单元／集成／文档测试 | 已通过指定提交的 CI | Linux 55 项单元／集成测试＋2 项文档测试 |
| 可复现 Benchmark、真实性能和内存测量 | 本地已完成；独立远端运行待检查 | 固定 sysinfo `=0.40.3`、原始数据、环境及 checksum |
| 文档与示例 | 已提供 | README、语义、平台、架构、基准、报告；README 两个完整 Rust 示例已编译 |
| 跨平台 CI | 已通过指定提交的 11-job 矩阵 | Windows/macOS 只验证 core；Linux 后端在原生 Linux 运行 |
| crates.io 发布 | **待确认** | 发布工作流已实现，实际成功必须以 registry 校验为准 |
| GitHub Release | **待确认** | 必须与 crate 版本、Tag、候选提交一致 |
| 全平台原生后端 | **未完成** | Windows/macOS 及后续平台仍待开发 |

当前阶段完成的是 Linux 实现与有明确范围的测量。用户设定的最终验收包含两个实际发布结果，这些条件未核验之前，版本仍是候选。进度以 [progress.md](progress.md) 和对应 Actions 运行记录更新。

## 2. 实际实现的功能

### 2.1 原子采集与核心模型

| 模块 | 已实现能力 | 使用边界 |
|---|---|---|
| Core | 计数器模型、时间戳与序号、差分、CPU 公式、`ProcessId`、`Cadence`、借用字节解析 | 默认关闭功能时不需要 `std` 或全局分配器；`alloc` 当前不增加独立 collector |
| CPU | 聚合与在线逻辑 CPU 时间、使用率、在线集合变化、按需 sysfs 频率 | 首次／回退／零总增量为 `None`；频率另行刷新 |
| Memory | 总量、空闲、可用、使用量、缓存相关字段与 Swap；旧内核可用量估计标记 | 单位为字节；不会自动换成 cgroup 配额 |
| Process | PID／PPID／名称／状态／线程数／启动 tick、CPU 累计值与利用率、RSS／虚拟内存；完整枚举与单 PID 更新 | 默认 RSS 来自 `statm`；不展开每个 task 为独立对象 |
| Process details | 可执行文件、保留原始字节的命令行、四组 UID／GID、字段级失败状态 | 按需拥有型结果；不查询用户名数据库 |
| Disk | 块设备／分区 I/O、挂载点／文件系统／源设备／选项、容量／可用空间／inode | 容量独立调用，当前只接受 Linux 64 位 ABI；远程文件系统可能阻塞 |
| Network | 十六类 `/proc/net/dev` 累计计数器、差分、IPv4／IPv6／掩码／scope、ifindex／flags、MAC／MTU／可用链路速率 | 计数器与详情独立；完整详情依赖 namespace、原生调用权限和驱动 |
| Hardware | DMI 描述、thermal／hwmon inventory 与读数、DRM primary card、PCI ID、可选驱动 VRAM／busy 数值 | hwmon 保留 raw 单位；GPU 并非通用厂商遥测；缺失显示 `Unavailable` |
| `System` | 延迟初始化、独立刷新及借用 getter；显式 `refresh_basic` | 按顺序组合 CPU／内存／进程／网络计数器／块 I/O；不提供跨模块原子快照 |

这些功能由 [src](../src/)、[examples](../examples/) 和 [公共 API 索引](semantics.md#公共-api-索引) 对应。调用者可以只实例化一个 collector；没有隐藏后台线程或强制统一刷新周期。共享对象的并发刷新由 Rust 的 `&mut self` 与调用者同步约束，独立对象可在线程间使用。

### 2.2 正确性优先的处理

进程身份由 PID 和启动 tick 组成，区分同次系统启动中的 PID 复用。每个 PID 保留自己的前次成功采样时间，进程 CPU 百分比使用真实的样本间隔；多线程进程可以超过 100%。读取失败保留旧成功数据并标记不可访问、已消失或无效，清除不能继续使用的 CPU 百分比。详情通过固定 PID 目录与前后身份检查避免把两个生命周期的数据拼在一起。[tests/processes.rs](../tests/processes.rs) 覆盖这些行为。

公平性验证曾发现 `/proc/PID/stat` resident 与 `statm`／status 的 RSS 观测不同，因此默认 `memory_bytes` 改为同一打开 PID 目录下的 `statm` resident×页大小，保留 `counters.resident_pages` 作为原始 `/stat` 字段。`statm` 缺失、损坏或权限错误会使本次更新失败，不会生成虚假的零内存。两条路径仍是 Linux 的近似 RSS，均不等价于 PSS 或逐映射精确统计；参见 [proc_pid_statm(5)](https://man7.org/linux/man-pages/man5/proc_pid_statm.5.html) 与 [RSS 修正提交](https://github.com/LIghtJUNction/zsys/commit/d4b000ba3852e7c5afae8aac4ad26e3ba321111b)。

网络在受限环境中先使用 `getifaddrs`，遇到指定权限／协议错误再通过只读 ioctl 与 procfs 获取地址，`AddressSource` 暴露来源。独立详情刷新检测 ifindex 变化或接口消失后，会使下一次计数器读取重新建立基线，避免跨接口身份计算流量；重复行或损坏输入不能部分覆盖有效快照。[网络修复提交](https://github.com/LIghtJUNction/zsys/commit/89e6c6c020869f3e6b37ea393bfc1040bd336d71) 与 [collector 测试](../tests/collectors.rs) 提供证据。

硬件没有假设所有设备存在，也没有把缺少驱动属性解释为零占用。thermal zone 可按毫摄氏度换算；hwmon 由于驱动 ABI 存在温度与 ADC 值差异而保留原始单位。GPU 数据只覆盖实际导出的 sysfs 识别和可选遥测，完整 NVML、Intel/AMD/NVIDIA 通用后端尚未实现。[Hardware 测试](../tests/hardware.rs)、[hwmon ABI](https://docs.kernel.org/hwmon/sysfs-interface.html) 与 [thermal API](https://docs.kernel.org/driver-api/thermal/sysfs-api.html) 对应此范围。

### 2.3 平台与依赖

Linux x86_64 glibc、aarch64 glibc、x86_64 musl 已在原生 CI 中运行。手写 FFI 的结构体与常量通过 [C 头文件校验](../scripts/check-abi.c) 和 Rust layout 测试约束；不依赖第三方 `libc` crate，但程序仍使用 Rust `std` 和正常的 OS C ABI。Windows/macOS 的绿色任务只证明核心与条件编译正确，没有交付原生采集后端。[平台文档](platforms.md) 给出完整矩阵。

主 crate 的 `[dependencies]` 为空；独立 [benchmarks/Cargo.toml](../benchmarks/Cargo.toml) 固定 sysinfo `=0.40.3`，不会成为库的运行时依赖。`no-default-features` 保留模型、公式和解析器；`alloc` 允许分配能力但不启用 OS；默认 `std` 仅在 Linux 启用当前后端。包检查与 CI 对上述依赖和 feature 边界执行门禁。

## 3. 测试、CI 与资源验证

### 3.1 指定提交的完整 CI

[分支运行 43198501256](https://github.com/LIghtJUNction/zsys/actions/runs/43198501256) 测试提交 `89e6c6c020869f3e6b37ea393bfc1040bd336d71`；[PR 运行 43198509342](https://github.com/LIghtJUNction/zsys/actions/runs/43198509342) 测试 GitHub 生成的合并提交 `4a012c3132f0b948de901821e4f25b2b986d06fd`。两次均为 **11 个 job 全部成功**。PR 测试合并提交是验证对象，不代表 PR 已经合入 `main`。

| 检查组 | 已执行范围 |
|---|---|
| Ubuntu 22.04／24.04 | 格式、无默认 feature、`alloc`、完整测试、Clippy、rustdoc、ABI、版本脚本、包依赖检查 |
| Windows／macOS | 核心测试、条件编译、格式、Clippy、rustdoc |
| MSRV | Rust 1.95 的核心与完整 Linux 测试 |
| `no_std` | `thumbv7em-none-eabi` 的 core 与 `alloc`，`wasm32-unknown-unknown` core 编译 |
| Linux aarch64 | 原生完整测试与 C ABI 校验 |
| Debian／Alpine | glibc、musl 的原生完整测试、核心编译与 ABI 校验 |
| 资源持续测试 | release 构建、同一个 collector 连续运行 60 秒 |
| Benchmark | 独立包格式、release 构建、Clippy |

Ubuntu 24.04 [完整日志](https://github.com/LIghtJUNction/zsys/actions/runs/43198509342/job/131115536770) 给出 22 个单元测试、33 个集成测试及 2 个文档测试通过；关闭默认 features 时为 15 个核心单元测试及 2 个文档测试。fixture 覆盖解析溢出、缺字段、损坏输入、CPU 热插拔、计数器回退、挂载转义、硬件缺失、PID 复用、权限变化与跨身份详情。真实 Linux 测试补充子进程创建／退出、系统 API 与线程独立使用。[tests](../tests/) 保留所有测试。

### 3.2 连续资源测试的结论

| 执行 | 持续时间 | 同一 collector 刷新次数 | 测试线程热路径分配 | 保留字节增长 | FD 增长 |
|---|---:|---:|---:|---:|---:|
| [分支 job](https://github.com/LIghtJUNction/zsys/actions/runs/43198501256/job/131115508106) | 60.003 s | 3,968,000 | 0 | 0 B | 0 |
| [PR job](https://github.com/LIghtJUNction/zsys/actions/runs/43198509342/job/131115536207) | 60.006 s | 4,514,816 | 0 | 0 B | 0 |

[测试实现](../tests/process_resources.rs) 先检查 100 次创建／刷新／销毁后的资源归还，再对一个稳定 PID 使用同一个 collector 分批刷新。每批校验分配和持有字节，读取 FD 列表的临时分配在计量批次之外；最后丢弃 collector，检查资源返回基线。这能支持“该稳定单 PID 路径在已执行的 60 秒内没有观测到分配或资源增长”，不能扩展成所有模块和任意时长均不存在泄漏。

更长测试由 [soak 工作流](../.github/workflows/soak.yml) 和 `ZSYS_SOAK_SECONDS` 提供。尚需在真实长期工作负载、PID 高频变化、网络热插拔与故障文件系统场景验证小时／天级表现；目前报告不把已配置的调度当作已执行记录。

## 4. 基准环境、方法与公平性

### 4.1 实际环境

| 字段 | 本地最终测量记录 |
|---|---|
| CPU | Intel Xeon Platinum 8573C；当前视图 9 个在线逻辑 CPU |
| RAM | `/proc/meminfo` 总量 10,475,802,624 B，约 9.756 GiB |
| OS／kernel | Debian GNU/Linux 13.6（trixie），Linux 6.18.57，x86_64 |
| Rust／Cargo | rustc 1.99.1、Cargo 1.99.1；编译器记录包含 LLVM 23.1.1 |
| 编译 | release，thin LTO，`codegen-units=1`，无额外 RUSTFLAGS |
| 对照版本 | sysinfo `0.40.3`，固定依赖及 Cargo.lock |
| 测量时间 | 2026-11-12 UTC；各组记录独立时间与二进制 SHA-256 |
| 进程范围 | 普通全表样本 3 个；受控规模每个样本实际为 100／500／1,000／5,000 个 |

来源为 [overview.json](../benchmarks/evidence/overview.json) 及 [原始归档](../benchmarks/evidence/final-raw.tar.gz) 中各组 `environment.json`。这是共享云环境，最终测量期间没有并行执行本地编译或额外主动造负载，但其他租户、调度与硬件状态不受完整控制。结果是该环境的描述性统计，不是生产 P99 保证。

普通热／冷路径每个实现有 3,000 个样本；真实间隔 CPU／basic 和每个受控规模各有 300 个样本。每组包含三轮重复，统计合并全部原始样本，P95／P99 使用 nearest-rank，不删除异常值；吞吐量为平均调用耗时的倒数。CPU 与 basic 在调用前等待 210 ms，等待不计入延迟或倒数吞吐，所以后者不是该采样策略实际每秒刷新次数。[run.py](../benchmarks/run.py) 与 [summarize.py](../benchmarks/summarize.py) 定义此方法。

### 4.2 采集范围验证

进程对比在同一环境检查双方 PID 集合，并对受控空闲子进程验证公共字段。每一条原始测量还记录实际进程数；zsys 的部分失败扫描会拒绝作为可比结果。最终默认 RSS 已统一采用 `statm` 语义，早期 stat-only 的较低成本只保留为开发证据，不再计入公平性能结论。双方不枚举每个线程的独立 task，也不在核心刷新中无条件读取命令行、可执行文件与 UID／GID。[benchmark 源码](../benchmarks/src/main.rs) 保留范围和门禁。

CPU 使用率要求两次真实读取。`cpu_hot` 的密集调用中，sysinfo 可以因最小更新间隔返回缓存，而 zsys 继续读计数器，因此它和 `basic_hot` 被标记 `different_freshness`，不能用该组数字算同范围胜负。报告的 CPU 和 basic 正面对比只使用等待 210 ms 的组。

磁盘容量对比限定为双方一致的 11 个 overlay 挂载点，并仅刷新总量和可用空间等 storage 数据；它不等于块设备 I/O 或所有挂载发现。`process_inventory` 比较双方提供 PID／PPID／名称／启动标识的刷新操作；zsys 当前 `refresh` 还更新核心计数器，因此该用例描述公共返回需求，不能当成严格只读 PID 的成本。只枚举 PID 的 `enumerate` 没有假造 sysinfo 等价 API。

本地 sysinfo 的网络列表为空，而 zsys 的后备路径能读到 loopback。完整网络门禁因此失败，`network` 和包含网络的 `full` 不生成对照倍率。网络修复后的 `network-fixed` 组覆盖旧 `hot` 网络行；只计数器路径和块 I/O 独立路径单列，不能与范围更大的 API 比速度。

## 5. 与 sysinfo 0.40.3 的真实性能结果

### 5.1 共同采集范围

下表延迟单位均为 **μs**；完整精度、每次分配和峰值信息在 [final-table.csv](../benchmarks/evidence/final-table.csv)。全进程相关普通组为 3 个实际进程；`basic` 只组合 CPU、RAM／Swap 与进程核心记录，**不同于** `System::refresh_basic` 还包含网络计数器与块 I/O 的 API 范围。

| 用例 | 实现 | Mean | Median | P95 | P99 | Max | 倒数吞吐 ops/s |
|---|---|---:|---:|---:|---:|---:|---:|
| CPU，真实间隔 | zsys | 60.939 | 42.520 | 137.969 | 514.797 | 581.471 | 16,410 |
| CPU，真实间隔 | sysinfo | 115.524 | 75.117 | 127.236 | 231.166 | 9,445.561 | 8,656 |
| RAM／Swap | zsys | 3.648 | 3.188 | 3.330 | 12.998 | 144.818 | 274,141 |
| RAM／Swap | sysinfo | 6.751 | 5.967 | 6.337 | 20.954 | 309.941 | 148,127 |
| 单进程核心 | zsys | 5.512 | 4.281 | 4.554 | 45.359 | 240.839 | 181,415 |
| 单进程核心 | sysinfo | 16.754 | 12.225 | 18.934 | 99.836 | 1,684.298 | 59,688 |
| 进程 inventory | zsys | 20.097 | 16.820 | 23.020 | 85.232 | 2,042.872 | 49,759 |
| 进程 inventory | sysinfo | 42.916 | 35.287 | 87.557 | 184.405 | 1,091.582 | 23,302 |
| 全进程核心 | zsys | 19.281 | 16.857 | 22.013 | 90.308 | 293.651 | 51,865 |
| 全进程核心 | sysinfo | 51.954 | 40.676 | 101.290 | 211.068 | 1,629.464 | 19,248 |
| 11 个挂载容量 | zsys | 8.462 | 7.001 | 7.237 | 58.890 | 324.783 | 118,173 |
| 11 个挂载容量 | sysinfo | 9.219 | 7.772 | 8.027 | 59.394 | 251.136 | 108,467 |
| CPU＋内存＋进程 | zsys | 213.791 | 158.253 | 278.566 | 1,453.126 | 5,344.132 | 4,677 |
| CPU＋内存＋进程 | sysinfo | 362.349 | 242.611 | 367.832 | 3,695.481 | 6,590.887 | 2,760 |

按中位延迟比 `sysinfo / zsys`，内存为 **1.87×**、单进程为 **2.86×**、3 进程全刷新为 **2.41×**、限定挂载容量为 **1.11×**、CPU／内存／进程组合为 **1.53×**。这些比值只描述上述采集范围与环境，不代表对所有 sysinfo API 的普遍优势。

CPU 中位延迟虽然为对照的约 1/1.77，但 **zsys 的 P95 与 P99 更差**：P99 514.797 μs 对 231.166 μs，约为对照的 **2.23 倍**。挂载容量和进程 inventory 的最大耗时也存在 zsys 更高的情况。未采用“只看平均值或中位数”的方式掩盖尾延迟；300 个 CPU 样本的 P99 仍需更多受控环境测量支持。

### 5.2 实际进程规模

以下全进程刷新包括核心身份、状态、CPU 累计值、RSS 和虚拟内存，单位为 **ms**。每个规模每个实现 300 个样本，原始行的实际记录数均与左栏一致。

| 实际进程数 | zsys Mean | sysinfo Mean | zsys Median | sysinfo Median | zsys P99 | sysinfo P99 | 中位比值 |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 100 | 0.522 | 1.201 | 0.482 | 1.097 | 0.873 | 2.791 | 2.28× |
| 500 | 3.052 | 5.900 | 2.973 | 5.808 | 5.179 | 7.512 | 1.95× |
| 1,000 | 6.469 | 12.649 | 6.116 | 12.158 | 12.711 | 22.666 | 1.99× |
| 5,000 | 46.914 | 86.401 | 43.247 | 81.204 | 93.548 | 165.681 | 1.88× |

100 个进程在此环境的中位数、平均值和 P99 均低于 1 ms；500 个及以上没有达到 1 ms。5,000 进程虽然比 sysinfo 快，单轮中位数仍约 43 ms，不能把这个结果宣传为 1 kHz 全表采集能力。规模中包括 runner、shell 和 benchmark 本身，不能把“创建的 sleep 子进程数量”代替实际采集数。

10,000 进程没有最终同范围结果。此前开发阶段在 6,340 个实际可见进程时触发可用内存保护；当时采用旧 stat-RSS 路径，数据保存在 [开发归档](../benchmarks/evidence/development-raw.tar.gz)，仅作为实验过程和资源上限记录。

### 5.3 构造与首次刷新

“冷”用例表示每次构造 collector、首次刷新并销毁，不进行 collector 预热。OS 缓存保持运行中的状态，不表示重启后的冷存储，也不是每条样本都重新启动可执行程序。`cold_basic` 首次 CPU 刷新只建立基线，没有有效使用率百分比，不能与经过真实间隔的 `basic` 视为同一状态。以下均为 μs，普通全表仍为 3 个进程。

| 用例 | zsys Median | sysinfo Median | zsys P99 | sysinfo P99 |
|---|---:|---:|---:|---:|
| 单 PID 构造＋首次刷新＋销毁 | 5.178 | 88.996 | 25.709 | 332.060 |
| 全进程构造＋首次刷新＋销毁 | 18.238 | 121.492 | 147.169 | 721.417 |
| CPU／内存／进程构造＋首次刷新＋销毁 | 31.132 | 126.521 | 146.057 | 449.892 |

构造路径有明确收益，但不为首次 CPU 读数补造利用率；首次采样仍然建立基线。按需初始化减少了此类短命采集器不需要的状态，具体机制见 [架构](architecture.md)，数字见 [最终表格](../benchmarks/evidence/final-table.csv)。

### 5.4 独立路径与原生枚举的取舍

| zsys 独立用例 | Median | P99 | 说明 |
|---|---:|---:|---|
| 网络计数器，修复后 | 1.706 μs | 2.640 μs | 稳定 loopback，0 次 Rust 分配；未包含地址与链路详情 |
| 完整网络，修复后 | 17.525 μs | 128.021 μs | loopback 计数器＋详情、ioctl 后备；不能生成 sysinfo 倍率 |
| 可见块设备／分区 I/O | 14.159 μs | 77.551 μs | 独立 diskstats 路径，未包含挂载容量 |
| 5,000 PID 纯枚举：Native | 1.681 ms | 3.329 ms | `getdents64`，不读取每个进程记录 |
| 5,000 PID 纯枚举：Std | 2.346 ms | 6.242 ms | 同一 zsys API 的标准库枚举对照 |

原生枚举的收益不能直接推广到完整刷新。5,000 进程全刷新中，Native 平均 46.914 ms、P99 93.548 ms；Std 平均 45.465 ms、P99 67.691 ms。Native 中位数略低，但平均值和 P99 未胜出。逐 PID 文件工作和运行环境对完整路径的贡献仍然很大，应保留两个可测实现，继续以数据选择，而不把“更底层”当成性能证明。

## 6. 分配与峰值内存

分配数据来自 Rust 全局分配器：累计成功的 `alloc`、`alloc_zeroed`、`realloc` 请求及请求字节。它不包含 C 库分配、分配器元数据、线程栈或映射；计数器本身的开销在计时中。保留堆扣除了保存测量行的 vector，进程 `VmHWM` 则包含可执行程序、加载器、分配器和 harness，不能解释成某个 collector 的独占峰值。[方法元数据](../benchmarks/evidence/overview.json) 和 [计量实现](../benchmarks/src/main.rs) 定义这些口径。

下表 `分配次数 / 请求字节` 为每次刷新均值，`保留堆` 为预热后仍存活的 Rust 请求字节，`VmHWM` 列给出三轮测量的范围，单位 KiB。

| 路径 | zsys 分配次数／B | sysinfo 分配次数／B | zsys 保留堆 B | sysinfo 保留堆 B | zsys VmHWM KiB | sysinfo VmHWM KiB |
|---|---:|---:|---:|---:|---:|---:|
| CPU，真实间隔 | 0 / 0 | 12 / 9,671 | 11,278 | 4,647 | 1,476–1,480 | 1,492–1,496 |
| RAM／Swap | 0 / 0 | 1 / 16,635 | 8,212 | 1,056 | 1,464–1,528 | 1,476–1,544 |
| 单进程 | 0 / 0 | 12 / 33,326 | 4,648 | 6,322 | 1,480–1,544 | 1,588–1,652 |
| 全进程，3 个 | 0 / 0 | 85 / 34,671 | 8,771 | 6,347 | 1,488–1,552 | 1,584–1,652 |
| 全进程，100 个 | 0 / 0 | 664 / 77,800 | 29,845 | 58,323 | 1,396–1,528 | 1,652–1,720 |
| 全进程，1,000 个 | 0 / 0 | 6,420.13 / 494,179.12 | 310,185 | 856,978 | 1,824–1,952 | 3,312–3,384 |
| 全进程，5,000 个 | 0 / 0 | 35,066.69 / 2,391,146.44 | 1,151,433 | 3,430,197 | 3,068–3,200 | 8,732–8,744 |
| 11 个挂载容量 | 0 / 0 | 22 / 723 | 1,572 | 3,339 | 1,612–1,616 | 1,552–1,556 |
| CPU／内存／进程 | 0 / 0 | 98 / 60,977 | 28,261 | 6,347 | 1,512–1,516 | 1,508–1,512 |

这些稳定热路径在当前数据中没有 Rust 分配请求，但“零请求”不等于零占用。CPU、Memory 和小表组合路径保留了比 sysinfo 更大的缓冲；磁盘和 basic 的进程 RSS 范围也并非都更小。5,000 进程下，zsys 的保留堆与进程峰值明显较低，这才是本次数据能够支持的大规模内存优势。

完整网络刷新仍平均分配 20 次／3,441 B，修复后的保留堆为 10,750 B；进程详情、发现新设备、硬件拥有型结果和扩容也可以分配。冷单 PID 路径 zsys 为 4 次／4,648 B，sysinfo 为 78 次／92,531 B，这里的请求总字节还可能超过任意时刻同时存活的字节数。完整字段可在 CSV 的 `peak_extra_live_bytes` 与保留字节列分别检查。

当前策略在规模下降后通常保留 Vec／HashMap 容量，以便下次复用。API 没有通用自动缩容或公开 reserve/shrink 策略，调用者可丢弃重建并接受差分基线重置。现有 soak 证明特定稳定路径没有增长，不替代长期变化负载下对历史峰值和资源生命周期的测量。

## 7. 对原研发目标的逐项检查

原目标没有指定只用中位数还是 P99 判定，本报告同时列出 Mean／Median／P99，避免选择最有利统计量。所有目标都属于指定内容与硬件下的研发目标，不是 API 保证。以下单位为 μs，进程规模另列。

| 目标与测量范围 | 目标 | zsys Mean | Median | P99 | 判定 |
|---|---:|---:|---:|---:|---|
| CPU：9 个在线逻辑 CPU＋聚合、真实 210 ms 间隔 | ≤5 | 60.939 | 42.520 | 514.797 | 均未达到 |
| Memory：RAM／Swap | ≤3 | 3.648 | 3.188 | 12.998 | 均未达到 |
| Network：本地 loopback 完整计数器＋详情 | ≤30 | 27.970 | 17.525 | 128.021 | Mean／Median 达到，P99 未达到；无对照倍率 |
| Disk：11 个固定 overlay 挂载容量 | ≤10 | 8.462 | 7.001 | 58.890 | Mean／Median 达到，P99 未达到 |
| Disk：全部可见块设备／分区 I/O | ≤10 | 16.808 | 14.159 | 77.551 | 均未达到 |
| 单进程核心 | ≤20 | 5.512 | 4.281 | 45.359 | Mean／Median 达到，P99 未达到 |
| 全进程核心：3 个 | ≤1,000 | 19.281 | 16.857 | 90.308 | 本规模三项达到 |
| 全进程核心：100 个 | ≤1,000 | 522.269 | 481.722 | 873.056 | 本规模三项达到 |
| 全进程核心：500 个及以上 | ≤1,000 | 3,051.531 起 | 2,972.964 起 | 5,179.038 起 | 已测规模均未达到 |
| CPU／内存／3 进程核心组合 | ≤1,000 | 213.791 | 158.253 | 1,453.126 | Mean／Median 达到，P99 未达到；不含网络与磁盘 |
| 所有模块完整组合 | ≤1,000 | — | — | — | 本地网络公平性门禁失败，未形成可比结果 |

目标检查的机器可读数据保存在 [overview.json 的 targets/unavailable](../benchmarks/evidence/overview.json)。网络只计数器达到更低延迟，并不能替代完整网络目标；少进程场景满足 1 ms，也不能代替 5,000 或 10,000 进程场景。

## 8. 仍有的瓶颈与下一阶段计划

**首先解决尾延迟的可解释性。** CPU 在实际采样间隔下的 P99 比 sysinfo 高，而连续密集读取的 zsys 延迟明显不同；共享调度、CPU 状态或内核生成统计的成本可能参与其中，但现有数据不能确定归因。下一步应在固定硬件和亲和性、记录负载的条件下增加重复和采样数量，结合系统调用／CPU profile 分解等待、读取和解析成本。验证标准需要同时看 Median、P95、P99 和最大值。

**其次减少全表读取成本，同时保留 RSS 与身份正确性。** 默认进程路径要读取 `statm` 和 `stat`，这是公平 RSS 语义带来的真实工作，不能为达标退回字段较少的旧路径。应比较目录句柄策略、读取顺序、进程表布局和字段选择对 100–10,000 规模的影响；若提供更轻的可选范围，应在 API 与基准中明确标记。1 ms 全表目标在 500 个及以上尚有数量级差距。

**为缓冲复用补充占用预算。** 当前策略压低了重复分配，但 CPU／Memory 的固定保留字节比对照大。后续可评估按需初始容量、显式预留、受控缩容或 collector 重置策略，以实际规模的分配次数、保留堆和 RSS 共同选择；不能只为分配计数为零而默认保留过多空间。

**继续优化按需详情并扩大故障覆盖。** 完整网络当前仍有每次分配，地址列表和 sysfs 元数据可以在不隐藏刷新语义的前提下研究存储复用。网络 namespace／接口快速替换、长时间 PID churn、权限变化、远程文件系统阻塞和设备热插拔需要更长压力测试。硬件则需按驱动单位与能力扩展，不能把有限 sysfs 支持称为全厂商 GPU 实现。

**完成独立环境复测与发布闭环。** GitHub benchmark [43198501217](https://github.com/LIghtJUNction/zsys/actions/runs/43198501217) 在本报告记录时仍执行受控进程规模；其 CPU／内存／进程以及网络／full 的公平性与原始产物将单独审查。10,000 规模需要更充足资源，小时／天级 soak 需要保留实际执行日志。随后确认发布候选的完整 CI、crate 索引校验和、Tag 和 GitHub Release，才可关闭最终验收。

Windows 与 macOS 应在 Linux 结果稳定之后分别使用原生接口实现，再为 FreeBSD、Android、iOS 制定可验证范围。它们的权限、计数单位和生命周期语义需要独立定义，不能用 Linux 的 `/proc` 数据结构冒充其他系统的实现。

## 9. 版本、发布与交付位置

版本管理接受 `none`、`patch`、`minor`、`major`。两个 lockfile 只同步本 crate 版本，不通过 bump 顺带升级 benchmark 依赖；工作流对精确候选提交运行完整 CI，然后打包、发布并核对 registry 中的 checksum，最后创建一致的 `vVERSION` Tag 与 GitHub Release。重试准备阶段仅复用父提交和文件树都匹配的已推送 bump；已发布候选必须与包 checksum 一致。[release.yml](../.github/workflows/release.yml)、[version.py](../scripts/version.py)、[publish.py](../scripts/publish.py) 是执行来源。

发布首选 secret **`CARGO_REGISTRY_TOKEN`**，兼容用户指定的 **`CARGO_REGISTY_TOKEN`**，后者作为 fallback；最终传给 Cargo 的环境变量使用 canonical 名称。Token 不进入仓库。配置和校验工作流已经存在，但是否有有效凭据、registry 是否接受本次上传以及 Release 是否生成，都必须由实际运行结果确认。

| 位置／版本证据 | 链接与状态 |
|---|---|
| 仓库 | [LIghtJUNction/zsys](https://github.com/LIghtJUNction/zsys) |
| 实施 PR | [PR #1](https://github.com/LIghtJUNction/zsys/pull/1)，本报告记录时为 open draft |
| RSS 语义与身份修正 | [d4b000ba3852e7c5afae8aac4ad26e3ba321111b](https://github.com/LIghtJUNction/zsys/commit/d4b000ba3852e7c5afae8aac4ad26e3ba321111b) |
| 本地普通／规模基准来源 | [b469697a422d84b5e75cab2f2316f7f21d6fe036](https://github.com/LIghtJUNction/zsys/commit/b469697a422d84b5e75cab2f2316f7f21d6fe036) |
| 本地真实间隔基准来源 | [e59722344c97e4365226837506a18f9817511795](https://github.com/LIghtJUNction/zsys/commit/e59722344c97e4365226837506a18f9817511795)，文档阶段提交，非网络原生代码与前述来源一致 |
| 当前原生代码／网络复测来源 | [89e6c6c020869f3e6b37ea393bfc1040bd336d71](https://github.com/LIghtJUNction/zsys/commit/89e6c6c020869f3e6b37ea393bfc1040bd336d71) |
| 开发原始证据保留提交 | [4dd4b537da3920a422b16c994030026b8f06dcc7](https://github.com/LIghtJUNction/zsys/commit/4dd4b537da3920a422b16c994030026b8f06dcc7) |
| GitHub Release | [Releases](https://github.com/LIghtJUNction/zsys/releases)，**尚待实际发布确认** |
| crates.io 候选目标 | [zsys 0.1.0](https://crates.io/crates/zsys/0.1.0)，**尚待 registry 校验确认** |

后续提交或合并会改变仓库 HEAD，因此报告以各组记录的精确 SHA 和二进制哈希追溯测量，不把所有数据笼统归到一个最新版本名。原生代码修改后须判断哪些数据失效；本次网络身份修复已单独重跑网络组。

## 10. 原始证据与复现入口

- [README](../README.md)：运行示例、feature、使用与验证命令。
- [BENCHMARKS.md](../BENCHMARKS.md)：基准协议、正式分析与运行说明。
- [benchmarks/run.py](../benchmarks/run.py)、[summarize.py](../benchmarks/summarize.py)：独立子进程、统计与范围门禁。
- [overview.json](../benchmarks/evidence/overview.json)：环境、各组统计、来源 SHA、目标与不可用项。
- [final-table.csv](../benchmarks/evidence/final-table.csv)：完整 Mean／Median／P95／P99／Max／吞吐／分配／保留堆／VmHWM。
- [final-raw.tar.gz](../benchmarks/evidence/final-raw.tar.gz)、[final-checksums.json](../benchmarks/evidence/final-checksums.json)：原始样本及产物校验和。
- [development-raw.tar.gz](../benchmarks/evidence/development-raw.tar.gz)：早期基线与过程记录，旧 RSS 范围不用于最终公平结论。
- [CI](https://github.com/LIghtJUNction/zsys/actions/workflows/ci.yml)、[Soak](https://github.com/LIghtJUNction/zsys/actions/workflows/soak.yml)、[Benchmark](https://github.com/LIghtJUNction/zsys/actions/workflows/benchmark.yml)：外部执行记录。

后续验收应补全远端 benchmark、实际发布和更长 soak 的独立记录，保留已经暴露的失败与劣势。已测路径能提供技术收益，与所有目标都已达到是两个不同层次的结论；本版本的发布状态和能力边界按上述证据分别判断。
