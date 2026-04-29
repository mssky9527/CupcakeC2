package services

import (
	"context"
	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/store"
	"fmt"
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/miekg/dns"
)

func RestoreListeners() {
	time.Sleep(1 * time.Second) // Wait for DB init
	listeners, err := store.GetAllListeners()
	if err != nil {
		log.Printf("Failed to restore listeners: %v", err)
		return
	}

	for _, l := range listeners {
		newLn := &globals.Listener{
			ID:                l.ID,
			BindIP:            l.BindIP,
			Port:              l.Port,
			Protocol:          l.Protocol,
			Note:              l.Note,
			EncryptMode:       l.EncryptMode,
			EncryptKey:        l.EncryptKey,
			EncryptionSalt:    l.EncryptionSalt,
			ObfuscateMode:     l.ObfuscateMode,
			CustomPath:        l.CustomPath,
			NSDomain:          l.NSDomain,
			PublicDNS:         l.PublicDNS,
			HeartbeatInterval: l.HeartbeatInterval,
			HeartbeatJitter:   l.HeartbeatJitter,
			MaxRetry:          l.MaxRetry,
			Status:            l.Status,
		}

		if newLn.Status == "Running" {
			if err := StartListenerInstance(newLn); err != nil {
				log.Printf("Failed to restart listener %s: %v", newLn.ID, err)
				newLn.Status = "Failed"
			}
		}

		globals.Listeners.Store(newLn.ID, newLn)
	}
}

func StartListenerInstance(ln *globals.Listener) error {
	if ln.Protocol == "WebSocket" {
		mux := http.NewServeMux()
		path := ln.CustomPath
		if path == "" || !strings.HasPrefix(path, "/") {
			path = "/ws"
		}
		mux.HandleFunc(path, func(w http.ResponseWriter, r *http.Request) {
			conn, err := globals.Upgrader.Upgrade(w, r, nil)
			if err != nil { return }
			go ProcessWebSocket(conn, r.RemoteAddr, ln)
		})
		ln.HTTPServer = &http.Server{
			Addr:    fmt.Sprintf("%s:%d", ln.BindIP, ln.Port),
			Handler: mux,
		}
	} else if ln.Protocol == "DNS" {
		ln.DNSServer = &dns.Server{
			Addr:    fmt.Sprintf("%s:%d", ln.BindIP, ln.Port),
			Net:     "udp",
			Handler: dns.HandlerFunc(HandleDNSQuery),
		}
	}

	go func() {
		var err error
		if ln.Protocol == "WebSocket" {
			err = ln.HTTPServer.ListenAndServe()
		} else if ln.Protocol == "DNS" {
			err = ln.DNSServer.(*dns.Server).ListenAndServe()
		} else if ln.Protocol == "TCP" {
			StartTCPListener(ln)
			return
		} else if ln.Protocol == "Bind-TCP" || ln.Protocol == "正向TCP" {
			ln.Status = "Running"
			return
		}

		if err != nil && err != http.ErrServerClosed {
			log.Printf("Listener on port %d failed: %v", ln.Port, err)
			ln.Status = "Failed"
		}
	}()
	return nil
}

func StopListenerInstance(ln *globals.Listener) {
	if ln.Protocol == "WebSocket" && ln.HTTPServer != nil {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = ln.HTTPServer.Shutdown(ctx)
	}
	if ln.Protocol == "DNS" && ln.DNSServer != nil {
		if srv, ok := ln.DNSServer.(*dns.Server); ok && srv != nil {
			_ = srv.Shutdown()
		}
	}
	if ln.Protocol == "TCP" && ln.TCPServer != nil {
		ln.TCPServer.Close()
	}
	ln.Status = "Stopped"
}

func HandleDNSQuery(w dns.ResponseWriter, r *dns.Msg) {
	m := new(dns.Msg)
	m.SetReply(r)
	m.Compress = false

	switch r.Opcode {
	case dns.OpcodeQuery:
		for _, q := range m.Question {
			if q.Qtype == dns.TypeTXT {
				name := strings.ToLower(q.Name)
				if strings.HasPrefix(name, "ping.") {
					txt := "alive"
					rr, _ := dns.NewRR(fmt.Sprintf("%s 3600 IN TXT \"%s\"", q.Name, txt))
					m.Answer = append(m.Answer, rr)
				}
			}
		}
	}
	w.WriteMsg(m)
}
