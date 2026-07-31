package utils

import (
	"bytes"
	"testing"
)

func TestX25519HandshakeSymmetricWithPSK(t *testing.T) {
	psk := []byte("test-psk-material-32-bytes-long!!")

	client, clientMsg, err := NoiseInitiate(psk)
	if err != nil {
		t.Fatal(err)
	}
	if len(clientMsg) != NoiseMsgLen || clientMsg[0] != NoiseVersion {
		t.Fatalf("bad client msg len=%d ver=%x", len(clientMsg), clientMsg[0])
	}

	serverResp, serverSK, err := NoiseRespond(clientMsg, psk)
	if err != nil {
		t.Fatal(err)
	}
	if len(serverResp) != NoiseMsgLen || serverResp[0] != NoiseVersion {
		t.Fatalf("bad server resp")
	}

	clientSK, err := NoiseComplete(client, serverResp, psk)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(clientSK[:], serverSK[:]) {
		t.Fatalf("session keys diverge\nclient=%x\nserver=%x", clientSK, serverSK)
	}

	// Traffic encrypt/decrypt with session key
	ct, err := NoiseEncrypt(serverSK, []byte("hello-noise"))
	if err != nil {
		t.Fatal(err)
	}
	pt, err := NoiseDecrypt(clientSK, ct)
	if err != nil {
		t.Fatal(err)
	}
	if string(pt) != "hello-noise" {
		t.Fatalf("decrypt got %q", pt)
	}
}

func TestNoiseWrongPSKFails(t *testing.T) {
	pskA := []byte("correct-psk-material-32bytes!!!!!")
	pskB := []byte("wrong-psk-material-xxxxxxxxxxxx!!")

	_, clientMsg, err := NoiseInitiate(pskA)
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err := NoiseRespond(clientMsg, pskB); err == nil {
		t.Fatal("wrong PSK must fail respond")
	}

	client, clientMsg, err := NoiseInitiate(pskA)
	if err != nil {
		t.Fatal(err)
	}
	serverResp, _, err := NoiseRespond(clientMsg, pskA)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := NoiseComplete(client, serverResp, pskB); err == nil {
		t.Fatal("wrong PSK must fail complete")
	}
}

func TestNoiseRespondRejectsLegacy32And33(t *testing.T) {
	psk := []byte("psk")
	if _, _, err := NoiseRespond(make([]byte, 32), psk); err == nil {
		t.Fatal("expected reject legacy 32-byte")
	}
	if _, _, err := NoiseRespond(make([]byte, 33), psk); err == nil {
		t.Fatal("expected reject v1 33-byte without mac")
	}
	if _, _, err := NoiseRespond(make([]byte, NoiseMsgLen), nil); err == nil {
		t.Fatal("empty psk must fail")
	}
}
