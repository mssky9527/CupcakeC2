# PTY 调试与修复指南 (v3.0.6)

## 修复内容

已修复客户端在建立终端连接时可能导致崩溃的问题，并增强了 Release 构建下的调试能力。

### 主要改进

1. **Release 诊断模式**
   - **核心修复**: 现在即使是 Release 构建，只要设置了 `RUST_LOG` 环境变量，也会自动启用日志系统并显示控制台窗口。
   - 自动禁用 `hide_console()`: 当检测到 `RUST_LOG` 时，客户端不再隐藏控制台，方便直接查看报错。
   - 全局 Panic 捕获: 所有崩溃现在都会尝试在日志中记录详细原因。

2. **PTY 系统稳定性增强**
   - **ConPty 生命周期管理**: 修复了 `NativePtySystem` 可能被过早释放导致的句柄失效问题。
   - **优雅降级**: 移除了 PTY 初始化和 Fallback 路径中的所有 `expect()`，改为 Result 错误处理，防止程序直接崩溃。
   - **详细步骤记录**: PTY 初始化的每个阶段（打开设备、生成 Shell、克隆 Reader）现在都有详尽的日志输出。

3. **编码与兼容性**
   - 增强了 Windows 8.1/10 的 ConPty 检测，失败时更稳定地回退到 PipeShell 模式。
   - 优化了 GBK <-> UTF8 的双向转码逻辑。

## 如何使用新版调试功能 (针对 Release 二进制)

如果你使用的是服务端生成的 `.exe` 文件（通常是 Release 构建）：

### 第一步：设置环境变量

在终端（CMD 或 PowerShell）中执行：

```powershell
# PowerShell
$env:RUST_LOG="debug"
.\agent_windows_amd64_xxxx.exe

# CMD
set RUST_LOG=debug
agent_windows_amd64_xxxx.exe
```

### 第二步：观察控制台

由于检测到 `RUST_LOG`，原本会自动隐藏的控制台窗口现在会保持可见，并输出类似以下内容：

```text
[INFO  cupcake_core] Logging initialized (Mode: Release-Diagnostic)
[INFO  cupcake_core] Stealth: hide_console() skipped due to active logging
[DEBUG cupcake_core::pty] [PTY] Initiating Cross-Generation Hybrid Shell...
[DEBUG cupcake_core::pty] [PTY] Attempting to initialize NativePtySystem...
...
```

## 崩溃排查 Checklist

1. **没有任何输出就退出?**
   - 检查 `RUST_LOG` 是否正确设置。
   - 查看是否输出了 `PANIC OCCURRED`。
   - 如果是 Windows 7/8，ConPty 会失败，请确认日志中是否有 `Falling back to PipeShell`。

2. **PTY 启动后显示 [!] PTY Stream Error?**
   - 检查服务端 Yamux 状态。
   - 查看客户端日志中的 `spawn_blocking join error`。

3. **中文乱码?**
   - 确认系统区域设置，客户端会自动检测并应用 GBK 解码。

## 构建建议

如果需要手动验证：

```bash
cd Client
cargo build -p cupcake-core
```

生成的二进制文件在 `target/debug/cupcake-core.exe`。
