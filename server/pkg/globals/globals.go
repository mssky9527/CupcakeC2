package globals

import (
	"io"
	"net"
	"net/http"
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
	CommandChannel chan string     `json:"-"`
	OutputChannel  chan string     `json:"-"`
	ListenerID     string          `json:"listener_id"`
	ListenerPort   int             `json:"listener_port"`
	CachedPlugins  map[string]bool `json:"-"`
	PluginMutex    sync.RWMutex    `json:"-"`
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
	PendingResponses  sync.Map
	Listeners         sync.Map
	Upgrader          = websocket.Upgrader{
		CheckOrigin: func(r *http.Request) bool { return true },
	}
	// 修复6: 使用 atomic 替代 Mutex，性能更高（无锁原子操作）
	reqCounter uint64
)

// GetNextReqID returns a globally unique monotonic request ID (thread-safe, lock-free)
func GetNextReqID() uint64 {
	return atomic.AddUint64(&reqCounter, 1)
}