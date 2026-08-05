package utils

import (
	"bytes"
	"encoding/binary"
	"io"
	"strings"
	"testing"
)

func TestEncodeDecodeFileRequestHeader(t *testing.T) {
	path := `C:\Users\test\payload.exe`
	enc, err := EncodeFileRequestHeader(FileOpPut, path)
	if err != nil {
		t.Fatal(err)
	}
	if enc[0] != FileOpPut {
		t.Fatalf("op: got %d", enc[0])
	}
	pl := binary.BigEndian.Uint16(enc[1:3])
	if int(pl) != len(path) {
		t.Fatalf("path_len %d want %d", pl, len(path))
	}
	if string(enc[3:]) != path {
		t.Fatalf("path mismatch")
	}

	op, gotPath, err := DecodeFileRequestHeader(bytes.NewReader(enc))
	if err != nil {
		t.Fatal(err)
	}
	if op != FileOpPut || gotPath != path {
		t.Fatalf("decode op=%d path=%q", op, gotPath)
	}

	encGet, err := EncodeFileRequestHeader(FileOpGet, "/tmp/x")
	if err != nil {
		t.Fatal(err)
	}
	op, gotPath, err = DecodeFileRequestHeader(bytes.NewReader(encGet))
	if err != nil {
		t.Fatal(err)
	}
	if op != FileOpGet || gotPath != "/tmp/x" {
		t.Fatalf("get decode op=%d path=%q", op, gotPath)
	}
}

func TestEncodeFileRequestHeaderRejects(t *testing.T) {
	if _, err := EncodeFileRequestHeader(9, "a"); err == nil {
		t.Fatal("bad op accepted")
	}
	if _, err := EncodeFileRequestHeader(FileOpPut, ""); err == nil {
		t.Fatal("empty path accepted")
	}
	long := strings.Repeat("a", MaxFilePathLen+1)
	if _, err := EncodeFileRequestHeader(FileOpPut, long); err == nil {
		t.Fatal("overlong path accepted")
	}
}

func TestEncodeDecodeFileChunkRoundTrip(t *testing.T) {
	payload := []byte("hello-file-chunk-payload")
	enc, err := EncodeFileChunk(payload)
	if err != nil {
		t.Fatal(err)
	}
	if binary.BigEndian.Uint32(enc[0:4]) != uint32(len(payload)) {
		t.Fatal("chunk_len mismatch")
	}
	got, err := ReadFileChunk(bytes.NewReader(enc))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, payload) {
		t.Fatalf("data mismatch %q", got)
	}

	// EOF marker
	eof, err := EncodeFileChunk(nil)
	if err != nil {
		t.Fatal(err)
	}
	if binary.BigEndian.Uint32(eof) != 0 || len(eof) != 4 {
		t.Fatalf("eof framing: %v", eof)
	}
	got, err = ReadFileChunk(bytes.NewReader(eof))
	if err != nil {
		t.Fatal(err)
	}
	if got != nil {
		t.Fatalf("eof should return nil data, got %v", got)
	}
}

func TestWriteReadFileChunkViaWriter(t *testing.T) {
	var buf bytes.Buffer
	if err := WriteFileChunk(&buf, []byte{1, 2, 3}); err != nil {
		t.Fatal(err)
	}
	if err := WriteFileChunkEOF(&buf); err != nil {
		t.Fatal(err)
	}
	r := bytes.NewReader(buf.Bytes())
	d1, err := ReadFileChunk(r)
	if err != nil || !bytes.Equal(d1, []byte{1, 2, 3}) {
		t.Fatalf("chunk1: %v %v", d1, err)
	}
	d2, err := ReadFileChunk(r)
	if err != nil || d2 != nil {
		t.Fatalf("eof: %v %v", d2, err)
	}
}

func TestFilePutResponseRoundTrip(t *testing.T) {
	enc, err := EncodeFilePutResponse(FileStatusOK, 123456789, "ok")
	if err != nil {
		t.Fatal(err)
	}
	resp, err := ReadFilePutResponse(bytes.NewReader(enc))
	if err != nil {
		t.Fatal(err)
	}
	if resp.Status != FileStatusOK || resp.Written != 123456789 || resp.Message != "ok" {
		t.Fatalf("%+v", resp)
	}

	// empty msg
	enc2, err := EncodeFilePutResponse(1, 0, "")
	if err != nil {
		t.Fatal(err)
	}
	resp2, err := ReadFilePutResponse(bytes.NewReader(enc2))
	if err != nil {
		t.Fatal(err)
	}
	if resp2.Status != 1 || resp2.Written != 0 || resp2.Message != "" {
		t.Fatalf("%+v", resp2)
	}
}

func TestFileGetHeaderOKAndErr(t *testing.T) {
	// OK + body
	var wire bytes.Buffer
	wire.Write(EncodeFileGetHeaderOK(5))
	wire.Write([]byte("abcde"))
	h, err := ReadFileGetHeader(&wire)
	if err != nil {
		t.Fatal(err)
	}
	if h.Status != FileStatusOK || h.Size != 5 {
		t.Fatalf("%+v", h)
	}
	body := make([]byte, 5)
	if _, err := io.ReadFull(&wire, body); err != nil {
		t.Fatal(err)
	}
	if string(body) != "abcde" {
		t.Fatalf("body %q", body)
	}

	// error
	errFrame, err := EncodeFileGetHeaderErr(2, "not found")
	if err != nil {
		t.Fatal(err)
	}
	h2, err := ReadFileGetHeader(bytes.NewReader(errFrame))
	if err != nil {
		t.Fatal(err)
	}
	if h2.Status != 2 || h2.Message != "not found" {
		t.Fatalf("%+v", h2)
	}

	if _, err := EncodeFileGetHeaderErr(FileStatusOK, "x"); err == nil {
		t.Fatal("status 0 error header should fail")
	}
}

func TestStreamFilePutBody(t *testing.T) {
	src := bytes.NewReader([]byte("0123456789ABCDEF"))
	var out bytes.Buffer
	n, err := StreamFilePutBody(&out, src, 4)
	if err != nil {
		t.Fatal(err)
	}
	if n != 16 {
		t.Fatalf("written %d", n)
	}
	// Reassemble
	r := bytes.NewReader(out.Bytes())
	var reassembled []byte
	for {
		chunk, err := ReadFileChunk(r)
		if err != nil {
			t.Fatal(err)
		}
		if chunk == nil {
			break
		}
		reassembled = append(reassembled, chunk...)
	}
	if string(reassembled) != "0123456789ABCDEF" {
		t.Fatalf("got %q", reassembled)
	}
	// ensure EOF consumed everything
	if r.Len() != 0 {
		t.Fatalf("leftover %d", r.Len())
	}
}

func TestStreamFilePutBodyEmpty(t *testing.T) {
	var out bytes.Buffer
	n, err := StreamFilePutBody(&out, bytes.NewReader(nil), 1024)
	if err != nil {
		t.Fatal(err)
	}
	if n != 0 {
		t.Fatalf("n=%d", n)
	}
	chunk, err := ReadFileChunk(bytes.NewReader(out.Bytes()))
	if err != nil || chunk != nil {
		t.Fatalf("want EOF only, got %v %v", chunk, err)
	}
}

func TestFullPutSessionFraming(t *testing.T) {
	// Simulate server put half + agent response on one buffer (server write side).
	var session bytes.Buffer
	// stream type
	session.WriteByte(YamuxStreamFILE)
	if err := WriteFileRequestHeader(&session, FileOpPut, "/tmp/out.bin"); err != nil {
		t.Fatal(err)
	}
	if _, err := StreamFilePutBody(&session, bytes.NewReader([]byte("payload")), 3); err != nil {
		t.Fatal(err)
	}
	// agent would write response — append for decode test
	respEnc, err := EncodeFilePutResponse(FileStatusOK, 7, "")
	if err != nil {
		t.Fatal(err)
	}
	// parse as agent would see request then we parse response separately
	r := bytes.NewReader(session.Bytes())
	var st [1]byte
	if _, err := io.ReadFull(r, st[:]); err != nil || st[0] != YamuxStreamFILE {
		t.Fatalf("type %v %v", st, err)
	}
	op, path, err := DecodeFileRequestHeader(r)
	if err != nil || op != FileOpPut || path != "/tmp/out.bin" {
		t.Fatalf("hdr op=%d path=%q err=%v", op, path, err)
	}
	var got []byte
	for {
		c, err := ReadFileChunk(r)
		if err != nil {
			t.Fatal(err)
		}
		if c == nil {
			break
		}
		got = append(got, c...)
	}
	if string(got) != "payload" {
		t.Fatalf("body %q", got)
	}
	// response path
	resp, err := ReadFilePutResponse(bytes.NewReader(respEnc))
	if err != nil || resp.Written != 7 || resp.Status != FileStatusOK {
		t.Fatalf("%+v %v", resp, err)
	}
}

func TestFullGetSessionFraming(t *testing.T) {
	var wire bytes.Buffer
	wire.WriteByte(YamuxStreamFILE)
	if err := WriteFileRequestHeader(&wire, FileOpGet, "C:\\a.bin"); err != nil {
		t.Fatal(err)
	}
	// agent response + body
	wire.Write(EncodeFileGetHeaderOK(4))
	wire.WriteString("wxyz")

	r := bytes.NewReader(wire.Bytes())
	var st [1]byte
	io.ReadFull(r, st[:])
	if st[0] != YamuxStreamFILE {
		t.Fatalf("type 0x%02x", st[0])
	}
	op, path, err := DecodeFileRequestHeader(r)
	if err != nil || op != FileOpGet || path != `C:\a.bin` {
		t.Fatalf("hdr %d %q %v", op, path, err)
	}
	h, err := ReadFileGetHeader(r)
	if err != nil || h.Status != FileStatusOK || h.Size != 4 {
		t.Fatalf("%+v %v", h, err)
	}
	body, err := io.ReadAll(io.LimitReader(r, int64(h.Size)))
	if err != nil || string(body) != "wxyz" {
		t.Fatalf("body %q %v", body, err)
	}
}
