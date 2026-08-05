# Changelog

本文件记录 Cupcake C2 产品版本变更。格式大致遵循 [Keep a Changelog](https://keepachangelog.com/)。

---

## [4.0.0] — 2026-08-05

**对比基线：`v3.0.5`**

本版是一次 **产品级大版本**，在远程桌面、文件传输、模块信任链、面板安全边界与运维可观测性上相对 3.0.5 做了系统性加固与能力升级。  
本地仓库根目录 `VERSION` = `4.0.0`；Git 标签 **`v4.0.0`**。

### 摘要（相对 v3.0.5 你得到了什么）

| 领域 | v3.0.5 痛点 / 现状 | v4.0.0 修复与改进 |
|------|-------------------|-------------------|
| 大文件上传 | 控制面分块 / base64 易超时、难背压 | Yamux **FILE `0x0E`** 二进制流；`.cupcake.part` 暂存 + 成功后原子 rename；Windows 错误路径 **先关句柄再删 part** |
| 远程桌面 | 能力偏弱 / 易拖垮会话 | L2 desktop 路径完善；RDP 端口转发；默认监听 **loopback**；双向 **idle 超时**；可选 **desktop_worker** 进程隔离（Job Object） |
| 模块 / 插件信任 | 弱校验 / 易被替换 | **HMAC trustchain** + 版本防回滚；插件 **SHA-256** 部署前校验（空 hash 默认拒绝） |
| 面板权限 | 角色边界不清晰 | 完整 **viewer / operator / admin** 路由 RBAC；高危写操作收紧 |
| MCP | 默认 token / 黑名单易误开 | **fail-closed** 白名单；默认只读；审计日志；客户端强制 `C2_API_TOKEN` |
| 公开 Stager | 可被扫、可被刷 | **IP 限速 / 命中次数 / TTL / 审计**（`pkg/stagerguard`） |
| Worker 稳定性 | 管道堵死、输出无界 | Job Object 资源上限；stdout 先读后等；输出 2MiB 封顶；会话退出 `stop_all` |
| 传输安全 | 缺 key 仍可能明文/半开 | Noise PSK **强制**；session crypto 拒绝空 key |
| 运维 | 缺少健康/指标 | `/health`、`/api/metrics`；任务日志保留清理；磁盘配额与上传上限 |
| 仓库卫生 | 本地 `config.json` 易带密码进库 | **停止跟踪** `server/config.json`，改用 `config.example.json` |

---

### 新增功能

#### 文件传输（FILE 流）

- 新增协议 **Yamux stream type `0x0E`（FILE）**，服务端与 agent 对称实现：
  - 客户端：`Client/core/src/file_stream.rs`
  - 服务端：`server/pkg/utils/file_stream.go` + `UploadViaYamux` / `OpenDownloadViaYamux`
- **Put（面板 → agent）**：`chunk_len(u32 BE) + data`，`chunk_len=0` 表示 EOF；写入 `path.cupcake.part`，成功后 rename 为最终路径。
- **Get（agent → 面板）**：status + size + 原始文件体。
- 控制面 `file_upload_chunk` 对 TCP/Yamux agent **降级为兜底**；主路径为二进制流。
- 大文件上传相关 admin HTTP：`ReadTimeout/WriteTimeout` 对长连接场景已做产品侧对齐（见 hardening 文档）。

#### 远程桌面 / RDP

- 完善 **L2 desktop** 模块与面板 Remote Desktop 流程。
- **RDP 端口转发**（agent → 目标 3389 via SOCKS/yamux DESKTOP `0x0D`）。
- Agent 侧 `desktop_bridge`：**120s 读空闲超时**，避免半开连接挂死。
- Server 侧：pipe 两端 idle deadline；**每 agent 并发连接上限**；默认 **`127.0.0.1` 监听**（`CUPCAKE_DESKTOP_LISTEN_HOST` 可覆盖）。
- 新增 **desktop_worker** 骨架与 opt-in 路径（`CUPCAKE_DESKTOP_WORKER=1`）；失败可回退 in-process bridge，避免回归。
- 辅助能力：`rdp_enable` 等启用/探测相关逻辑。

#### 模块 / 插件信任

- `pkg/trustchain`：对 module 元数据做 **HMAC-SHA256** 规范串签名校验。
- **版本防回滚**（`RollbackGuard`）：拒绝低于已发布版本的包。
- 插件上传记录 SHA-256；`DeployPlugin` 前 **VerifyPluginHash**；空 hash 默认 fail-closed（实验室迁移：`CUPCAKE_ALLOW_LEGACY_PLUGIN_HASH=1`）。

#### 安全与权限

- **Origin 严格校验**（CORS / WS）：按 scheme/host/port 解析，拒绝畸形与恶意子域。
- **MCP**：
  - 端点 **显式白名单** + 读/写能力；默认只读。
  - 高危写（文件删/传、杀进程、插件/模块推送、隧道等）**移出 allowlist**。
  - MCP 拒绝/放行写入审计日志。
  - MCPClient 取消硬编码 token，缺 `C2_API_TOKEN` 直接启动失败；command_guard 不可通过配置关闭。
- **RBAC**：viewer / operator / admin 路由矩阵（命令、文件、隧道、模块、生成器等分层）。
- 改密后 **会话 token 轮换**，旧 token 立即失效。
- 公开 **stager** 路径：每 IP 限速、每 id 最大下载次数、审计事件。

#### Worker / 进程隔离

- iso_host / inject 路径：stdout **先读线程再 Wait**，修复管道满导致的死锁。
- 输出 **有界读取**（默认 2MiB），超限杀 Job/进程。
- Job Object：**fail-closed**（限配失败则不创建“裸”job）；进程数 / 内存 / CPU 时间上限；kill-on-close。
- Agent 会话结束与自毁前 **`module_supervisor.stop_all()`**。

#### 运维与工程

- 健康检查：`health_controller`。
- 管理端指标：`/api/metrics`（JSON，非 Prometheus 全文生态）。
- 磁盘配额 / 上传体积门禁 / agent UUID 校验。
- 任务日志按天保留清理（默认 7 天）。
- WebSocket 短期 **ticket** 鉴权（PTY/shell/build logs 等升级路径）。
- GitHub Actions CI（`go vet` / `go test -tags nodonut` 等）。
- 文档：`SECURITY_HARDENING.md`、`MODULE_WORKER_ISOLATION.md`、`DESKTOP_MODULE_DESIGN.md`、`POSTEX_WORKLIST.md` 同步。

---

### 修复（相对 v3.0.5 / 早期 0.0.x 线）

- **文件上传中断 / 失败后 `.part` 残留（Windows）**：错误分支必须 **先 `drop(file)` 关闭句柄**，再 `remove_file`；否则句柄占用导致删除失败。
- **文件上传易失败 / 控制面超时**：主路径改为 yamux 二进制 FILE 流，减少 base64 膨胀与控制面挤占。
- **Inject/iso_host 大输出卡死**：修复 Wait 前未消费 stdout 的死锁。
- **Desktop / RDP 半开连接**：agent 与 server 双侧 idle 超时，避免 ESTABLISHED 僵尸占用。
- **缺 Noise key 仍建连**：TCP/WS/bind 在无 PSK 时拒绝建立会话。
- **Bind 地址被强改 0.0.0.0**：保留配置 host；仅端口时默认 loopback。
- **MCP / 面板权限过宽**：白名单 + RBAC + 高危写隔离。
- **Stager 被扫刷**：限速、命中上限、过期删除。
- **插件可被同名脏文件替换**：部署前 hash 校验。
- **改密后旧 token 仍可用**：强制轮换。
- **仓库泄露本地管理员口令风险**：不再提交 `server/config.json`。

---

### 破坏性变更 / 迁移注意

1. **MCP**：必须配置有效 `C2_API_TOKEN`；默认只读且 allowlist 更严——旧自动化若依赖上传/杀进程/推模块，需改走面板 admin 或收紧后的策略。
2. **插件**：无 hash 的旧记录默认不可部署，实验室需设 `CUPCAKE_ALLOW_LEGACY_PLUGIN_HASH=1` 或重新上传插件。
3. **模块包**：需符合 trustchain 签名与版本规则，低版本包会被拒。
4. **大文件上传**：TCP agent 依赖 Yamux FILE；无 Yamux 的 WS/DNS 仍可能走控制面分块兜底（能力与稳定性不同）。
5. **Desktop 监听**：默认仅本机 `127.0.0.1`，需要远端连 mstsc 时显式设置 `CUPCAKE_DESKTOP_LISTEN_HOST`。
6. **配置**：从 `config.example.json` 复制为本地 `config.json`，勿把真实口令提交回仓库。
7. **Agent 与 Server 需配套**：`CUPCAKE_WIRE_SEED` / AES / Noise 参数必须与 agent 构建一致。

---

### 已知问题（不阻塞 4.0.0，后续单独修）

- **上传失败后 yamux 流关闭，agent 读端偶发感知不到 FIN**（连接仍 ESTABLISHED、`.part` 可能残留）：与 Go `hashicorp/yamux` v0.1.2 关流语义 / agent 侧无读超时兜底相关。  
  **建议后续**：agent put 循环加 idle 读超时；或 server 失败路径更明确收尾（慎用整会话掐 TCP）。

---

### 版本引用

| 项 | 值 |
|----|-----|
| 标签 | `v4.0.0` |
| 对比 | `v3.0.5` → `v4.0.0` |
| 仓库 | https://github.com/yellatiamo/CupcakeC2 |

---

## [3.0.5] — 基线

产品仓库历史标签 **`v3.0.5`** 作为 4.0.0 的对比基线。  
4.0.0 在能力与安全边界上属于跨代升级；详细 diff 以本仓库 `v4.0.0` 树与上文条目为准。
