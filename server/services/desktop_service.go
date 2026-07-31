package services

import (
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"sync"
	"time"

	"cupcake-server/pkg/utils"
)

// DesktopSession tracks one active desktop stream per agent (MVP: single viewer).
type DesktopSession struct {
	AgentID   string
	Stream    net.Conn
	Bucket    *utils.TokenBucket
	OpenedAt  time.Time
	MaxFps    int
	MaxFrameB int
}

var (
	desktopMu   sync.Mutex
	desktopByID = map[string]*DesktopSession{}
)

// DesktopBusyError means another viewer already holds the session.
var DesktopBusyError = fmt.Errorf("desktop busy")

// TryReserveDesktop registers agent as busy **before** opening Yamux.
// Returns release func to call if Open fails, or error if already busy.
func TryReserveDesktop(agentID string) (release func(), err error) {
	desktopMu.Lock()
	defer desktopMu.Unlock()
	if _, ok := desktopByID[agentID]; ok {
		return nil, DesktopBusyError
	}
	// placeholder until stream attached
	desktopByID[agentID] = &DesktopSession{
		AgentID:   agentID,
		OpenedAt:  time.Now(),
		Bucket:    utils.NewTokenBucket(utils.DesktopDefaultMaxBps, time.Now().UnixMilli()),
		MaxFps:    utils.DesktopDefaultMaxFps,
		MaxFrameB: utils.DesktopDefaultMaxFrameBytes,
	}
	return func() {
		desktopMu.Lock()
		defer desktopMu.Unlock()
		if s, ok := desktopByID[agentID]; ok && s.Stream == nil {
			delete(desktopByID, agentID)
		}
	}, nil
}

// AttachDesktopStream binds the open Yamux stream to the reserved session.
func AttachDesktopStream(agentID string, stream net.Conn) error {
	desktopMu.Lock()
	defer desktopMu.Unlock()
	s, ok := desktopByID[agentID]
	if !ok {
		return fmt.Errorf("desktop not reserved")
	}
	s.Stream = stream
	return nil
}

// ReleaseDesktop removes session (STOP / EOF / error). Idempotent.
func ReleaseDesktop(agentID string) {
	desktopMu.Lock()
	defer desktopMu.Unlock()
	if s, ok := desktopByID[agentID]; ok {
		if s.Stream != nil {
			_ = s.Stream.Close()
			s.Stream = nil
		}
		delete(desktopByID, agentID)
	}
}

// HasDesktopSession reports whether agent already has a desktop viewer.
func HasDesktopSession(agentID string) bool {
	desktopMu.Lock()
	defer desktopMu.Unlock()
	_, ok := desktopByID[agentID]
	return ok
}

// OpenDesktopToAgent opens Yamux DESKTOP stream after busy check.
// On success caller owns reading/writing until ReleaseDesktop.
func OpenDesktopToAgent(agentID string) (net.Conn, error) {
	release, err := TryReserveDesktop(agentID)
	if err != nil {
		return nil, err
	}

	session, ok := GetAgentSession(agentID)
	if !ok {
		release()
		return nil, fmt.Errorf("no yamux session")
	}
	stream, err := session.Open()
	if err != nil {
		release()
		return nil, err
	}
	if _, err := stream.Write([]byte{utils.YamuxStreamDesktop}); err != nil {
		_ = stream.Close()
		release()
		return nil, err
	}
	if err := AttachDesktopStream(agentID, stream); err != nil {
		_ = stream.Close()
		release()
		return nil, err
	}
	return stream, nil
}

// DesktopAllowFrame applies per-stream size/bps limits.
func DesktopAllowFrame(agentID string, frameBytes int) bool {
	desktopMu.Lock()
	s := desktopByID[agentID]
	desktopMu.Unlock()
	if s == nil {
		return false
	}
	if frameBytes > s.MaxFrameB {
		return false
	}
	return s.Bucket.Allow(int64(frameBytes), time.Now().UnixMilli())
}

// WriteDesktopHello sends HELLO JSON to agent.
func WriteDesktopHello(w io.Writer, fps, quality, maxW int, encode string, maxBps int) error {
	if encode == "" {
		encode = "jpeg"
	}
	body, _ := json.Marshal(map[string]interface{}{
		"fps":      fps,
		"quality":  quality,
		"max_w":    maxW,
		"encode":   encode,
		"monitor":  0,
		"max_bps":  maxBps,
	})
	msg, err := utils.EncodeDesktopMessage(utils.DesktopMsgHello, 0, body)
	if err != nil {
		return err
	}
	_, err = w.Write(msg)
	return err
}

// WriteDesktopStop sends STOP frame.
func WriteDesktopStop(w io.Writer) error {
	msg, err := utils.EncodeDesktopMessage(utils.DesktopMsgStop, 0, []byte("{}"))
	if err != nil {
		return err
	}
	_, err = w.Write(msg)
	return err
}

// PumpDesktopAgentToAdmin copies agent frames to admin writer with rate limit.
// Stops on error/EOF; always ReleaseDesktop.
func PumpDesktopAgentToAdmin(agentID string, agent net.Conn, admin io.Writer) {
	defer ReleaseDesktop(agentID)
	hdr := make([]byte, utils.DesktopHeaderLen)
	for {
		if _, err := io.ReadFull(agent, hdr); err != nil {
			return
		}
		env, err := utils.ParseDesktopHeader(hdr)
		if err != nil {
			if err == utils.ErrDesktopSilentClose {
				log.Printf("[desktop] agent %s silent close parse", agentID)
			}
			return
		}
		payload := make([]byte, env.PayloadLen)
		if env.PayloadLen > 0 {
			if _, err := io.ReadFull(agent, payload); err != nil {
				return
			}
		}
		frameLen := utils.DesktopHeaderLen + int(env.PayloadLen)
		if env.MsgType == utils.DesktopMsgFrame && !DesktopAllowFrame(agentID, frameLen) {
			// drop frame under rate limit
			continue
		}
		// re-encode same envelope to admin
		msg := append(append([]byte{}, hdr...), payload...)
		if _, err := admin.Write(msg); err != nil {
			return
		}
	}
}
