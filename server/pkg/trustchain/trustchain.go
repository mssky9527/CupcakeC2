// Package trustchain provides HMAC-SHA256 package signing, verification, and
// anti-rollback version tracking for Cupcake plugins and L2 modules.
//
// Fail-closed: missing keys, empty signatures, and wrong signatures all reject.
package trustchain

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"strconv"
	"strings"
	"sync"
)

// PackageMeta is the signed metadata for a plugin or module package.
type PackageMeta struct {
	ModuleID   string // or plugin id
	Version    string // semver-like "1.2.3" or integer string "3"
	SHA256     string // lowercase hex of payload
	Target     string // optional e.g. windows-x64
	ABIVersion int
	Signer     string // key id e.g. "test-key-1"
	Signature  string // hex HMAC-SHA256 over canonical payload
}

// CanonicalPayload returns the stable sign input:
// module_id|version|sha256|target|abi_version|signer
func CanonicalPayload(meta PackageMeta) string {
	sha := strings.ToLower(strings.TrimSpace(meta.SHA256))
	return fmt.Sprintf("%s|%s|%s|%s|%d|%s",
		meta.ModuleID,
		meta.Version,
		sha,
		meta.Target,
		meta.ABIVersion,
		meta.Signer,
	)
}

// HMACKeyForSigner resolves the HMAC key for a signer id.
//
// Priority:
//  1. CUPCAKE_TRUST_HMAC_KEY env (raw key bytes as UTF-8 string)
//  2. Built-in test key when CUPCAKE_TRUST_DEV_KEYS=1 (domain-separated by signer)
//  3. nil → callers / Verify fail closed
func HMACKeyForSigner(signer string) []byte {
	if k := strings.TrimSpace(os.Getenv("CUPCAKE_TRUST_HMAC_KEY")); k != "" {
		return []byte(k)
	}
	if os.Getenv("CUPCAKE_TRUST_DEV_KEYS") == "1" {
		// Fixed, deterministic test-only material (never use in production).
		seed := "cupcake-trust-dev-key-v1|" + signer
		sum := sha256.Sum256([]byte(seed))
		return sum[:]
	}
	return nil
}

// Sign returns lowercase hex HMAC-SHA256 over CanonicalPayload(meta).
func Sign(meta PackageMeta, key []byte) (signatureHex string, err error) {
	if len(key) == 0 {
		return "", fmt.Errorf("trust key missing: cannot sign")
	}
	mac := hmac.New(sha256.New, key)
	_, _ = mac.Write([]byte(CanonicalPayload(meta)))
	return hex.EncodeToString(mac.Sum(nil)), nil
}

// Verify checks signature with constant-time compare. Fail-closed on empty key,
// empty signature, decode errors, and wrong MAC.
func Verify(meta PackageMeta, key []byte) error {
	if len(key) == 0 {
		return fmt.Errorf("trust key missing: refuse verify (configure CUPCAKE_TRUST_HMAC_KEY or CUPCAKE_TRUST_DEV_KEYS=1 for lab)")
	}
	sigHex := strings.TrimSpace(meta.Signature)
	if sigHex == "" {
		return fmt.Errorf("empty signature")
	}
	want, err := hex.DecodeString(strings.ToLower(sigHex))
	if err != nil {
		return fmt.Errorf("invalid signature hex: %w", err)
	}
	mac := hmac.New(sha256.New, key)
	_, _ = mac.Write([]byte(CanonicalPayload(meta)))
	got := mac.Sum(nil)
	if !hmac.Equal(want, got) {
		return fmt.Errorf("signature mismatch")
	}
	return nil
}

// RollbackGuard tracks the highest published version per package id.
// Equal version is allowed (re-publish); lower is refused.
type RollbackGuard struct {
	mu  sync.Mutex
	max map[string]string // id -> highest version committed
}

// NewRollbackGuard returns an empty in-memory anti-rollback store.
func NewRollbackGuard() *RollbackGuard {
	return &RollbackGuard{max: make(map[string]string)}
}

// CheckAndCommit refuses versions lower than the recorded max for id.
// On success (equal or higher / first publish), records the max and returns nil.
func (g *RollbackGuard) CheckAndCommit(id, version string) error {
	if g == nil {
		return fmt.Errorf("nil rollback guard")
	}
	id = strings.TrimSpace(id)
	version = strings.TrimSpace(version)
	if id == "" {
		return fmt.Errorf("empty package id")
	}
	if version == "" {
		return fmt.Errorf("empty version")
	}

	g.mu.Lock()
	defer g.mu.Unlock()
	if g.max == nil {
		g.max = make(map[string]string)
	}
	prev, ok := g.max[id]
	if ok {
		cmp := CompareVersion(version, prev)
		if cmp < 0 {
			return fmt.Errorf("version rollback refused: %s has published %s, got %s", id, prev, version)
		}
		if cmp == 0 {
			// re-publish same version — allowed, max unchanged
			return nil
		}
	}
	g.max[id] = version
	return nil
}

// MaxVersion returns the recorded max for id (empty if none).
func (g *RollbackGuard) MaxVersion(id string) string {
	if g == nil {
		return ""
	}
	g.mu.Lock()
	defer g.mu.Unlock()
	return g.max[id]
}

// Reset clears all recorded versions (tests).
func (g *RollbackGuard) Reset() {
	if g == nil {
		return
	}
	g.mu.Lock()
	defer g.mu.Unlock()
	g.max = make(map[string]string)
}

// CompareVersion compares semver-like or integer strings.
// Missing parts are 0. Returns -1 if a < b, 0 if equal, 1 if a > b.
func CompareVersion(a, b string) int {
	pa := parseVersionParts(a)
	pb := parseVersionParts(b)
	for i := 0; i < 3; i++ {
		if pa[i] < pb[i] {
			return -1
		}
		if pa[i] > pb[i] {
			return 1
		}
	}
	return 0
}

func parseVersionParts(v string) [3]int {
	v = strings.TrimSpace(v)
	// strip optional leading "v"
	v = strings.TrimPrefix(v, "v")
	v = strings.TrimPrefix(v, "V")
	var out [3]int
	if v == "" {
		return out
	}
	parts := strings.Split(v, ".")
	for i := 0; i < 3 && i < len(parts); i++ {
		p := strings.TrimSpace(parts[i])
		// take leading digits only (tolerate "1.2.3-beta" → 1.2.3)
		num := ""
		for _, c := range p {
			if c >= '0' && c <= '9' {
				num += string(c)
			} else {
				break
			}
		}
		if num == "" {
			continue
		}
		n, err := strconv.Atoi(num)
		if err == nil {
			out[i] = n
		}
	}
	return out
}
