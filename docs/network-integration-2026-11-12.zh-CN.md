# 主分支兼容集成与网络内存优化补充报告

记录日期：2026-11-12。代码提交：[`468922b`](https://github.com/LIghtJUNction/zsys/commit/468922b829180546510cdf8316c2d9f38af58806)，集成记录：[PR #2](https://github.com/LIghtJUNction/zsys/pull/2)。本报告补充已有[候选版本验收报告](acceptance-report.zh-CN.md)，不覆盖主分支的原始基准、发布失败记录或同期工作。

## 1. 集成边界

核对发现主分支已经推进到 `791a55f8c9ad1208db98feb36433e59eddc0ee32`，并包含更严格的 `statm` RSS 采集、受限网络环境后备路径、硬件能力、真实多进程规模基准和两次发布记录。独立实验分支 `work/acceptance-audit` 没有整支覆盖主分支；只移植兼容的网络分配优化和边界测试。

实验分支的完整报告保留在[提交 7070842](https://github.com/LIghtJUNction/zsys/blob/70708423cc2ce902a435d78ed74aac512db980c4/BENCHMARKS.md)。它的源码、宿主环境、RSS 路径和测量框架与当前主分支不同，尤其旧 stat-only 进程数字不能作为当前 statm 默认路径的公平性能结论。实验数据仅作为开发过程证据，不与主分支数据混算。原先测得的 25,894→1,318 字节网络优化也属于该实验分支；本次主分支的实际变化见下表。

## 2. 本次实际修改

完整网络刷新仍重新读取接口计数器、地址、掩码、ifindex、flags、MAC、MTU 和可用链路速率，没有把旧结果当成新采样，也没有减少约定字段。只做两项存储管理改进：sysfs 速率属性读取复用按需创建的缓冲区，初始 256 字节、最多 4 KiB；已经拥有的接口详情直接移动到快照，避免再次复制名称和地址数组。

这不是缓存采集结果。每次详情刷新仍打开并读取当前属性；负值、零、驱动 unknown 哨兵、溢出、属性消失和超长内容均保持不可用语义。仅刷新计数器时不创建详情缓冲区。主分支的 getifaddrs→只读 ioctl/procfs 后备路径、接口身份变化后重新建立基线、statm 读取及其他模块均保留。

新增测试覆盖速率属性变化、无效输入、4 KiB 边界、失败后的有效读取恢复、属性消失、仅计数器懒分配；原有接口复用测试追加了拥有型缓冲区移动而非复制的断言。没有删除测试或放宽 Clippy／格式门禁。

## 3. 实测方法与完整结果

本组使用主分支原有独立 Benchmark，固定 sysinfo 0.40.3。两次运行的非空网络字段范围验证均通过。优化前后各三轮，每轮每个实现 1,000 次完整网络刷新；另测独立 network_counters。宿主为 AMD EPYC 9V74、五个可见逻辑 CPU、四 CPU cgroup 配额、4 GiB 容器内存限制、Debian 13.3／Linux 6.18.57，Rust 1.99.1，release、thin LTO、一个 codegen unit。完整环境、作用域、二进制 SHA-256 与源码绑定均在原始归档。

**重要限制：继承的主分支 Benchmark 在计时区间内包含 Rust 分配计数器开销。以下延迟不是移除计数器后的独立延迟结果。** 分配次数较多的一方会额外承担计数器工作；本组适合确认分配优化及附带的调用分布，不能把倍率直接当成生产环境加速保证。共享宿主也没有锁频、CPU 绑核或独占调度。所有样本参与统计，P95／P99 使用 nearest-rank，不丢弃长尾。

| 指标 | zsys 优化前 | zsys 优化后 | sysinfo 优化后同组 |
|---|---:|---:|---:|
| 每组样本 | 3,000 | 3,000 | 3,000 |
| 平均耗时 μs | 31.935 | 25.051 | 67.588 |
| Median μs | 20.791 | 20.140 | 53.646 |
| P95 μs | 90.446 | 49.945 | 126.831 |
| P99 μs | 154.934 | 111.388 | 299.291 |
| 最大耗时 μs | 602.918 | 620.806 | 5,589.443 |
| 平均耗时倒数，次／秒 | 31,313 | 39,918 | 14,795 |
| Rust 分配／重分配请求，次／调用 | 19 | 13 | 73 |
| 请求分配字节，B／调用 | 9,292 | 934 | 6,821 |
| 单次额外存活 Rust 字节峰值 | 4,876 | 808 | 4,292 |
| 刷新前后保留 Rust 堆字节 | 10,842 | 11,258 | 874 |
| 子进程 VmHWM 最大值，B | 1,736,704 | 1,736,704 | 1,675,264 |

本次主分支每次完整网络刷新请求的分配字节减少 **89.95%**，但为复用缓冲区增加了 **416 B 保留堆空间**。峰值 RSS 没有下降，且此组 zsys 的保留堆和进程 RSS 高于 sysinfo。优化后的最大耗时也比自身优化前略高。这些不利结果一并保留；不能宣称所有内存指标或尾延迟均获胜。

仅网络计数器刷新仍是热路径零 Rust 分配，前后合并 Median 均为 2.013 μs。这是独立窄范围接口，没有构造 sysinfo 等价调用来宣称倍率。Rust 分配计数不覆盖 getifaddrs 等系统 C 函数的内部 malloc。VmHWM 包含二进制、运行时、collector 和样本存储，不是库独占堆大小。

## 4. 源码与原始数据

优化前网络代码来自主分支 `791a55f`，完整导出提交 `1d2875e327a27cd8cd60329ade1262e9f8fbb0c4` 只额外包含源码导出工作流。优化后源码提交为 `468922b829180546510cdf8316c2d9f38af58806`，树为 `6c9f604df0f0616a61af4adcd85412b4a655fd64`。没有将实验分支的数字重新标为这次主分支测量。

[主分支集成原始证据 ZIP](https://drive.google.com/file/d/1g9eANoLbT87ZGqTnrrWkICMQjjMGHQIU/view)已交付到用户云盘，访问需要相应 Drive 权限，未更改公开共享设置。文件 SHA-256：`30c01e463408a35f4c5a5dec5e3cebe8e92c865055763aab1a5fcb6ab44210d6`。内含前后源码、全部压缩原始样本、环境、范围检查、测试日志、独立复算脚本和校验清单。独立复算校验每个原始文件 SHA-256、样本数、均值／中位数／P95／P99／最大值及分配指标，再合并所有重复测量。

```sh
python3 benchmarks/run.py --output before-network --cases network network_counters --samples 1000 --repeats 3
# 切换到优化后的源码并重建，再以另一个输出目录运行同样参数。
python3 benchmarks/run.py --output after-network --cases network network_counters --samples 1000 --repeats 3
python3 benchmarks/audit_network_results.py before-network after-network --output analysis
```

实验分支的另一个归档也保留：[666 组开发测量及源码](https://drive.google.com/file/d/1YiHh8lXTpOLto-G4XgsQvN3ZP5JuQE-C/view)，SHA-256 `cfb5643778d33e4003622851a321407447c298292b15afba8176c7a5a39257dd`。该归档使用独立的延迟与分配二进制，但旧进程采集范围不同，不能替代当前主分支 RSS 公平性验证。

## 5. 测试与未解决事项

本地本次代码已通过 **57 项单元／集成测试、2 项文档测试**、无默认 feature 测试与严格 Clippy；独立 benchmark 构建和字段范围校验也通过。合并依赖 PR #2 对应提交的完整 CI，而不是引用旧提交的绿色结果。原有 Linux x86-64／AArch64、Debian／Alpine、MSRV、bare-metal／WASM 与 Windows／macOS 核心矩阵保留。

Windows／macOS 的原生采集后端仍未完成；核心可编译不代表原生采集已实现。通用 GPU 厂商遥测、小时／天级全模块故障与热插拔 soak、隔离宿主的稳定 P99，以及多项微秒／毫秒目标仍未全部达成。全进程默认路径保留 statm 的正确 RSS 范围，不以删掉读取来换取数字。

优先后续优化为：把当前主分支的延迟与分配测量分成独立构建并重新测量；评估有界 PID 文件描述符缓存及复用安全；减少完整网络元数据的额外遍历；在独占宿主重测规模与长尾。计数器-only API 的优势不能冒充完整采集的优势。

## 6. 发布状态与必须处理的阻塞

源码仍是 `0.1.0` 候选。已配置版本 bump、锁文件同步、变更记录、完整测试、crates.io 上传、校验和核对以及 GitHub Release。主分支接受 `CARGO_REGISTY_TOKEN` Secret，并映射到 Cargo 的 `CARGO_REGISTRY_TOKEN` 环境变量。

已实际读取[第二次发布日志](https://github.com/LIghtJUNction/zsys/actions/runs/43201957205/job/131128028423)：日志明确显示选择 `CARGO_REGISTY_TOKEN`，测试及打包成功，上传时 crates.io 返回 **403 Forbidden: authentication failed**。这不能靠继续改变量拼写解决，也不能据此断定究竟是过期、值无效还是权限不适用。需要在仓库 Actions Secret 中替换为有效且有 zsys 发布权限的 crates.io 凭据；不要将 Token 发到聊天或提交到代码。

在恢复授权后，应针对希望发布的准确源码重新触发 Release；只重跑旧失败 job 会仍发布旧候选 `6a6f450`，不会自动包含本次网络改进。未核验上传与 GitHub Release 的实际成功前，本报告不宣布最终验收完成。
