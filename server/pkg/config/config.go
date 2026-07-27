package config

import (
	"encoding/json"
	"os"
)

type ServerConfig struct {
	AdminPort int    `json:"admin_port"`
	AdminUser string `json:"admin_user"`
	AdminPass string `json:"admin_pass"`
	// Bind address for admin panel (public panel: keep 0.0.0.0)
	AdminBind string `json:"admin_bind"`
	// Optional TLS for admin panel (public HTTPS)
	AdminTLS       bool   `json:"admin_tls"`
	AdminTLSCert   string `json:"admin_tls_cert"`
	AdminTLSKey    string `json:"admin_tls_key"`
	AdminTLSAuto   bool   `json:"admin_tls_auto"` // generate self-signed if cert/key empty
	// DataDir overrides CUPCAKE_DATA_DIR when set
	DataDir string `json:"data_dir"`
	// Agent stale after this many seconds without LastSeen update
	AgentStaleSecs int `json:"agent_stale_secs"`
}

func LoadConfig() (*ServerConfig, error) {
	// Safer defaults for new installs: loopback bind, empty pass → random at first boot.
	// Production public panel: set admin_bind/admin_pass in config.json explicitly.
	config := &ServerConfig{
		AdminPort:      9999,
		AdminUser:      "admin",
		AdminPass:      "", // empty → bootstrap generates random (or CUPCAKE_FORCE_DEV_PASS)
		AdminBind:      "127.0.0.1",
		AdminTLS:       false,
		AdminTLSAuto:   false,
		AgentStaleSecs: 180,
	}

	configFile := "config.json"
	if _, err := os.Stat(configFile); os.IsNotExist(err) {
		data, _ := json.MarshalIndent(config, "", "  ")
		_ = os.WriteFile(configFile, data, 0644)
		return config, nil
	}

	data, err := os.ReadFile(configFile)
	if err != nil {
		return nil, err
	}

	if err := json.Unmarshal(data, config); err != nil {
		return nil, err
	}
	if config.AdminBind == "" {
		config.AdminBind = "127.0.0.1"
	}
	if config.AgentStaleSecs <= 0 {
		config.AgentStaleSecs = 180
	}

	return config, nil
}
