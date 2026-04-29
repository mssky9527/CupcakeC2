package utils

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"errors"
	"io"
	"strings"
)

// EncryptAES encrypts data using AES-256-GCM.
// Returns [Nonce (12 bytes) + Ciphertext]
func EncryptAES(plaintext []byte, key []byte) ([]byte, error) {
	// Ensure key is 32 bytes for AES-256
	fixedKey := make([]byte, 32)
	copy(fixedKey, key)

	block, err := aes.NewCipher(fixedKey)
	if err != nil {
		return nil, err
	}

	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}

	nonce := make([]byte, gcm.NonceSize())
	if _, err := io.ReadFull(rand.Reader, nonce); err != nil {
		return nil, err
	}

	ciphertext := gcm.Seal(nil, nonce, plaintext, nil)
	// Return Nonce + Ciphertext
	return append(nonce, ciphertext...), nil
}

// DecryptAES decrypts data encrypted with AES-256-GCM.
// Expects [Nonce (12 bytes) + Ciphertext]
func DecryptAES(data []byte, key []byte) ([]byte, error) {
	if len(data) < 12 {
		return nil, errors.New("ciphertext too short")
	}

	fixedKey := make([]byte, 32)
	copy(fixedKey, key)

	block, err := aes.NewCipher(fixedKey)
	if err != nil {
		return nil, err
	}

	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}

	nonceSize := gcm.NonceSize()
	if len(data) < nonceSize {
		return nil, errors.New("invalid data size")
	}

	nonce, ciphertext := data[:nonceSize], data[nonceSize:]
	return gcm.Open(nil, nonce, ciphertext, nil)
}

// DeriveKey implements SHA256(BaseKey + Salt) to derive a unique session key
func DeriveKey(baseKey []byte, salt []byte) []byte {
	if len(salt) == 0 {
		return baseKey
	}
	// Standardize key to 32 bytes for SHA256 consistency if needed
	// But usually SHA256 takes any length input.
	hasher := sha256.New()
	hasher.Write(baseKey)
	hasher.Write(salt)
	return hasher.Sum(nil)
}

// ObfuscatePacket applies secondary obfuscation to encrypted data
func ObfuscatePacket(data []byte, mode string, key []byte) []byte {
	mode = strings.ToLower(strings.TrimSpace(mode))
	switch mode {
	case "base64":
		return []byte(base64.StdEncoding.EncodeToString(data))
	case "xor":
		if len(key) == 0 { return data }
		out := make([]byte, len(data))
		for i := 0; i < len(data); i++ {
			out[i] = data[i] ^ key[i%len(key)]
		}
		return out
	case "junk":
		originalLen := uint32(len(data))
		// Random junk 8-64 bytes
		junkLen := randInt(8, 64)
		junk := make([]byte, junkLen)
		_, _ = rand.Read(junk)
		
		out := make([]byte, len(data)+junkLen+4)
		copy(out, data)
		copy(out[len(data):], junk)
		// Put original length at the very end (4 bytes BE)
		binary.BigEndian.PutUint32(out[len(out)-4:], originalLen)
		return out
	default:
		return data
	}
}

// DeobfuscatePacket reverses the obfuscation
func DeobfuscatePacket(data []byte, mode string, key []byte) []byte {
	mode = strings.ToLower(strings.TrimSpace(mode))
	switch mode {
	case "base64":
		decoded, err := base64.StdEncoding.DecodeString(string(data))
		if err != nil { return data }
		return decoded
	case "xor":
		if len(key) == 0 { return data }
		out := make([]byte, len(data))
		for i := 0; i < len(data); i++ {
			out[i] = data[i] ^ key[i%len(key)]
		}
		return out
	case "junk":
		if len(data) < 4 { return data }
		originalLen := binary.BigEndian.Uint32(data[len(data)-4:])
		if int(originalLen) <= len(data)-4 {
			return data[:originalLen]
		}
		return data
	default:
		return data
	}
}

func randInt(min, max int) int {
	b := make([]byte, 1)
	_, _ = rand.Read(b)
	return int(b[0])%(max-min) + min
}
