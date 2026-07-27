package services

import (
	"context"
	"crypto/tls"
	"cupcake-server/pkg/globals"
	"cupcake-server/pkg/store"
	"cupcake-server/pkg/utils"
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
			EnableTLS:         l.EnableTLS,
			TLSCertPath:       l.TLSCertPath,
			TLSKeyPath:        l.TLSKeyPath,
			TLSCertPEM:        l.TLSCertPEM,
			TLSKeyPEM:         l.TLSKeyPEM,
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

		// 🔒 TLS Configuration for Secure WebSocket (wss://)
		if ln.EnableTLS {
			if err := configureTLS(ln); err != nil {
				log.Printf("[TLS] Failed to configure TLS for listener %s: %v", ln.ID, err)
				return err
			}
			log.Printf("[TLS] Secure WebSocket (wss://) enabled on port %d", ln.Port)
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
			if ln.EnableTLS && ln.HTTPServer.TLSConfig != nil {
				// Start TLS-enabled WebSocket server
				err = ln.HTTPServer.ListenAndServeTLS("", "") // Cert/key already loaded in TLSConfig
			} else {
				// Start plain WebSocket server
				err = ln.HTTPServer.ListenAndServe()
			}
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

// 🔒 Configure TLS for Secure WebSocket
func configureTLS(ln *globals.Listener) error {
	// Priority: Inline PEM > File paths
	var cert tls.Certificate
	var err error

	if ln.TLSCertPEM != "" && ln.TLSKeyPEM != "" {
		// Use inline PEM certificates
		cert, err = tls.X509KeyPair([]byte(ln.TLSCertPEM), []byte(ln.TLSKeyPEM))
		if err != nil {
			return fmt.Errorf("failed to parse inline PEM: %v", err)
		}
		log.Printf("[TLS] Using inline PEM certificate for listener %s", ln.ID)
	} else if ln.TLSCertPath != "" && ln.TLSKeyPath != "" {
		// Load from file paths
		cert, err = tls.LoadX509KeyPair(ln.TLSCertPath, ln.TLSKeyPath)
		if err != nil {
			return fmt.Errorf("failed to load cert/key files: %v", err)
		}
		log.Printf("[TLS] Loaded certificate from %s for listener %s", ln.TLSCertPath, ln.ID)
	} else {
		// Generate self-signed certificate for testing/internal use
		cert, err = generateSelfSignedCert()
		if err != nil {
			return fmt.Errorf("failed to generate self-signed cert: %v", err)
		}
		log.Printf("[TLS] Using auto-generated self-signed certificate for listener %s", ln.ID)
	}

	ln.HTTPServer.TLSConfig = &tls.Config{
		Certificates: []tls.Certificate{cert},
		MinVersion:   tls.VersionTLS12, // Enforce TLS 1.2+ for security
	}

	return nil
}

// Generate a self-signed certificate for development/testing
func generateSelfSignedCert() (tls.Certificate, error) {
	return utils.GenerateSelfSignedCert([]string{"localhost", "127.0.0.1", "0.0.0.0"})
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

// HandleDNSQuery is defined in dns_tunnel.go (TXT cmd:/alive/ok protocol).
