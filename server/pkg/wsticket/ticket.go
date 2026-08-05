// Package wsticket issues short-lived, single-use WebSocket upgrade tickets.
// Panel interactive WS paths (pty, shell, build logs) must not rely on the
// durable session bearer in the query string; clients mint a ticket under
// session auth and redeem it once via ?ticket=.
package wsticket

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"strconv"
	"strings"
	"sync"
	"time"
)

const (
	// DefaultTTL is used when Mint is called with a non-positive ttl.
	DefaultTTL = 60 * time.Second
	// MaxTTL caps ticket lifetime even if a longer ttl is requested.
	MaxTTL = 300 * time.Second
	// rawTicketBytes is the entropy used for the opaque ticket string.
	rawTicketBytes = 32
)

// Known purposes accepted by Mint / Redeem.
const (
	PurposePTY       = "pty"
	PurposeShell     = "shell"
	PurposeBuildLogs = "build_logs"
)

var (
	// ErrInvalid is returned for missing or unknown tickets.
	ErrInvalid = errors.New("invalid ticket")
	// ErrExpired is returned when the ticket TTL has elapsed.
	ErrExpired = errors.New("ticket expired")
	// ErrPurpose is returned when the redeem purpose does not match.
	ErrPurpose = errors.New("wrong purpose")
	// ErrPurposeUnknown is returned when Mint receives an unsupported purpose.
	ErrPurposeUnknown = errors.New("unknown purpose")
)

type entry struct {
	userID    uint
	username  string
	role      string
	purpose   string
	expiresAt time.Time
}

var (
	mu      sync.Mutex
	tickets = make(map[string]*entry) // key = SHA-256 hex of raw ticket
)

// ValidPurpose reports whether purpose is a known WS upgrade purpose.
func ValidPurpose(purpose string) bool {
	switch strings.ToLower(strings.TrimSpace(purpose)) {
	case PurposePTY, PurposeShell, PurposeBuildLogs:
		return true
	default:
		return false
	}
}

func hashTicket(raw string) string {
	sum := sha256.Sum256([]byte(raw))
	return hex.EncodeToString(sum[:])
}

func purgeExpiredLocked(now time.Time) {
	for h, e := range tickets {
		if now.After(e.expiresAt) {
			delete(tickets, h)
		}
	}
}

// Mint creates a short-lived upgrade ticket. Only the SHA-256 hash is retained
// in memory; the raw ticket is returned once to the caller.
// ttl <= 0 uses DefaultTTL; ttl above MaxTTL is clamped.
func Mint(userID uint, username, role, purpose string, ttl time.Duration) (rawTicket string, err error) {
	purpose = strings.ToLower(strings.TrimSpace(purpose))
	if !ValidPurpose(purpose) {
		return "", ErrPurposeUnknown
	}
	username = strings.TrimSpace(username)
	role = strings.TrimSpace(role)
	if username == "" {
		return "", errors.New("username required")
	}
	if ttl <= 0 {
		ttl = DefaultTTL
	}
	if ttl > MaxTTL {
		ttl = MaxTTL
	}

	buf := make([]byte, rawTicketBytes)
	if _, err := rand.Read(buf); err != nil {
		return "", err
	}
	raw := base64.RawURLEncoding.EncodeToString(buf)
	h := hashTicket(raw)
	now := time.Now()

	mu.Lock()
	defer mu.Unlock()
	// Opportunistic cleanup to keep the map bounded under load.
	if len(tickets) > 64 {
		purgeExpiredLocked(now)
	}
	tickets[h] = &entry{
		userID:    userID,
		username:  username,
		role:      role,
		purpose:   purpose,
		expiresAt: now.Add(ttl),
	}
	return raw, nil
}

// Redeem consumes a raw ticket once. On success the ticket is deleted so reuse fails.
// Wrong purpose, missing, already-used, and expired tickets all fail closed.
func Redeem(rawTicket, purpose string) (userID, username, role string, err error) {
	rawTicket = strings.TrimSpace(rawTicket)
	purpose = strings.ToLower(strings.TrimSpace(purpose))
	if rawTicket == "" {
		return "", "", "", ErrInvalid
	}
	if !ValidPurpose(purpose) {
		return "", "", "", ErrPurpose
	}

	h := hashTicket(rawTicket)
	now := time.Now()

	mu.Lock()
	defer mu.Unlock()
	e, ok := tickets[h]
	if !ok {
		return "", "", "", ErrInvalid
	}
	if now.After(e.expiresAt) {
		// Drop expired entries so they cannot be retried.
		delete(tickets, h)
		return "", "", "", ErrExpired
	}
	if e.purpose != purpose {
		// Leave ticket in place until correct purpose or expiry (delete only on success).
		return "", "", "", ErrPurpose
	}
	// One-time: delete on success so reuse fails.
	delete(tickets, h)
	return strconv.FormatUint(uint64(e.userID), 10), e.username, e.role, nil
}

// ResetForTest clears the in-memory ticket map (unit tests only).
func ResetForTest() {
	mu.Lock()
	defer mu.Unlock()
	tickets = make(map[string]*entry)
}

// CountForTest returns the number of outstanding tickets (unit tests only).
func CountForTest() int {
	mu.Lock()
	defer mu.Unlock()
	return len(tickets)
}
