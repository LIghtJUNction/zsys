# 采样语义与 API

zsys 的一次刷新表示一次明确的读取操作。调用者选择模块、调用时间和需要的详情，采集器保存本模块上次成功观测及可复用存储。所有单位、差分和失败状态都应与对应快照一起使用。

## 时间、快照与刷新边界

`Sample` 包含 `timestamp_ns` 和 `sequence`。Linux 采集器的时间戳来自该实例创建时建立的 `Instant` 原点，因此可用于同一实例前后采样的差分。不同采集器的数值没有共同原点；它们也不是 Unix 时间或系统启动时间。若业务需要跨模块时间轴，应在调用刷新前后额外记录应用自己的共享时钟。

CPU、内存、网络计数器、块 I/O、挂载列表和文件系统容量在首次成功采样前没有有效观测。相应计数器快照的 `sequence == 0` 表示尚未成功采样，读取接口上的 `Option` 用于避免把初始化零值当作真实数据。硬件字段另用 `HardwareValue` 区分尚未采样和读取失败，硬件数值的样本序号表示读取尝试，字段可能为 `Unavailable`。

`System::new()` 不执行文件系统访问，模块在首次显式刷新时初始化。`refresh_memory` 不会刷新 CPU；`refresh_process` 不会扫描其他 PID。`refresh_basic` 依次刷新 CPU、内存、进程、网络计数器和块 I/O，较早步骤成功后即保留新值；后续步骤失败时，剩余模块仍保留自己的旧值。这种组合保留了每个模块的真实时序。相关断言见 [tests/system.rs](../tests/system.rs)。

`SystemSnapshot` 是可由调用者组合的 CPU／内存数据类型，本身不会执行采集，也不会使不同时间的观测变成原子快照。`CpuTimes`、`Memory` 等 `Copy` 数据可以直接复制；进程等含拥有型字段的记录可以按需 `clone`。借用视图避免在每次读取结果时重新构造整个集合。

## CPU 使用率

系统与单核 CPU 百分比来自两次累计 tick 的差。`CpuTimes` 累计 user、nice、system、idle、iowait、irq、softirq 和 steal；guest 时间已经包含在 user/nice 中，不重复累加。实现将 idle 和 iowait 视为空闲，计算：

```text
busy_percent = 100 × (Δtotal − Δidle − Δiowait) / Δtotal
```

首次观测、累计值回退或 `Δtotal == 0` 时，使用率为 `None`。Linux 文档说明 iowait 也可能下降，因此该情况会使当前差分无效。`CpuCollector` 每次发现当前在线 CPU；在线集合变化时重建聚合基线，仍存在且身份一致的单核可以继续差分。实现和回归测试见 [src/cpu.rs](../src/cpu.rs)、[tests/collectors.rs](../tests/collectors.rs)，字段来源见 [proc_stat(5)](https://man7.org/linux/man-pages/man5/proc_stat.5.html)。

进程 CPU 百分比以一颗逻辑 CPU 为 100%，使用成功观测之间的单调时间间隔：

```text
cpu_percent = 100 × Δ(user_ticks + system_ticks)
              / (ticks_per_second × elapsed_seconds)
```

多线程进程同时占用两颗逻辑 CPU 时可以接近 200%。代码不按机器核数、CPU affinity 或 cgroup 配额再归一化。`Processes` 为各 PID 保存各自的采样时间；函数执行了多少微秒，不会被用作两次采样之间的时间。第一次读取、PID 复用、前次不可访问、计数器回退或无效时间间隔会得到 `None`。该公式在 [src/process.rs](../src/process.rs) 有确定性测试。

高频读取不会增加内核 CPU tick 的分辨率。短间隔内可能得到零增量、`None`，或量化明显的瞬时进程百分比；应用可按业务需要延长间隔或对多次结果平滑。`CpuCollector::refresh_frequencies` 单独读取 sysfs `scaling_cur_freq` 并转为 Hz；它不修改使用率采样时间，频率也不保证等于芯片此刻的实际运行频率。频率字段没有单独的内置时间戳，调用者如需精确 freshness 应记录调用时间。参见 [CPUFreq 文档](https://docs.kernel.org/admin-guide/pm/cpufreq.html)。

## 进程身份、可见性与详情

`ProcessId` 由 PID 和 `start_time_ticks` 组成，区分同一次系统启动中的 PID 复用。`start_time_since_boot(ticks_per_second)` 把启动 tick 转为启动后的 `Duration`，保留可获得的亚秒精度。该值不是墙上时钟日期；跨重启持久化身份时，应用还需要自行关联启动会话。时钟 tick 和页大小由原生 `sysconf` 读取；fixture 可以通过 `with_parameters` 显式给定。字段定义见 [proc_pid_stat(5)](https://man7.org/linux/man-pages/man5/proc_pid_stat.5.html)。

`Processes::refresh` 每次枚举当前可见 PID 并读取选中的记录，复用表项、名称缓冲区和基线。增量体现在状态维护与生命周期变化；它没有订阅内核事件，也没有跳过全部已有 PID 的实时计数器读取。`ProcessChanges` 的 `added`、`removed`、`reused`、`failed` 描述生命周期与采样结果，`updated` 统计成功读取的数量，包含新建和复用 PID，并不表示每个字段发生变化。

| 情况 | 全表 `refresh` 的处理 |
|---|---|
| 新 PID 可读取 | 插入记录，CPU 基线为 `None` |
| PID 相同、启动 tick 改变 | 标记 `reused`，替换身份，重置 CPU 基线 |
| 原记录本轮无法访问 | 保留历史数据和时间戳，标记 `Inaccessible`、`Vanished` 或 `Invalid`，清除 CPU 使用率 |
| 原 PID 未出现在成功枚举结果 | 从可见进程表移除 |
| 整个目录枚举失败 | 返回错误，保留已有表 |

`refresh_pid(pid)` 只更新该 PID，发现其已经消失时移除该项，不会顺带清理其他 PID。`enumerate_pids()` 返回复用缓冲区中的排序去重列表，不读取进程内容、不更新快照。枚举后进程仍可立刻退出，后续读取必须处理错误。`failures()` 对应最近一次全表刷新；单 PID 调用的错误通过返回值和该记录的 `availability` 表达。生命周期、权限、快速创建退出与枚举测试见 [tests/processes.rs](../tests/processes.rs)。

权限与 namespace 决定采集范围。`hidepid`、容器 PID namespace 或挂载视图可以让进程完全不出现在 `/proc`；从表中移除表示不再可见，业务不能仅据此推断其已在宿主机终止。`name` 保存内核 `comm` 的原始字节，它可能被内核截短，也可能不是 UTF-8。线程数量是记录字段；当前 API 不展开每个线程为独立进程对象。

默认进程 `memory_bytes` 读取 `/proc/PID/statm` 的 resident 页数并转换为字节。`counters.resident_pages` 则保留 `/proc/PID/stat` 的原始近似字段，两者可能不同。Linux 对这类 RSS 统计仍有近似说明；它们没有提供 PSS，也不等同于逐映射计算的精确内存。共享页会出现在多个进程的 RSS 中，不宜直接求和当作系统已用内存。参见 [proc_pid_statm(5)](https://man7.org/linux/man-pages/man5/proc_pid_statm.5.html)。

`details(pid, options)` 按需读取 executable、NUL 分隔的命令行、四组 UID 和四组 GID，返回拥有型 `ProcessDetails`。用户／组 ID 顺序为 real、effective、saved-set、filesystem；当前 API 不执行用户名数据库查询。命令行保留空参数和非 UTF-8 字节。每个详情字段为 `Detail::NotRequested`、`Detail::Unavailable(ErrorKind)` 或 `Detail::Value`，空值与读失败各有语义。

详情操作通过同一个打开的 PID 目录读取，并在前后校验身份。它返回读取时的 `ProcessDetails.id`，不会自动替换进程表中较早的记录。把详情附加到已有快照前，应比较两个 `id`，如 README 的示例所示。详情读取会分配拥有型结果，适合按需请求而非无条件放入每个高频循环。

## 内存

`Memory` 的容量单位均为字节，procfs 的 kB 数值按 1024 转换。`used_bytes()` 是 total 减 available；`swap_used_bytes()` 是 swap total 减 swap free。`free_bytes` 与 `available_bytes` 分别表示空闲页和内核估计的可用容量，含义不同。

当内核未提供 `MemAvailable` 时，解析器使用 free、buffers、cached、reclaimable、shared 的已记录近似算法，`MemorySample::available_is_estimate` 明确标记此路径。已有 `MemAvailable` 时使用内核值。解析失败保留旧成功快照，测试见 [tests/collectors.rs](../tests/collectors.rs)；来源见 [proc_meminfo(5)](https://man7.org/linux/man-pages/man5/proc_meminfo.5.html)。默认读取所在挂载视图的 `/proc/meminfo`，不自动替换为容器 cgroup 内存配额。

## 磁盘、挂载与容量

`DiskIoCollector` 从 `/proc/diskstats` 获取块设备和分区计数器。sector 固定按 512 字节计算，与设备物理扇区大小分开；in-flight 是当前进行中的 I/O 数量，属于 gauge。`DiskDelta` 对支持的累计计数器求差，遇到回退、设备名称变化或溢出时不生成有效差分。物理设备和它的分区可能表示同一批 I/O，混合求和会重复统计。参见 [Linux I/O statistics](https://docs.kernel.org/admin-guide/iostats.html) 和 [tests/disks.rs](../tests/disks.rs)。

`Mounts::refresh` 只发现挂载及其路径、类型、源设备和选项。它解码内核的路径转义，按挂载身份保留仍匹配的容量样本。`Filesystem::refresh` 才会调用原生 `statvfs` 获取容量；`free_bytes` 包含所有空闲块，`available_bytes` 表示普通用户可用部分。失败会保留上次成功容量与时间戳，并更新 `last_error`。当前容量 FFI 支持 Linux 64 位 ABI，其他位宽返回 `Unsupported`。参见 [statvfs(3)](https://man7.org/linux/man-pages/man3/statvfs.3.html)。

容量查询可能因远程或故障文件系统阻塞。调用者可以使用 `iter_mut()` 挑选本地挂载点；`refresh_capacities()` 会遍历所有已发现挂载，并返回成功／失败数量。库不设置内核文件系统访问超时，也不在后台执行这些调用。

## 网络

`Networks::refresh_counters` 读取 `/proc/net/dev` 的全部十六个累计计数器，包含字节、包、错误、丢弃等字段。完整解析和重复接口校验成功后才提交快照，旧接口移除，新接口建立基线。`Network.delta` 与 `interval_ns` 可用于应用自己的速率计算；计数器回退时差分为 `None`。字段来源见 [Linux interface statistics](https://docs.kernel.org/networking/statistics.html)。

`refresh_details` 独立获取地址、掩码、ifindex、flags、MAC、MTU 和可用的驱动链路速率，详情带有自己的 `sampled_at_ns`。`AddressSource::Getifaddrs` 表示原生地址列表；明确的 netlink 权限或协议限制会启用 `IoctlProcfs` 后备：IPv4 通过只读 ioctl 枚举，IPv6 来自 `/proc/net/if_inet6`。后备 flags 受 `SIOCGIFFLAGS` 返回范围限制。IPv6 scope ID 保留，六字节 MAC 不适用的硬件类型返回 `None`。参见 [getifaddrs(3)](https://man7.org/linux/man-pages/man3/getifaddrs.3.html)、[netdevice(7)](https://man7.org/linux/man-pages/man7/netdevice.7.html)。

详情发现 ifindex 变化时清除相关差分，避免跨接口复用计算速率。若同名接口的移除和重建都发生在两次详情刷新之间，纯计数器路径可能无法识别该生命周期；需要更强身份检查的应用应同时安排详情刷新。`refresh()` 依次调用 counters 和 details，因此详情失败时，计数器可能已是新样本，调用者应分别检查它们的时间。

网络详情始终读取当前进程所在的网络 namespace；`with_proc_root` 提供的 fixture 路径只替换计数器文件。详情刷新和新接口发现可以分配内存，稳定接口的计数器路径复用存储。

## 硬件与 GPU

`Hardware::refresh_inventory` 发现 hwmon、thermal zone 和 DRM primary card，connector 与 render 节点不作为重复 GPU。缺失 class 目录表示没有暴露此类设备，其他目录错误保留旧 inventory。`refresh_info` 读取 DMI 描述，`refresh_temperatures`、`refresh_gpus` 分别读取已知设备的可用数值。硬件字段通过 `HardwareValue::{NotSampled, Unavailable, Value}` 表达结果。

thermal zone 的 `raw_input` 单位是毫摄氏度，`celsius()` 提供换算。hwmon 保留驱动原始固定点值：多数驱动使用毫摄氏度，部分 ADC 驱动输出毫伏，因此库不对所有 hwmon 输入统一作温度换算。`fault == Value(true)` 表示该通道的测量存在硬件故障。hwmon 与 thermal 两个视图可能重复描述同一个物理传感器，inventory 保留来源供应用选择。依据见 [hwmon ABI](https://docs.kernel.org/hwmon/sysfs-interface.html)、[thermal sysfs API](https://docs.kernel.org/driver-api/thermal/sysfs-api.html)。

GPU 识别包含 DRM card、PCI vendor/device ID、驱动及可用产品名称。VRAM 总量／已用字节和 busy 百分比来自驱动公开的可选 sysfs 文件，当前覆盖 AMDGPU 提供这些属性的情形。缺少属性返回 `Unavailable`，不会生成零占用数据；NVML、完整 Intel/AMD/NVIDIA 遥测和 GPU 控制不在当前实现内。依据见 [PCI sysfs](https://docs.kernel.org/PCI/sysfs-pci.html)、[AMDGPU memory](https://docs.kernel.org/gpu/amdgpu/driver-misc.html)、[AMDGPU activity](https://docs.kernel.org/gpu/amdgpu/thermal.html)，fixture 见 [tests/hardware.rs](../tests/hardware.rs)。

## 内存复用、并发与调度

采集器拥有自己的缓冲区和状态，通过 `&mut self` 刷新，通过借用读取快照。新 PID、较长名称、新接口、CPU 热插拔或更大的文件都可能扩容。Vec 和 HashMap 在规模回落后通常保留容量，目的是复用；这部分驻留容量与未释放对象泄漏应分开测量。当前没有自动缩容策略或公共 reserve/shrink 接口，调用者可以丢弃并重建采集器以释放保留容量，同时建立新的时间与差分基线。

公共 procfs 文件复用打开的文件与读取缓冲；进程记录按本次身份读取，文件所有权由 Rust RAII 管理。读取缓冲存在记录大小上限，过大的输入返回错误，避免无限增长；PID 表的内存仍随可见进程规模增长。`LinuxCollector::processes()` 是早期兼容接口，每次构造 `Vec<ProcessRecord>`；高频调用应使用保留表的 `Processes`。

独立实例可以放在不同线程中使用。库没有全局采样锁；同一实例的并发刷新仍需要调用者提供同步。`Send + Sync` 编译检查和并行独立采集见 [tests/system.rs](../tests/system.rs)。[tests/process_resources.rs](../tests/process_resources.rs) 对持续存活的进程采集器检查 Rust 分配、保留字节和文件描述符，并支持 `ZSYS_SOAK_SECONDS` 延长运行时间。

`Cadence` 接收调用者提供的单调纳秒时间。先检查 `is_due(now)`，刷新成功后调用 `mark_success(now)`；失败不推进成功时间。`set_interval_ns` 修改采样策略，不改写先前观测；`reset` 清除调度基线。它不会 sleep、spawn 线程或隐式刷新。示例见 [examples/cadence.rs](../examples/cadence.rs)。

## 公共 API 索引

| 模块／类型 | 入口与数据 |
|---|---|
| `cpu` | `CpuTimes::total`、`busy_percent_since`；八类累计 tick |
| `memory` | `Memory`；`used_bytes`、`swap_used_bytes` |
| `process` | `ProcessId::start_time_since_boot`、`ProcessCounters::cpu_percent_since`、`ProcessState` |
| `parse` | `parse_cpu_stat`、`parse_meminfo`、`process_stat`／`RawProcess`、兼容 `parse_process_stat`、`ParseError` |
| `disk` | `DiskCounters`、`DiskDelta`、`DiscardCounters`、`FlushCounters`、`Capacity`、`RawDisk`、`RawMount`、`DISKSTATS_SECTOR_BYTES`、diskstats/mountinfo 解析与转义解码 |
| `network` | `NetworkCounters::delta_since`、`RawNetwork`、`parse_net_dev_line` |
| `snapshot`／`sampling` | `Sample`、`SystemSnapshot`、`Cadence` |
| `CpuCollector` | `new`、`with_roots`、`refresh`、`refresh_frequencies`、`cores`、`aggregate_times`、`aggregate_busy_percent`、`sample`；`CpuSample` |
| `MemoryCollector` | `new`、`with_proc_root`、`refresh`、`snapshot`；`MemorySample` |
| `Processes` | `new`、`with_proc_root`、`with_parameters`、`set_scan_mode`、`enumerate_pids`、`refresh`、`refresh_pid`、`details`、`iter/get/len/is_empty/failures`、`ticks_per_second/page_size`；`Process`、`ProcessChanges`、`ProcessFailure`、`Availability`、`Detail`、`DetailOptions`、`ProcessDetails`、`ScanMode` |
| `DiskIoCollector` | `new`、`with_proc_root`、`refresh`、`iter/get/sample`；`DiskDevice` |
| `Filesystem`／`Mounts` | `Filesystem::new/refresh/path/capacity/last_error/sample`；`Mounts::new/with_proc_root/refresh/refresh_capacities/iter/iter_mut/sample`；`Mount` |
| `Networks` | `new`、`with_proc_root`、`refresh_counters/refresh_details/refresh`、`iter/get/sample`；`Network`、`InterfaceDetails`（含 `is_up/is_running/is_loopback`）、`InterfaceAddress`、`AddressSource` |
| `Hardware` | `new`、`with_sys_root`、四个显式刷新方法、`temperatures/gpus/info/inventory_sample`；`Gpu`、`HardwareInfo`、`HardwareValue::value`、`TemperatureSensor::celsius`、`TemperatureSource` |
| `System` | `new`、`refresh_cpu/memory/processes/process/networks/network_counters/disk_io/mounts/basic`；对应借用 getter、`process_table` 与 `process_collector` |
| `LinuxCollector` | `new`、`with_proc_root`、`cpu_times`、`memory`、`processes`；`ProcessRecord`、`LinuxError::{Io, Parse}` |

精确签名和所有公开字段以 `cargo doc --no-deps --all-features` 生成的 rustdoc 为准。
