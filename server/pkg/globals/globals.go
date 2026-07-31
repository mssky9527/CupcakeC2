package globals

import (
	"io"
	"net"
	"net/http"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/gorilla/websocket"
	"github.com/hashicorp/yamux"
)

// MessageWrapper matches the Client's top-level JSON
type MessageWrapper struct {
	MsgType string      `json:"msg_type"`
	Payload interface{} `json:"payload"`
}

// CommandPayload (Server -> Agent)
type CommandPayload struct {
	CommandType    string `json:"command_type"`
	CommandContent string `json:"command_content"`
	Path           string `json:"path,omitempty"`
	ReqID          string `json:"req_id"`
	Data           string `json:"data,omitempty"` // For file upload
}

// ResponsePayload (Agent -> Server)
type ResponsePayload struct {
	Stdout string `json:"stdout"`
	Stderr string `json:"stderr"`
	Path   string `json:"path,omitempty"`
	ReqID  string `json:"req_id"`
}

type Client struct {
	WebSocketConn *websocket.Conn `json:"-"`
	TCPConn       net.Conn        `json:"-"`
	YamuxSession  *yamux.Session  `json:"-"`
	Transport     string          `json:"transport"` // "websocket", "tcp"
	TCPWriteMu    sync.Mutex      `json:"-"`         // 🐛 修复: 防止并发写 TCPConn 导致消息错位
	UUID           string          `json:"uuid"`
	Hostname       string          `json:"hostname"`
	OS             string          `json:"os"`
	Username       string          `json:"username"`
	Arch           string          `json:"arch"`
	IP             string          `json:"ip"`
	EncryptMode    string          `json:"-"`
	EncryptKey     string          `json:"-"`
	EncryptionSalt string          `json:"-"`
	ObfuscateMode  string          `json:"-"`
	// Phase 1: Noise-like ephemeral session key (forward secrecy)
	NoiseSessionKey [32]byte       `json:"-"`
	// Cached Argon2id-derived static session key (set once after register / first use)
	SessionKey     []byte          `json:"-"`
	CommandChannel chan string     `json:"-"`
	OutputChannel  chan string     `json:"-"`
	// outputCloseOnce ensures OutputChannel is closed at most once (reconnect races).
	outputCloseOnce sync.Once `json:"-"`
	// Protect concurrent WebSocket writes (admin shell, multi-subscriber)
	WSWriteMu      sync.Mutex      `json:"-"`
	ListenerID     string          `json:"listener_id"`
	ListenerPort   int             `json:"listener_port"`
	CachedPlugins  map[string]bool `json:"-"`
	PluginMutex    sync.RWMutex    `json:"-"`
}

// CloseOutputChannel closes OutputChannel at most once (safe under reconnect/offline races).
func (c *Client) CloseOutputChannel() {
	if c == nil {
		return
	}
	c.outputCloseOnce.Do(func() {
		if c.OutputChannel != nil {
			close(c.OutputChannel)
		}
	})
}

// PTYSession 代表一个底层的异步 PTY 会话状态（支持断线重连与多端复用，类似 Tmux/CobaltStrike）
type PTYSession struct {
	Stream        io.ReadWriteCloser `json:"-"`
	HistoryBuffer []byte             `json:"-"`
	Subscribers   sync.Map           `json:"-"` // *websocket.Conn -> bool
	Mutex         sync.RWMutex       `json:"-"`
}

type Listener struct {
	ID                string       `json:"id"`
	BindIP            string       `json:"bind_ip"`
	Port              int          `json:"port"`
	Protocol          string       `json:"protocol"`
	PublicHost        string       `json:"public_host"`
	Note              string       `json:"note"`
	EncryptMode       string       `json:"encrypt_mode"`
	EncryptKey        string       `json:"encrypt_key"`
	EncryptionSalt    string       `json:"encryption_salt"`
	ObfuscateMode     string       `json:"obfuscate_mode"`
	CustomPath        string       `json:"custom_path"` // e.g. /ws or /api/updates
	// Profile: expected malleable profile (gmail|outlook|aws|github|default); empty = any
	Profile           string       `json:"profile"`
	// ProfileStrict: reject handshake when profile headers/path do not match
	ProfileStrict     bool         `json:"profile_strict"`
	// DNS-specific fields
	NSDomain          string       `json:"ns_domain"`
	PublicDNS         string       `json:"public_dns"`
	// Heartbeat/Advanced config
	HeartbeatInterval int          `json:"heartbeat_interval"` // in seconds
	HeartbeatJitter   int          `json:"heartbeat_jitter"`   // 0-100 percentage
	MaxRetry          int          `json:"max_retry"`
	// 🔒 TLS Configuration (Phase 1 - Secure WebSocket)
	EnableTLS         bool         `json:"enable_tls"`
	TLSCertPath       string       `json:"tls_cert_path"`
	TLSKeyPath        string       `json:"tls_key_path"`
	TLSCertPEM        string       `json:"tls_cert_pem"`
	TLSKeyPEM         string       `json:"tls_key_pem"`
	// Status and server instances
	Status            string       `json:"status"`
	HTTPServer        *http.Server `json:"-"`
	DNSServer         interface{}  `json:"-"`
	TCPServer         net.Listener `json:"-"`
}

var (
	Clients           sync.Map
	PTYState          sync.Map
	ActivePTYSessions sync.Map // UUID -> *PTYSession (用于保持后端 PTY 历史和分发)
	LogsMap           sync.Map
	LogsMapMu         sync.Mutex // serializes LoadOrStore+append+Store on LogsMap
	PendingResponses  sync.Map
	Listeners         sync.Map
	// Upgrader for agent listener WebSockets (Rust agents typically omit Origin).
	Upgrader = websocket.Upgrader{
		CheckOrigin: AgentCheckOrigin,
	}
	// AdminUpgrader for browser-facing panel WS (PTY / shell / build logs).
	// Empty Origin is rejected — modern browsers always send Origin on WS.
	AdminUpgrader = websocket.Upgrader{
		CheckOrigin: AdminCheckOrigin,
	}
)

// AgentCheckOrigin allows empty Origin (non-browser agents) and same-host/localhost.
func AgentCheckOrigin(r *http.Request) bool {
	origin := r.Header.Get("Origin")
	if origin == "" {
		return true
	}
	return originAllowed(origin, r.Host)
}

// AdminCheckOrigin rejects empty Origin and requires localhost or same-host match.
func AdminCheckOrigin(r *http.Request) bool {
	origin := r.Header.Get("Origin")
	if origin == "" {
		return false
	}
	return originAllowed(origin, r.Host)
}

func originAllowed(origin, host string) bool {
	if strings.Contains(origin, "localhost") || strings.Contains(origin, "127.0.0.1") {
		return true
	}
	if host != "" && (strings.Contains(origin, host) || strings.HasSuffix(origin, "://"+host)) {
		return true
	}
	return false
}

// reqCounter is lock-free monotonic IDs for command correlation.
var reqCounter uint64

// GetNextReqID returns a globally unique monotonic request ID (thread-safe, lock-free)
func GetNextReqID() uint64 {
	return atomic.AddUint64(&reqCounter, 1)
}