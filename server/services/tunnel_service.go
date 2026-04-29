package services

import (
    "bufio"
    "encoding/base64"
    "encoding/binary"
    "fmt"
    "io"
    "log"
    "net"
    "net/http"
    "strconv"
    "strings"
    "sync"
    "cupcake-server/pkg/globals"
    "cupcake-server/pkg/model"
    "cupcake-server/pkg/store"
)

type Tunnel struct {
    Port      string `json:"port"`
    AgentID   string `json:"agent_id"`
    Type      string `json:"type"`   // "socks5" or "http"
    Status    string `json:"status"`
    Username  string `json:"username"`
    Password  string `json:"password"`
    listener  net.Listener
}

var (
    activeTunnels = make(map[string]*Tunnel)
    tunnelMutex   sync.RWMutex
)

// StartTunnel starts a TCP listener on the VPS for either SOCKS5 or HTTP Proxy
func StartTunnel(agentID, port, tType, username, password string) error {
    tunnelMutex.Lock()
    defer tunnelMutex.Unlock()

    // 1. Check if port is physically occupied in our App memory
    if _, exists := activeTunnels[port]; exists {
        return fmt.Errorf("port %s is already active", port)
    }

    // 2. Start Listener
    l, err := net.Listen("tcp", "0.0.0.0:"+port)
    if err != nil {
        return err
    }

    // 3. Register in Memory
    activeTunnels[port] = &Tunnel{
        Port:     port,
        AgentID:  agentID,
        Type:     strings.ToLower(tType),
        Status:   "running",
        Username: username,
        Password: password,
        listener: l,
    }

    // 4. Start Handler
    go func() {
        defer l.Close()
        for {
            conn, err := l.Accept()
            if err != nil { 
                return // Listener closed
            }
            
            if strings.ToLower(tType) == "http" {
                go handleHTTPConnection(conn, agentID, username, password)
            } else {
                go handleSocksConnection(conn, agentID, username, password)
            }
        }
    }()

    // 5. Update/Create Database Record
    var dbTunnel model.Tunnel
    if err := store.DB.Where("port = ?", port).First(&dbTunnel).Error; err != nil {
        dbTunnel = model.Tunnel{
            Port:     port,
            AgentID:  agentID,
            Mode:     strings.ToUpper(tType),
            Status:   "running",
            Username: username,
            Password: password,
        }
    } else {
        dbTunnel.AgentID = agentID
        dbTunnel.Mode = strings.ToUpper(tType)
        dbTunnel.Status = "running"
        dbTunnel.Username = username
        dbTunnel.Password = password
    }
    
    if err := store.SaveTunnel(&dbTunnel); err != nil {
        l.Close()
        delete(activeTunnels, port)
        return err
    }

    log.Printf("[%s] Tunnel started on port %s for Agent %s", strings.ToUpper(tType), port, agentID)
    return nil
}

// StopTunnel stops but keeps record
func StopTunnel(port string) error {
    tunnelMutex.Lock()
    defer tunnelMutex.Unlock()

    // 1. Close the Listener (Network Layer)
    if t, exists := activeTunnels[port]; exists {
        if t.listener != nil {
            t.listener.Close()
        }
        // Remove from MEMORY map to release the "lock" on the port
        delete(activeTunnels, port)
    }

    // 2. Update Database Status (Persistence Layer)
    if err := store.UpdateTunnelStatus(port, "stopped"); err != nil {
        return err
    }

    log.Printf("[TUNNEL] Stopped tunnel on port %s", port)
    return nil
}

// DeleteTunnel stops and removes the tunnel record from DB
func DeleteTunnel(port string) error {
    tunnelMutex.Lock()
    defer tunnelMutex.Unlock()

    // 1. Stop if running
    if t, exists := activeTunnels[port]; exists {
        if t.listener != nil {
            t.listener.Close()
        }
        delete(activeTunnels, port) // Remove from memory
    }

    // 2. Remove from Database (Persistent Store)
    if err := store.DeleteTunnel(port); err != nil {
        return err
    }

    log.Printf("[TUNNEL] Deleted tunnel on port %s", port)
    return nil
}

// RestoreTunnels re-starts listeners from database on startup
func RestoreTunnels() {
    var tunnels []model.Tunnel
    store.DB.Where("status = ?", "running").Find(&tunnels)
    
    for _, t := range tunnels {
        log.Printf("[TUNNEL] Restoring %s tunnel on port %s for Agent %s", t.Mode, t.Port, t.AgentID)
        err := StartTunnel(t.AgentID, t.Port, strings.ToLower(t.Mode), t.Username, t.Password)
        if err != nil {
            log.Printf("[TUNNEL] Failed to restore tunnel on port %s: %v", t.Port, err)
            store.UpdateTunnelStatus(t.Port, "stopped")
        }
    }
}

// TunnelDTO is the enriched data transfer object for the API
type TunnelDTO struct {
    Port      string `json:"port"`
    AgentID   string `json:"agent_id"`
    Type      string `json:"type"`
    Status    string `json:"status"`
    Username  string `json:"username"`
    Password  string `json:"password"`
    AgentName string `json:"agent_name"`
    AgentIP   string `json:"agent_ip"`
}

// GetActiveTunnels returns a list of all tunnels from DB (running or stopped)
func GetActiveTunnels() []TunnelDTO {
    // 1. Fetch all records from DB
    dbTunnels, err := store.GetAllTunnels()
    if err != nil {
        log.Printf("[TUNNEL] Failed to fetch from DB: %v", err)
        return []TunnelDTO{}
    }

    list := make([]TunnelDTO, 0, len(dbTunnels))
    
    // 2. Map DB models to DTOs
    for _, t := range dbTunnels {
        // Enrichment: Lookup Agent Details
        var name, ip string
        val, exists := globals.Clients.Load(t.AgentID)
        if exists {
            client := val.(*globals.Client)
            name = client.Hostname
            ip = client.IP
        } else {
            name = "Unknown"
            ip = "Offline"
        }

        list = append(list, TunnelDTO{
            Port:      t.Port,
            AgentID:   t.AgentID,
            Type:      strings.ToLower(t.Mode),
            Status:    t.Status,
            Username:  t.Username,
            Password:  t.Password,
            AgentName: name,
            AgentIP:   ip,
        })
    }
    return list
}

func handleSocksConnection(conn net.Conn, agentID, user, pass string) {
    defer conn.Close()

    // 1. SOCKS5 Handshake
    buf := make([]byte, 258)
    if _, err := io.ReadAtLeast(conn, buf, 2); err != nil { return }
    if buf[0] != 0x05 { return }
    
    // Choose Auth Method
    if user != "" && pass != "" {
        conn.Write([]byte{0x05, 0x02}) // Username/Password Auth (0x02)

        // Auth Negotiation
        header := make([]byte, 2)
        if _, err := io.ReadAtLeast(conn, header, 2); err != nil { return }
        if header[0] != 0x01 { return } // Sub-negotiation version 1

        uLen := int(header[1])
        uBuf := make([]byte, uLen)
        if _, err := io.ReadAtLeast(conn, uBuf, uLen); err != nil { return }
        
        pLenBuf := make([]byte, 1)
        if _, err := io.ReadAtLeast(conn, pLenBuf, 1); err != nil { return }
        pLen := int(pLenBuf[0])
        pBuf := make([]byte, pLen)
        if _, err := io.ReadAtLeast(conn, pBuf, pLen); err != nil { return }

        if string(uBuf) != user || string(pBuf) != pass {
            conn.Write([]byte{0x01, 0x01}) // Auth Failed
            return
        }
        conn.Write([]byte{0x01, 0x00}) // Auth Success
    } else {
        conn.Write([]byte{0x05, 0x00}) // No Auth
    }

    // 2. Request Details
    if _, err := io.ReadAtLeast(conn, buf, 4); err != nil { return }
    if buf[1] != 0x01 { return }

    var targetHost string
    switch buf[3] {
    case 0x03: // Domain Name
        lenBuf := make([]byte, 1)
        if _, err := io.ReadFull(conn, lenBuf); err != nil { return }
        domainLen := int(lenBuf[0])
        domainBytes := make([]byte, domainLen)
        if _, err := io.ReadFull(conn, domainBytes); err != nil { return }
        targetHost = string(domainBytes)
    case 0x01: // IPv4
        ipBuf := make([]byte, 4)
        if _, err := io.ReadFull(conn, ipBuf); err != nil { return }
        targetHost = net.IP(ipBuf).String()
    default:
        return 
    }
    
    portBuf := make([]byte, 2)
    if _, err := io.ReadFull(conn, portBuf); err != nil { return }
    port := binary.BigEndian.Uint16(portBuf)

    // 3. Connect to Agent via Yamux
    val, ok := globals.Clients.Load(agentID)
    if !ok { return }
    client := val.(*globals.Client)
    session := client.YamuxSession
    if session == nil { return }

    stream, err := session.Open()
    if err != nil { return }
    defer stream.Close()

    // 4. Send Protocol Header (0x02 for SOCKS/HTTP-TCP)
    stream.Write([]byte{0x02})

	// 5. Send Target Info to Agent
	sendTargetInfo(stream, targetHost, strconv.Itoa(int(port)))

	// ⚡️ V3.0.1 Fix: Wait for Agent's Ack (1 byte) before piping
	// This prevents the Agent's internal ack byte (0x01) from leaking into the SOCKS tunnel
	ack := make([]byte, 1)
	if _, err := io.ReadFull(stream, ack); err != nil || ack[0] != 0x01 {
		conn.Write([]byte{0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0}) // 0x05 = Connection refused
		return
	}

	// 6. Respond to SOCKS Client "Success"
	conn.Write([]byte{0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0})

    // 7. Pipe Data
    go io.Copy(stream, conn)
    io.Copy(conn, stream)
}

func handleHTTPConnection(conn net.Conn, agentID, user, pass string) {
    defer conn.Close()
    
    br := bufio.NewReader(conn)
    req, err := http.ReadRequest(br)
    if err != nil { return }

    // Auth Check
    if user != "" && pass != "" {
        auth := req.Header.Get("Proxy-Authorization")
        valid := false
        if strings.HasPrefix(auth, "Basic ") {
            payload, _ := base64.StdEncoding.DecodeString(strings.TrimPrefix(auth, "Basic "))
            pair := strings.SplitN(string(payload), ":", 2)
            if len(pair) == 2 && pair[0] == user && pair[1] == pass {
                valid = true
            }
        }

        if !valid {
            resp := http.Response{
                StatusCode: 407,
                ProtoMajor: 1,
                ProtoMinor: 1,
                Header:     make(http.Header),
            }
            resp.Header.Set("Proxy-Authenticate", "Basic realm=\"Cupcake C2 Proxy\"")
            resp.Write(conn)
            return
        }
    }

    var targetHost string
    var targetPort string

    if req.Method == "CONNECT" {
        host, port, err := net.SplitHostPort(req.URL.Host)
        if err != nil {
            targetHost = req.URL.Host
            targetPort = "443"
        } else {
            targetHost = host
            targetPort = port
        }
    } else {
        targetHost = req.URL.Hostname()
        targetPort = req.URL.Port()
        if targetPort == "" { targetPort = "80" }
    }

    // 1. Connect to Agent via Yamux
    val, ok := globals.Clients.Load(agentID)
    if !ok { return }
    client := val.(*globals.Client)
    session := client.YamuxSession
    if session == nil { return }

    stream, err := session.Open()
    if err != nil { return }
    defer stream.Close()

    // 2. Send Protocol Header (0x02)
    stream.Write([]byte{0x02})

	// 3. Send Target Info
	sendTargetInfo(stream, targetHost, targetPort)

	// ⚡️ V3.0.1 Fix: Wait for Agent's Ack (1 byte)
	ack := make([]byte, 1)
	if _, err := io.ReadFull(stream, ack); err != nil || ack[0] != 0x01 {
		resp := http.Response{
			StatusCode: 502, // Bad Gateway
			ProtoMajor: 1,
			ProtoMinor: 1,
		}
		resp.Write(conn)
		return
	}

    // 4. Handle Protocol Specifics
    if req.Method == "CONNECT" {
        conn.Write([]byte("HTTP/1.1 200 Connection Established\r\n\r\n"))
        go io.Copy(stream, br)
        io.Copy(conn, stream)
    } else {
        req.Write(stream)
        go io.Copy(stream, br)
        io.Copy(conn, stream)
    }
}

func sendTargetInfo(w io.Writer, host string, portStr string) {
    portInt, _ := strconv.Atoi(portStr)
    hostBytes := []byte(host)
    header := make([]byte, 1+len(hostBytes)+2)
    header[0] = uint8(len(hostBytes))
    copy(header[1:], hostBytes)
    binary.BigEndian.PutUint16(header[1+len(hostBytes):], uint16(portInt))
    w.Write(header)
}
