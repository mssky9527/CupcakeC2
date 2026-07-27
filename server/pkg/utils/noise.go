package utils

import (
	"crypto/rand"
	"crypto/sha256"
	"fmt"
	"io"

	"golang.org/x/crypto/curve25519"
	"golang.org/x/crypto/hkdf"
)

// =============================================================================
// Real X25519 ECDH handshake (hard cutover — not the old SHA256-fake "Noise")
// Wire: version(1)=0x01 || public_key(32)  → 33 bytes each direction
// Session key: HKDF-SHA256(ikm=shared_secret, salt=psk, info=WireIDs.NoiseInfo)
// =============================================================================

const (
	NoiseVersion byte = 0x01
	NoiseMsgLen       = 33 // version + 32-byte X25519 public
)

// NoiseInfoBytes returns build-seed derived HKDF info (not a product string).
func NoiseInfoBytes() []byte {
	return GetWireIDs().NoiseInfo
}

// EphemeralKey is an X25519 key pair.
type EphemeralKey struct {
	Secret [32]byte
	Public [32]byte
}

// GenerateEphemeralKey creates a random X25519 key pair.
func GenerateEphemeralKey() (*EphemeralKey, error) {
	var secret [32]byte
	if _, err := rand.Read(secret[:]); err != nil {
		return nil, fmt.Errorf("rand.Read failed: %w", err)
	}
	// Clamp for X25519
	secret[0] &= 248
	secret[31] &= 127
	secret[31] |= 64

	var public [32]byte
	curve25519.ScalarBaseMult(&public, &secret)
	return &EphemeralKey{Secret: secret, Public: public}, nil
}

// ecdhShared computes X25519(local_secret, peer_public).
func ecdhShared(localSecret, peerPublic *[32]byte) ([32]byte, error) {
	var shared [32]byte
	curve25519.ScalarMult(&shared, localSecret, peerPublic)
	// Reject all-zero shared secret (low-order points)
	var zero [32]byte
	if shared == zero {
		return zero, fmt.Errorf("invalid ECDH shared secret (zero)")
	}
	return shared, nil
}

// deriveSessionKeyHKDF derives 32-byte AES key from ECDH shared + PSK.
func deriveSessionKeyHKDF(sharedSecret, psk []byte) ([32]byte, error) {
	r := hkdf.New(sha256.New, sharedSecret, psk, NoiseInfoBytes())
	var sk [32]byte
	if _, err := io.ReadFull(r, sk[:]); err != nil {
		return [32]byte{}, err
	}
	return sk, nil
}

// NoiseRespond processes client handshake (33-byte v1 or legacy reject).
// Returns (serverResponse 33 bytes, sessionKey, error).
func NoiseRespond(clientMsg []byte, psk []byte) ([]byte, [32]byte, error) {
	if len(clientMsg) != NoiseMsgLen {
		return nil, [32]byte{}, fmt.Errorf("invalid handshake length: %d (want %d X25519)", len(clientMsg), NoiseMsgLen)
	}
	if clientMsg[0] != NoiseVersion {
		return nil, [32]byte{}, fmt.Errorf("unsupported noise version: 0x%02x", clientMsg[0])
	}

	e, err := GenerateEphemeralKey()
	if err != nil {
		return nil, [32]byte{}, err
	}

	var clientPublic [32]byte
	copy(clientPublic[:], clientMsg[1:33])

	shared, err := ecdhShared(&e.Secret, &clientPublic)
	if err != nil {
		return nil, [32]byte{}, err
	}
	sessionKey, err := deriveSessionKeyHKDF(shared[:], psk)
	if err != nil {
		return nil, [32]byte{}, err
	}

	resp := make([]byte, NoiseMsgLen)
	resp[0] = NoiseVersion
	copy(resp[1:], e.Public[:])
	return resp, sessionKey, nil
}

// NoiseEncrypt encrypts plaintext with the session key (AES-256-GCM wrapper).
func NoiseEncrypt(sessionKey [32]byte, plaintext []byte) ([]byte, error) {
	return EncryptAES(plaintext, sessionKey[:])
}

// NoiseDecrypt decrypts ciphertext with the session key (AES-256-GCM wrapper).
func NoiseDecrypt(sessionKey [32]byte, ciphertext []byte) ([]byte, error) {
	return DecryptAES(ciphertext, sessionKey[:])
}
