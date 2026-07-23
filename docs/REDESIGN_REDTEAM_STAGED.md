# Cupcake 红队向 Staged 架构改造说明

> 状态：Phase 0 冻结 + 评审补充已并入  
> 用途：红队演练默认交付链  
> 日期：2026-07-23

---

## 1. 目标

| 目标 | 说明 |
|------|------|
| 演练窗口内可上线、可作业 | 不追求 APT 级数年潜伏 |
| 降低首包查杀面 | 静态 + 短时行为 |
| 能力按需、可丢可换 | 重模块脏了不影响通信核 |
| 面板协议尽量兼容 | 降改造成本 |
| Stage0 作为「必须活下来的唯一样本」 | 见 §8 对抗深化 |

### 非目标

- 永久免杀保证
- 以 full/ETW·AMSI 堆料当银弹
- 恢复进程注入产品线
- 一次迁完所有模块

---

## 2. 三层架构

```text
L0 Delivery   投递壳（脚本/小 loader/shellcode 入口）— 可频繁换皮
L1 Stage0     通信核（回连/心跳/任务路由/模块加载器）— 默认落盘目标
L2 Modules    按需能力（shell/fs/proc/socks/bof/dotnet/plugin）— 优先内存
```

| 业界 | 本项目 |
|------|--------|
| Stager | L0 |
| Beacon | L1 Stage0 |
| BOF/Plugin | L2 Module |

**原则：** 首包 ≠ 全功能；模块哈希与 Beacon 解耦；重对抗代码跟模块走。

---

## 3. 能力边界

### L1 允许（反向产品默认 = cargo `minimal`）

- 单协议传输（构建时选定）
- 加解密 / 注册 / 心跳 / jitter
- 任务路由 + 模块加载器
- **hybrid 终端 / PTY**（上线即用，不加载模块）
- **文件管理**（上线即用）
- **进程管理**（上线即用）
- 精简 systeminfo
- **启动 delay/jitter**（§8.1）

### L1 禁止 / 迁 L2（重能力）

| 模块 | 内容 |
|------|------|
| `mod_bof` | BOF + 重 syscall 面 |
| `mod_dotnet` | CLR 内存执行 |
| `mod_socks` | SOCKS（正向/standard 可内置） |
| `mod_plugin` | 插件系统 |
| `mod_shell` | 可选实验：纯 Stage0 才需要；**反向产品不依赖** |

### 产品冻结决策

| 项 | 决策 |
|----|------|
| 反向客户端 | **`minimal` + 回连协议**：shell/fs/proc/pty 内置 + loader；~0.8MB |
| 正向客户端 | **同一 `minimal` + bind**：能力与反向相同；仅连接方向不同 |
| 模块短时落盘 | **允许（可配置关）**，优先内存 |
| 第一重模块 | **`bof` / `dotnet`**（非 shell） |
| 纯 Stage0 `beacon` | 可选超薄非产品路径 |

---

## 4. 交付链（红队默认）

```text
面板生成 Stage0 +（可选）L0 一键命令
  → 目标执行 L0/L1
  → 延迟+抖动后回连
  → 操作触发模块下发 → 加载 → 作业 → 可选卸载
```

变体：直投 Stage0 / 脚本下载 / shellcode 加载 Stage0 / 双阶段拉 body。

---

## 5. 模块 ABI（首版务实）

```text
mod_init(ctx) -> status
mod_invoke(cmd_type, payload) -> result
mod_shutdown()
```

- 形态：原生 DLL（或等价），加密封装 `.mod.enc`
- 加载：内存优先 → 失败短时落盘 + 删 +（可选）时间戳处理
- 协议：保留 `shell` / `file_*` 等命令类型，L1 负责「缺模块则拉」

---

## 6. 分阶段

| Phase | 内容 | 周期 |
|-------|------|------|
| 0 | 本文档 + 决策冻结 | 0.5d |
| 1 | 瘦 Stage0 + Builder 双产物 + 加载器 stub | 1–2w |
| 2 | `mod_shell` 闭环 | 1–2w |
| 3 | `mod_fs` / `mod_proc` | 1–2w |
| 4 | socks / plugin / bof / dotnet | 2–4w |
| 5 | 交付硬化 + OPSEC 手册（§8） | 持续 |
| 6 | 收敛 monolith 默认入口 | 可选 |

---

## 7. 已有基础（保持并下沉到合适层）

| 能力 | 归属 |
|------|------|
| Syscall 懒加载 + Gadget 池 + Stack Spoof | **L2 敏感模块**（L1 默认不链全量） |
| IAT 收紧 / PEB 动态解析 | L1 最小集 + L2 |
| 进程 Nt* / 混合终端避 cmd | **mod_shell / mod_proc** |
| staged 边界清晰 | 本文档 |

---

## 8. 评审补充：Stage0 与 staged 对抗深化（最高优先级）

> Stage0 是「必须活下来的唯一样本」；其他都可丢可换。

### 8.1 Stage0 行为与体积（P0）

| 项 | 要求 |
|----|------|
| 启动 | **延迟上线 + 强 jitter**；禁止启动即枚举、乱碰文件、网络外动作 |
| 导入/字符串 | 压缩 IAT；敏感串运行时解密或间接引用 |
| 内存足迹 | 少线程、少怪异堆模式，降低典型 Beacon 布局 |
| 启动链 | 尽量像更新程序/脚本宿主拉起（与 L0 配合） |

### 8.2 模块加载/卸载 OPSEC（P0–P1）

| 项 | 要求 |
|----|------|
| 加载 | 内存优先；落盘降级要克制（短时 + 删除 + 可选时间戳） |
| 时机 | **禁止**心跳后批量拉模块；仅操作员需求触发 |
| 卸载 | 释放映射/回调/句柄，减少残留 |
| 模块自身 | 轻量 Layer-A（syscall/栈伪装按需），不假设「进内存就安全」 |

### 8.3 网络与 C2 通道（P1）

| 项 | 要求 |
|----|------|
| 心跳/任务包 | 控制大小、间隔；注意 TLS/JA3 类指纹 |
| 画像 | 贴近正常客户端/CDN/云 API 风格（Malleable 方向） |
| 协议 | 默认单协议构建；**要能快速换协议重新生成** |
| 基础设施 | 阶段隔离域名/IP，避免一把梭 |

### 8.4 行为序列与时间（P1–P2）

| 项 | 要求 |
|----|------|
| 节奏 | 侦察→移动→执行之间留间隔与噪声 |
| 避免 | 短时间完整攻击链自动化 |
| 敏感操作前后 | 可插入干扰/无关动作 |
| 用户活动感知 | 可选：有人值守时降敏（红队可配置） |

### 8.5 环境感知与自适应（P2）

| 项 | 要求 |
|----|------|
| EDR/AV 感知 | 检测常见进程/驱动，**不主动杀软** |
| 行为档位 | 强 EDR 环境更克制、模块更谨慎 |
| 沙箱识别 | 延迟/交互/硬件等，**避免检测逻辑本身成签名** |

### 8.6 残留与清理（P1–P2）

| 项 | 要求 |
|----|------|
| 模块卸载后 | 内存、句柄、命名对象 |
| 临时文件/注册表/日志 | 明确清理策略 |
| Stage0 退出 | 可选平滑退出/自删 |
| 取证时间线 | 减少可关联磁盘痕迹 |

### 8.7 架构持续演进（贯穿）

| 项 | 要求 |
|----|------|
| L0 多样性 | 投递方式可轮换，降家族关联 |
| 模块粒度 | 单模块勿过大过全，脏了只换一个 |
| 密钥分离 | Stage0 失陷不能直接解历史全部模块 |
| 版本回滚 | 模块可停用，不影响 Stage0 存活 |

### 8.8 与 Phase 映射

| 评审项 | 主要 Phase |
|--------|------------|
| 8.1 启动极简、字符串、体积 | Phase 1 + 5 |
| 8.2 加载卸载 OPSEC | Phase 2（加载器实装）起 |
| 8.3 网络画像 | Phase 1 基础 jitter；Phase 5 深化 |
| 8.4 行为节奏 | OPSEC 手册 + 面板/操作规范；可选引擎支持 |
| 8.5 环境感知 | Phase 5+（克制实现） |
| 8.6 清理 | Phase 2+ 模块生命周期 |
| 8.7 密钥/粒度/L0 | Phase 1–5 持续 |

---

## 9. 成功标准

1. 默认交付 = Stage0，不是 monolith standard  
2. 无模块时仅注册/心跳（或明确 `module_required`）  
3. 按需模块恢复接近现 standard 作业能力  
4. 首包体积与敏感代码面明显下降  
5. 具备红队交付与模块使用 OPSEC 说明（§8 + 操作手册）

---

## 10. Profile 命名

| Profile | 含义 |
|---------|------|
| `beacon` | 默认红队路径：仅 L1 |
| `standard` / `legacy-standard` | 旧 monolith（兼容） |
| `minimal` / `full` | 暂保留；后续 full 不再作为免杀路径推荐 |

---

## 11. 实现状态（冻结）

> 更新：2026-07-23 — 产品边界：日常作业内置，BOF/.NET 等才模块化。

| 项 | 状态 |
|----|------|
| **反向 / 正向** = 同为 cargo `minimal`（仅协议方向不同，~0.8MB） | ✅ |
| `module_for_command`：仅 bof/dotnet/plugin；shell/fs/proc 为 None | ✅ |
| module_loader CKMS pipeline | ✅ |
| 模块仓库 UI（重能力） | ✅ |
| pure `beacon` 超薄 | feature 保留，非面板默认 |
| `mod_bof` / `mod_dotnet` 按需 DLL | ✅ `Client/modules/bof` + `dotnet` |

### 构建

```text
反向: cargo build -p cupcake-core --no-default-features --features "ws,minimal" --release
正向: cargo build -p cupcake-core --no-default-features --features "tcp_bind,minimal" --release
BOF 模块: cargo build -p cupcake-mod-bof --release
         → server/storage/modules/bof.bin
.NET 模块: cargo build -p cupcake-mod-dotnet --release
         → server/storage/modules/dotnet.bin
```

### 重能力隔离执行（PPID 伪装短命宿主）

- Agent **不**在本进程跑 BOF/CLR；下发任务到 `cupcake-iso-host`
- `CreateProcess` + `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS`（父进程伪装 explorer/RuntimeBroker）
- 管道下发 CIS1 帧（内存载荷），结果回传后宿主退出；临时 PE 删除
- 服务端登记：`storage/modules/iso_host.bin`（= cupcake-iso-host.exe）
- 插件页运行 BOF/.NET 前自动 stage `iso_host`
- 进程树上尽量不是 Agent 子进程；EDR 仍可能通过其它遥测关联
