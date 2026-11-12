# 开发与验收进展

更新日期：2026-11-12（UTC）。当前版本为 **0.1.0 发布候选**；Linux 实现、测试和本地基准已形成可核验交付，crates.io 与 GitHub Release 的实际发布仍待合并后的流程确认。完整证据与限制见 [中文验收报告](acceptance-report.zh-CN.md)。

## 已完成的工程里程碑

- [x] Core、procfs 解析器、时间戳、差分、PID 身份与调用者驱动的采样间隔；核心可在无 `std`、无分配器环境编译。
- [x] Linux CPU、Memory、Process、Disk 和 Network 独立采集器，以及延迟初始化的 `System` 组合 API。
- [x] 进程全表／单 PID 刷新、生命周期与失败状态、`statm` RSS、按需 executable／cmdline／UID／GID；保留缓冲与状态。
- [x] `getdents64` 与标准库枚举路径、固定 PID 目录的多文件读取、原生 ABI 校验。
- [x] 磁盘容量、挂载发现与块 I/O；网络计数器、IPv4／IPv6、MAC、MTU、接口状态、可用链路速率和受限 netlink 环境的后备路径。
- [x] 可选 DMI、thermal／hwmon 与 DRM／驱动 sysfs 硬件信息；缺失值有明确状态。
- [x] 单元、fixture、边界、并发、权限、PID 复用、快速创建退出、资源持续运行测试。
- [x] Ubuntu 22.04／24.04、Linux aarch64、Debian/glibc、Alpine/musl、Windows/macOS core、MSRV、bare-metal／WASM core 和 benchmark 构建的 CI。
- [x] 固定 sysinfo `=0.40.3` 的独立 benchmark、采集范围验证、原始延迟与分配记录、保留堆与峰值 RSS、100／500／1,000／5,000 实际进程规模。
- [x] README、可编译示例、[语义](semantics.md)、[平台](platforms.md)、[架构](architecture.md)、基准与验收文档。
- [x] 自动版本 bump、两个 lockfile 同步、候选提交完整测试、crates.io 校验与 GitHub Release 工作流；兼容两种 token 名称。

## 已核验的执行证据

| 项目 | 结果 | 对应证据 |
|---|---|---|
| 原生代码提交 | `89e6c6c020869f3e6b37ea393bfc1040bd336d71` | [网络身份／计数器基线修复](https://github.com/LIghtJUNction/zsys/commit/89e6c6c020869f3e6b37ea393bfc1040bd336d71) |
| 分支 CI | 11 个 job 全部成功 | [运行 43198501256](https://github.com/LIghtJUNction/zsys/actions/runs/43198501256) |
| PR 测试合并 CI | 11 个 job 全部成功；测试合并 SHA `4a012c3132f0b948de901821e4f25b2b986d06fd` | [运行 43198509342](https://github.com/LIghtJUNction/zsys/actions/runs/43198509342) |
| Linux 全功能测试 | 22 个单元测试＋33 个集成测试通过，另 2 个文档测试通过 | [Ubuntu 24.04 日志](https://github.com/LIghtJUNction/zsys/actions/runs/43198509342/job/131115536770) |
| 核心测试 | `no-default-features` 的 15 个单元测试与 2 个文档测试通过 | 同上；跨目标编译见 CI |
| 连续资源测试 | 60.003 秒／3,968,000 次与 60.006 秒／4,514,816 次；两次热路径分配、保留字节增长、FD 增长均为 0 | [分支运行](https://github.com/LIghtJUNction/zsys/actions/runs/43198501256/job/131115508106)、[PR 运行](https://github.com/LIghtJUNction/zsys/actions/runs/43198509342/job/131115536207) |
| 本地最终基准 | 公平 `statm` RSS 路径；真实进程数与采样范围记录完整 | [overview.json](../benchmarks/evidence/overview.json)、[final-table.csv](../benchmarks/evidence/final-table.csv)、[原始数据](../benchmarks/evidence/final-raw.tar.gz) |

CI 结论对应表中指定的提交，不自动代表未来变更。资源测试只覆盖稳定单 PID 的持续刷新及反复创建销毁；它没有证明所有模块在任意工作负载下永不泄漏。

## 尚未完成或尚待验收

- [ ] 合并 [PR #1](https://github.com/LIghtJUNction/zsys/pull/1) 后，确认发布候选的完整测试与包校验。
- [ ] 确认 [crates.io `zsys/0.1.0`](https://crates.io/crates/zsys/0.1.0) 发布、索引与校验和匹配。
- [ ] 确认对应 Git Tag 与 [GitHub Release](https://github.com/LIghtJUNction/zsys/releases) 创建成功。
- [ ] 独立 GitHub benchmark [运行 43198501217](https://github.com/LIghtJUNction/zsys/actions/runs/43198501217) 完成并审查产物；本记录更新时仍在运行。
- [ ] 本地完整网络与所有模块组合的同范围对比：sysinfo 在当前受限 namespace 返回空接口，公平性门禁阻止生成速度倍率。
- [ ] 10,000 进程最终同范围基准：此前开发运行在 6,340 个实际可见进程时触发可用内存保护；旧 RSS 路径结果不能代替最终数据。
- [ ] 进一步优化未达目标的 CPU、Memory、块 I/O、500 个及以上全进程刷新和 P99 延迟。
- [ ] Windows、macOS、FreeBSD、Android、iOS 原生后端；当前这些平台的 core 可编译能力不等于原生采集完成。

## 性能判断的范围

本地同范围数据表明，zsys 在内存、进程与限定挂载容量等多条路径上降低了中位延迟，并在已测稳定热路径中消除了 Rust 分配请求。CPU 的真实间隔采样中位数较低，但 P99 为 **514.797 μs**，高于 sysinfo 的 **231.166 μs**。5,000 进程全刷新中位数为 **43.247 ms**，仍远高于 1 ms 目标。CPU／Memory 的保留缓冲也比对应 sysinfo 路径大。因此当前结果不能概括为“所有路径更快、更省内存”，更不能宣布用户的最终发布验收已经全部完成。数字、采样范围与原始证据见 [验收报告](acceptance-report.zh-CN.md) 和 [BENCHMARKS.md](../BENCHMARKS.md)。
