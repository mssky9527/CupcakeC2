package utils

import (
	"bytes"
	"testing"

	"golang.org/x/crypto/curve25519"
)

func TestX25519HandshakeSymmetric(t *testing.T) {
	psk := []byte("test-psk-material-32-bytes-long!!")

	// Client side
	client, err := GenerateEphemeralKey()
	if err != nil {
		t.Fatal(err)
	}
	clientMsg := make([]byte, NoiseMsgLen)
	clientMsg[0] = NoiseVersion
	copy(clientMsg[1:], client.Public[:])

	// Server
	serverResp, serverSK, err := NoiseRespond(clientMsg, psk)
	if err != nil {
		t.Fatal(err)
	}
	if len(serverResp) != NoiseMsgLen || serverResp[0] != NoiseVersion {
		t.Fatalf("bad server resp")
	}

	// Client completes: ECDH(client_secret, server_public) + HKDF
	var serverPub [32]byte
	copy(serverPub[:], serverResp[1:33])
	var shared [32]byte
	curve25519.ScalarMult(&shared, &client.Secret, &serverPub)
	clientSK, err := deriveSessionKeyHKDF(shared[:], psk)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(clientSK[:], serverSK[:]) {
		t.Fatalf("session keys diverge\nclient=%x\nserver=%x", clientSK, serverSK)
	}
}

func TestNoiseRespondRejectsLegacy32(t *testing.T) {
	_, _, err := NoiseRespond(make([]byte, 32), []byte("psk"))
	if err == nil {
		t.Fatal("expected reject legacy 32-byte fake-noise")
	}
}
