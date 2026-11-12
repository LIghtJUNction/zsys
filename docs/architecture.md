# 架构与可验证的设计

zsys 把采集分成三个责任：解释字节、读取操作系统、维护采样状态。核心层独立于 OS 和分配器，Linux 采集器拥有缓冲区与历史值，`System` 仅组合调用。设计的性能收益以 [BENCHMARKS.md](../BENCHMARKS.md) 为证据入口，本文不赋予未经测量的倍数优势。

## 模块边界

| 层次 | 输入与输出 | 设计目的 | 代码与验证 |
|---|---|---|---|
| Core | 原始计数器、时间间隔 → 有单位的数据与差分 | 公式可单独测试，解析器可用于 fixture 或其他数据来源 | `src/cpu.rs`、`src/process.rs`、`src/disk.rs`、`src/network.rs` 的单元测试 |
| Parsers | 借用的 byte slice → 计数器或借用字段 | 解析时不创建临时字符串，拒绝缺失字段、溢出和损坏记录 | [src/linux/parse.rs](../src/linux/parse.rs) 与各 core parser 测试 |
| Linux I/O | procfs/sysfs/原生 API → 可复用读取存储 | 降低固定文件重复打开、路径构建和缓冲扩容成本 | [src/linux/io.rs](../src/linux/io.rs)、[C ABI 检查](../scripts/check-abi.c) |
| Stateful collectors | 本轮记录＋上次状态 → 新快照、差分、生命周期与失败状态 | 稳定对象复用存储，识别身份变化，区分新数据和旧数据 | [tests/processes.rs](../tests/processes.rs)、[tests/collectors.rs](../tests/collectors.rs)、[tests/disks.rs](../tests/disks.rs) |
| Composition | 显式刷新请求 → 相应模块的借用视图 | 按需初始化，保持独立刷新，无后台调度成本 | [src/linux/system.rs](../src/linux/system.rs)、[tests/system.rs](../tests/system.rs) |

## 一次读取如何形成快照

公共计数器文件通过缓存的打开句柄读取，数据进入采集器自己的缓冲区。解析器在这些已读取字节上工作，名字或记录只有在需要跨次刷新保留时才复制到拥有型字段。这里仍然存在内核到用户缓冲区的读取复制；“借用解析”描述的是解析阶段的内存行为。

CPU、网络与块 I/O 先验证输入，再更新可见结果。网络与块 I/O 使用复用的暂存记录，重复键或损坏输入会返回错误并保留旧成功快照。CPU 热插拔改变聚合基线，网络 ifindex 变化、设备名称变化或累计值回退会影响相应差分。错误路径的实际行为见 [采样语义](semantics.md)。

进程读取采用可见 PID 表与代次标记。完整枚举成功后，采集器更新各 PID 的当前记录，记录失败项，最后移除本轮不再可见的项。相同 PID 的启动 tick 改变时，使用新的身份与 CPU 基线。默认 RSS 来自同一 PID 身份下的 `statm`，`stat` 的原始 resident 字段仍供检查；详情进一步通过固定 PID 目录和前后身份检查避免跨生命周期拼接。[tests/processes.rs](../tests/processes.rs) 覆盖生命周期、权限、异常输入和身份一致性。

## 为什么拆开高频计数器与详情

CPU 时间、内存统计、网络流量与进程核心记录适合频繁读取，命令行、可执行文件、用户 ID、网络地址及硬件 inventory 的读取范围和变化频率不同。独立 API 使调用者决定何时付出这些成本。例如 `Networks::refresh_counters` 不发现 IP 地址，`Processes::details` 不隐式刷新整个进程表，`CpuCollector::refresh_frequencies` 不混入每次 CPU 使用率读取。

磁盘容量也单独暴露，因为挂载点可能属于远程或故障文件系统。`Mounts` 提供发现结果，调用者选择 `Filesystem` 执行容量查询；`System::refresh_basic` 保持为明确的计数器组合，不包括所有挂载的容量与硬件详情。这个边界既影响延迟，也影响失败处理方式，参见 [statvfs(3)](https://man7.org/linux/man-pages/man3/statvfs.3.html)。

## 内存与原生调用的选择

采集器保留 Vec、HashMap 和读取缓冲容量，避免稳定规模下重复创建整个数据结构。新增对象、名字增长或更大的文件仍然可能分配。保留容量使驻留内存受历史峰值影响；丢弃采集器会关闭其拥有的文件描述符并释放拥有型存储。[进程资源测试](../tests/process_resources.rs) 检查热路径分配、持续运行的保留字节与描述符，以及反复创建销毁后的资源归还。

PID 枚举有 `ScanMode::Native` 和 `ScanMode::Std` 两个实现。原生路径在 x86_64/aarch64 使用 `getdents64`，目录缓冲从需要枚举时才分配，并有增长上限；标准库路径保留为可测量的对照和其他架构的默认选择。单 PID 刷新无需分配整个目录枚举缓冲。两种路径的输出一致性由 [tests/processes.rs](../tests/processes.rs) 检查，性能差异由独立 benchmark 测量。

原生 FFI 限制在 Linux 后端，结构体布局和常量通过 C 头文件校验与 Rust layout 单元测试约束。文件描述符交给 `File` 或 `OwnedFd`，`getifaddrs` 分配的链表由专用 RAII 对象释放。零第三方运行时 crate 表示依赖关系为空，并不消除 OS 接口、Rust 标准库或平台 C ABI 的职责。依赖门禁见 [scripts/check-package.py](../scripts/check-package.py)。

## 测量与验收依据

[benchmark package](../benchmarks/) 固定 sysinfo `=0.40.3`，只调用双方公开 API。每个实现运行在独立子进程中，记录分位延迟、吞吐、Rust 分配请求、保留堆字节及整个测试进程的峰值 RSS。报告需要保留采样范围与 freshness：同名函数的两次调用可能执行不同工作，系统接口受到权限限制时也可能得到不同集合。

真实 CPU 刷新与可能命中 sysinfo 最小间隔缓存的高频调用分开记录；网络对比先检查双方实际集合和详情；进程比较以实际采集身份、RSS 来源和字段范围为准；磁盘容量使用对应的挂载集合。仅 zsys 支持的原子操作单独报告。该方法将架构选择与可复查的观测关联，而不把单个机器上的结果扩大为所有平台的承诺。

## 文档主张与证据核对

| 主张 | 证据 | 边界 |
|---|---|---|
| 核心不要求 `std` 或分配器 | no-default-features 测试、bare-metal/WASM 编译门禁 | 这些目标只验证 core，未实现原生采集 |
| 运行时第三方依赖为空 | Cargo metadata 与 `scripts/check-package.py` | benchmark 独立使用 sysinfo 开发工具依赖 |
| 刷新与详情可独立组合 | `tests/system.rs` 和公开 collector API | `refresh_basic` 是顺序组合，失败可留下跨模块不同 freshness |
| PID 复用与失败状态有明确行为 | 生命周期 fixture、权限、快速退出测试 | 可见范围仍由内核权限和 namespace 决定 |
| 复用采集器有可检查的资源行为 | `tests/process_resources.rs` | 持续运行时长和实际结果以执行日志为准 |
| 相对性能可以复现 | benchmark 源码、原始数据与 `BENCHMARKS.md` | 依赖硬件、进程规模、采样内容与环境权限 |

## 自审

- **贡献范围：** 提供原子采集 API、保留状态和明确差分语义；尚未实现其他 OS 的原生后端。
- **表达清晰：** API、单位、时间原点、错误状态和拥有关系分别说明，术语与公开类型保持一致。
- **实验强度：** README 不引用待验收数字；所有性能判断由 benchmark 数据和采样条件支撑。
- **评估完整性：** fixture、并发、资源、发行版、ABI 与编译矩阵分别覆盖明确的失败模式；环境允许的进程规模由报告记录。
- **设计边界：** 保留容量、近似 RSS、硬件驱动差异、namespace 和可能阻塞的容量调用均有明确说明。
