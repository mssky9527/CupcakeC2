package config

import (
	"encoding/json"
	"os"
)

type ServerConfig struct {
	AdminPort int    `json:"admin_port"`
	AdminUser string `json:"admin_user"`
	AdminPass string `json:"admin_pass"`
}

func LoadConfig() (*ServerConfig, error) {
	// Default configuration
	config := &ServerConfig{
		AdminPort: 9999,
		AdminUser: "admin",
		AdminPass: "cupcake123",
	}

	configFile := "config.json"
	if _, err := os.Stat(configFile); os.IsNotExist(err) {
		// Create default config file if it doesn't exist
		data, _ := json.MarshalIndent(config, "", "  ")
		_ = os.WriteFile(configFile, data, 0644)
		return config, nil
	}

	// Read existing config
	data, err := os.ReadFile(configFile)
	if err != nil {
		return nil, err
	}

	if err := json.Unmarshal(data, config); err != nil {
		return nil, err
	}

	return config, nil
}
