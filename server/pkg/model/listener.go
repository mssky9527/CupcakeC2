package model

import (
	"time"
)

type Listener struct {
	ID                string    `gorm:"primaryKey" json:"id"`
	BindIP            string    `json:"bind_ip"`
	Port              int       `json:"port"`
	Protocol          string    `json:"protocol"`
	PublicHost        string    `json:"public_host"`
	Note              string    `json:"note"`
	EncryptMode       string    `json:"encrypt_mode"`
	EncryptKey        string    `json:"encrypt_key"`
	EncryptionSalt    string    `json:"encryption_salt"`
	ObfuscateMode     string    `json:"obfuscate_mode"`
	CustomPath        string    `json:"custom_path"`
	NSDomain          string    `json:"ns_domain"`
	PublicDNS         string    `json:"public_dns"`
	HeartbeatInterval int       `json:"heartbeat_interval"`
	HeartbeatJitter   int       `json:"heartbeat_jitter"`
	MaxRetry          int       `json:"max_retry"`
	Status            string    `json:"status"` // "Running", "Stopped", "Failed"
	CreatedAt         time.Time `json:"created_at"`
	UpdatedAt         time.Time `json:"updated_at"`
}
