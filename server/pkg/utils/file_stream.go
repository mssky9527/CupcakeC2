// Package utils — Yamux FILE (0x0E) binary framing (server ↔ agent).
//
// Wire protocol (must match client). After yamux.Session.Open the server writes
// the stream type byte YamuxStreamFILE (0x0E), then:
//
// Request header:
//
//	op:       u8        // FileOpPut=1 (upload panel→agent), FileOpGet=2 (download)
//	path_len: u16 BE
//	path:     [path_len] UTF-8
//
// Put (upload) body after header:
//
//	repeat:
//	  chunk_len: u32 BE  // 0 = EOF
//	  data:      [chunk_len]
//	then agent put response:
//	  status:  u8        // 0 = ok
//	  written: u64 BE
//	  msg_len: u16 BE
//	  msg:     [msg_len] UTF-8
//
// Get (download) after header:
//
//	status: u8
//	if status != 0: msg_len u16 BE + msg
//	if status == 0: size u64 BE + size raw bytes
package utils

import (
	"encoding/binary"
	"fmt"
	"io"
	"unicode/utf8"
)

// FILE stream op codes (request header op field).
const (
	FileOpPut byte = 1 // panel → agent upload
	FileOpGet byte = 2 // agent → panel download
)

// FileStatusOK is the success status in put/get responses.
const FileStatusOK byte = 0

// DefaultFileChunkSize is the default put chunk payload size (raw bytes).
const DefaultFileChunkSize = 512 * 1024

// MaxFilePathLen rejects absurd path lengths on the wire.
const MaxFilePathLen = 32 * 1024

// MaxFileChunkLen caps a single put chunk (defense-in-depth; agent may be lower).
const MaxFileChunkLen = 16 * 1024 * 1024

// MaxFileMsgLen caps status message lengths.
const MaxFileMsgLen = 16 * 1024

// EncodeFileRequestHeader builds the request header bytes (op + path_len + path).
// Does not include the Yamux stream type byte.
func EncodeFileRequestHeader(op byte, path string) ([]byte, error) {
	if op != FileOpPut && op != FileOpGet {
		return nil, fmt.Errorf("invalid file op 0x%02x", op)
	}
	if path == "" {
		return nil, fmt.Errorf("empty path")
	}
	if !utf8.ValidString(path) {
		return nil, fmt.Errorf("path is not valid UTF-8")
	}
	pl := len(path)
	if pl > MaxFilePathLen {
		return nil, fmt.Errorf("path too long: %d", pl)
	}
	if pl > 0xFFFF {
		return nil, fmt.Errorf("path exceeds u16 length")
	}
	buf := make([]byte, 1+2+pl)
	buf[0] = op
	binary.BigEndian.PutUint16(buf[1:3], uint16(pl))
	copy(buf[3:], path)
	return buf, nil
}

// WriteFileRequestHeader writes EncodeFileRequestHeader to w.
func WriteFileRequestHeader(w io.Writer, op byte, path string) error {
	hdr, err := EncodeFileRequestHeader(op, path)
	if err != nil {
		return err
	}
	_, err = w.Write(hdr)
	return err
}

// DecodeFileRequestHeader parses a request header from r (after stream type).
func DecodeFileRequestHeader(r io.Reader) (op byte, path string, err error) {
	var head [3]byte
	if _, err = io.ReadFull(r, head[:]); err != nil {
		return 0, "", fmt.Errorf("read file request header: %w", err)
	}
	op = head[0]
	pl := int(binary.BigEndian.Uint16(head[1:3]))
	if pl > MaxFilePathLen {
		return 0, "", fmt.Errorf("path_len too large: %d", pl)
	}
	if pl == 0 {
		return 0, "", fmt.Errorf("empty path")
	}
	pbuf := make([]byte, pl)
	if _, err = io.ReadFull(r, pbuf); err != nil {
		return 0, "", fmt.Errorf("read path: %w", err)
	}
	if !utf8.Valid(pbuf) {
		return 0, "", fmt.Errorf("path is not valid UTF-8")
	}
	return op, string(pbuf), nil
}

// EncodeFileChunk builds chunk_len(u32 BE) + data. Empty data is valid (EOF marker).
func EncodeFileChunk(data []byte) ([]byte, error) {
	n := len(data)
	if n > MaxFileChunkLen {
		return nil, fmt.Errorf("chunk too large: %d", n)
	}
	buf := make([]byte, 4+n)
	binary.BigEndian.PutUint32(buf[0:4], uint32(n))
	copy(buf[4:], data)
	return buf, nil
}

// WriteFileChunk writes one put chunk (length-prefixed). Use nil/empty for EOF.
func WriteFileChunk(w io.Writer, data []byte) error {
	enc, err := EncodeFileChunk(data)
	if err != nil {
		return err
	}
	_, err = w.Write(enc)
	return err
}

// WriteFileChunkEOF writes chunk_len=0 to signal end of put body.
func WriteFileChunkEOF(w io.Writer) error {
	return WriteFileChunk(w, nil)
}

// ReadFileChunk reads one put chunk; nil data and n==0 means EOF.
func ReadFileChunk(r io.Reader) (data []byte, err error) {
	var lenBuf [4]byte
	if _, err = io.ReadFull(r, lenBuf[:]); err != nil {
		return nil, fmt.Errorf("read chunk_len: %w", err)
	}
	n := binary.BigEndian.Uint32(lenBuf[:])
	if n > MaxFileChunkLen {
		return nil, fmt.Errorf("chunk_len too large: %d", n)
	}
	if n == 0 {
		return nil, nil
	}
	data = make([]byte, n)
	if _, err = io.ReadFull(r, data); err != nil {
		return nil, fmt.Errorf("read chunk data: %w", err)
	}
	return data, nil
}

// FilePutResponse is the agent reply after put EOF.
type FilePutResponse struct {
	Status  byte
	Written uint64
	Message string
}

// EncodeFilePutResponse builds the put response frame.
func EncodeFilePutResponse(status byte, written uint64, msg string) ([]byte, error) {
	if !utf8.ValidString(msg) {
		return nil, fmt.Errorf("msg is not valid UTF-8")
	}
	ml := len(msg)
	if ml > MaxFileMsgLen {
		return nil, fmt.Errorf("msg too long: %d", ml)
	}
	if ml > 0xFFFF {
		return nil, fmt.Errorf("msg exceeds u16 length")
	}
	buf := make([]byte, 1+8+2+ml)
	buf[0] = status
	binary.BigEndian.PutUint64(buf[1:9], written)
	binary.BigEndian.PutUint16(buf[9:11], uint16(ml))
	copy(buf[11:], msg)
	return buf, nil
}

// WriteFilePutResponse writes a put response to w.
func WriteFilePutResponse(w io.Writer, status byte, written uint64, msg string) error {
	enc, err := EncodeFilePutResponse(status, written, msg)
	if err != nil {
		return err
	}
	_, err = w.Write(enc)
	return err
}

// ReadFilePutResponse parses the put response after the server finishes sending chunks.
func ReadFilePutResponse(r io.Reader) (*FilePutResponse, error) {
	var head [1 + 8 + 2]byte
	if _, err := io.ReadFull(r, head[:]); err != nil {
		return nil, fmt.Errorf("read put response: %w", err)
	}
	status := head[0]
	written := binary.BigEndian.Uint64(head[1:9])
	ml := int(binary.BigEndian.Uint16(head[9:11]))
	if ml > MaxFileMsgLen {
		return nil, fmt.Errorf("put msg_len too large: %d", ml)
	}
	var msg string
	if ml > 0 {
		mbuf := make([]byte, ml)
		if _, err := io.ReadFull(r, mbuf); err != nil {
			return nil, fmt.Errorf("read put msg: %w", err)
		}
		if !utf8.Valid(mbuf) {
			return nil, fmt.Errorf("put msg is not valid UTF-8")
		}
		msg = string(mbuf)
	}
	return &FilePutResponse{Status: status, Written: written, Message: msg}, nil
}

// FileGetHeader is the agent reply before download body (or error only).
type FileGetHeader struct {
	Status  byte
	Size    uint64 // meaningful when Status == FileStatusOK
	Message string // meaningful when Status != FileStatusOK
}

// EncodeFileGetHeaderOK builds status=0 + size (body follows separately).
func EncodeFileGetHeaderOK(size uint64) []byte {
	buf := make([]byte, 1+8)
	buf[0] = FileStatusOK
	binary.BigEndian.PutUint64(buf[1:9], size)
	return buf
}

// EncodeFileGetHeaderErr builds status!=0 + msg_len + msg (no body).
func EncodeFileGetHeaderErr(status byte, msg string) ([]byte, error) {
	if status == FileStatusOK {
		return nil, fmt.Errorf("error status must be non-zero")
	}
	if !utf8.ValidString(msg) {
		return nil, fmt.Errorf("msg is not valid UTF-8")
	}
	ml := len(msg)
	if ml > MaxFileMsgLen {
		return nil, fmt.Errorf("msg too long: %d", ml)
	}
	if ml > 0xFFFF {
		return nil, fmt.Errorf("msg exceeds u16 length")
	}
	buf := make([]byte, 1+2+ml)
	buf[0] = status
	binary.BigEndian.PutUint16(buf[1:3], uint16(ml))
	copy(buf[3:], msg)
	return buf, nil
}

// ReadFileGetHeader parses get response header; body of Size bytes follows on success.
func ReadFileGetHeader(r io.Reader) (*FileGetHeader, error) {
	var st [1]byte
	if _, err := io.ReadFull(r, st[:]); err != nil {
		return nil, fmt.Errorf("read get status: %w", err)
	}
	h := &FileGetHeader{Status: st[0]}
	if h.Status != FileStatusOK {
		var mlBuf [2]byte
		if _, err := io.ReadFull(r, mlBuf[:]); err != nil {
			return nil, fmt.Errorf("read get msg_len: %w", err)
		}
		ml := int(binary.BigEndian.Uint16(mlBuf[:]))
		if ml > MaxFileMsgLen {
			return nil, fmt.Errorf("get msg_len too large: %d", ml)
		}
		if ml > 0 {
			mbuf := make([]byte, ml)
			if _, err := io.ReadFull(r, mbuf); err != nil {
				return nil, fmt.Errorf("read get msg: %w", err)
			}
			if !utf8.Valid(mbuf) {
				return nil, fmt.Errorf("get msg is not valid UTF-8")
			}
			h.Message = string(mbuf)
		}
		return h, nil
	}
	var sizeBuf [8]byte
	if _, err := io.ReadFull(r, sizeBuf[:]); err != nil {
		return nil, fmt.Errorf("read get size: %w", err)
	}
	h.Size = binary.BigEndian.Uint64(sizeBuf[:])
	return h, nil
}

// StreamFilePutBody copies r to w as length-prefixed put chunks ending with EOF.
// Returns total raw bytes written (excluding framing).
func StreamFilePutBody(w io.Writer, r io.Reader, chunkSize int) (int64, error) {
	if chunkSize <= 0 {
		chunkSize = DefaultFileChunkSize
	}
	if chunkSize > MaxFileChunkLen {
		chunkSize = MaxFileChunkLen
	}
	buf := make([]byte, chunkSize)
	var total int64
	for {
		n, err := r.Read(buf)
		if n > 0 {
			if errW := WriteFileChunk(w, buf[:n]); errW != nil {
				return total, errW
			}
			total += int64(n)
		}
		if err == io.EOF {
			break
		}
		if err != nil {
			return total, err
		}
	}
	if err := WriteFileChunkEOF(w); err != nil {
		return total, err
	}
	return total, nil
}
