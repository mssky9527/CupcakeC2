//! CPXD desktop session framing (pure codec — no DXGI, always compiled).
//!
//! Contract: `docs/DESKTOP_MODULE_DESIGN.md` §4.
//! Magic is fixed `CPXD` (not wire-seed). Dual-side max payload 2 MiB.

/// Fixed product magic (LE bytes 'C','P','X','D').
pub const DESKTOP_MAGIC: [u8; 4] = *b"CPXD";
pub const DESKTOP_PROTO_VERSION: u8 = 1;
pub const DESKTOP_MAX_PAYLOAD: u32 = 2 * 1024 * 1024;
pub const DESKTOP_HEADER_LEN: usize = 12;

pub const MSG_HELLO: u8 = 0x01;
pub const MSG_HELLO_ACK: u8 = 0x02;
pub const MSG_FRAME: u8 = 0x03;
pub const MSG_INPUT: u8 = 0x04;
pub const MSG_CONFIG: u8 = 0x05;
pub const MSG_PING: u8 = 0x06;
pub const MSG_PONG: u8 = 0x07;
pub const MSG_ERROR: u8 = 0x08;
pub const MSG_STOP: u8 = 0x09;
pub const MSG_KEYFRAME_REQ: u8 = 0x0A;
pub const MSG_STATS: u8 = 0x10;

pub const ENCODE_JPEG: u8 = 1;
pub const ENCODE_H264: u8 = 2; // reserved

pub const FRAME_FLAG_KEYFRAME: u8 = 0x01;
pub const FRAME_FLAG_DIRTY: u8 = 0x02;

pub const INPUT_MOUSE_MOVE: u8 = 1;
pub const INPUT_MOUSE_BTN: u8 = 2;
pub const INPUT_WHEEL: u8 = 3;
pub const INPUT_KEY: u8 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeHeader {
    pub version: u8,
    pub msg_type: u8,
    pub flags: u16,
    pub payload_len: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseHeaderError {
    /// Bad magic or unsupported version — caller must silent-close (no ERROR oracle).
    SilentClose,
    /// Header incomplete.
    Truncated,
    /// payload_len exceeds 2 MiB.
    PayloadTooLarge,
}

/// Parse 12-byte envelope header. Does **not** read payload.
pub fn parse_header(buf: &[u8]) -> Result<EnvelopeHeader, ParseHeaderError> {
    if buf.len() < DESKTOP_HEADER_LEN {
        return Err(ParseHeaderError::Truncated);
    }
    if buf[0..4] != DESKTOP_MAGIC {
        return Err(ParseHeaderError::SilentClose);
    }
    if buf[4] != DESKTOP_PROTO_VERSION {
        return Err(ParseHeaderError::SilentClose);
    }
    let msg_type = buf[5];
    let flags = u16::from_le_bytes([buf[6], buf[7]]);
    let payload_len = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
    if payload_len > DESKTOP_MAX_PAYLOAD {
        return Err(ParseHeaderError::PayloadTooLarge);
    }
    Ok(EnvelopeHeader {
        version: DESKTOP_PROTO_VERSION,
        msg_type,
        flags,
        payload_len,
    })
}

/// Encode full message: header + payload. Returns None if payload too large.
pub fn encode_message(msg_type: u8, flags: u16, payload: &[u8]) -> Option<Vec<u8>> {
    if payload.len() as u32 > DESKTOP_MAX_PAYLOAD {
        return None;
    }
    let mut out = Vec::with_capacity(DESKTOP_HEADER_LEN + payload.len());
    out.extend_from_slice(&DESKTOP_MAGIC);
    out.push(DESKTOP_PROTO_VERSION);
    out.push(msg_type);
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    Some(out)
}

/// FRAME payload header (before JPEG bytes). rect_count=0 → full frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    pub w: u16,
    pub h: u16,
    pub encode: u8,
    pub frame_flags: u8,
    pub rect_count: u16,
    pub ts_ms: u32,
    pub seq: u32,
}

pub fn encode_frame_payload(hdr: &FrameHeader, jpeg: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(16 + jpeg.len());
    p.extend_from_slice(&hdr.w.to_le_bytes());
    p.extend_from_slice(&hdr.h.to_le_bytes());
    p.push(hdr.encode);
    p.push(hdr.frame_flags);
    p.extend_from_slice(&hdr.rect_count.to_le_bytes());
    p.extend_from_slice(&hdr.ts_ms.to_le_bytes());
    p.extend_from_slice(&hdr.seq.to_le_bytes());
    p.extend_from_slice(jpeg);
    p
}

pub fn parse_frame_payload(buf: &[u8]) -> Option<(FrameHeader, &[u8])> {
    if buf.len() < 16 {
        return None;
    }
    let w = u16::from_le_bytes([buf[0], buf[1]]);
    let h = u16::from_le_bytes([buf[2], buf[3]]);
    let encode = buf[4];
    let frame_flags = buf[5];
    let rect_count = u16::from_le_bytes([buf[6], buf[7]]);
    let ts_ms = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
    let seq = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
    // rects omitted when rect_count==0; dirty rects would follow at 16
    let rects_bytes = (rect_count as usize).saturating_mul(8);
    if buf.len() < 16 + rects_bytes {
        return None;
    }
    let jpeg = &buf[16 + rects_bytes..];
    Some((
        FrameHeader {
            w,
            h,
            encode,
            frame_flags,
            rect_count,
            ts_ms,
            seq,
        },
        jpeg,
    ))
}

/// Map frame-space mouse coords to physical desktop pixels.
/// `frame_w`/`frame_h` are the encoded FRAME dimensions; physical_* from capture.
pub fn map_input_to_physical(
    x: u16,
    y: u16,
    frame_w: u16,
    frame_h: u16,
    physical_w: u16,
    physical_h: u16,
) -> Option<(u16, u16)> {
    if frame_w == 0 || frame_h == 0 || physical_w == 0 || physical_h == 0 {
        return None;
    }
    let px = (x as u32)
        .saturating_mul(physical_w as u32)
        .checked_div(frame_w as u32)? as u16;
    let py = (y as u32)
        .saturating_mul(physical_h as u32)
        .checked_div(frame_h as u32)? as u16;
    Some((
        px.min(physical_w.saturating_sub(1)),
        py.min(physical_h.saturating_sub(1)),
    ))
}

/// Encode mouse button input in frame pixel space.
pub fn encode_mouse_btn(x: u16, y: u16, button: u8, down: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(7);
    v.push(INPUT_MOUSE_BTN);
    v.extend_from_slice(&x.to_le_bytes());
    v.extend_from_slice(&y.to_le_bytes());
    v.push(button);
    v.push(down);
    v
}

pub fn parse_mouse_btn(buf: &[u8]) -> Option<(u16, u16, u8, u8)> {
    if buf.len() < 7 || buf[0] != INPUT_MOUSE_BTN {
        return None;
    }
    let x = u16::from_le_bytes([buf[1], buf[2]]);
    let y = u16::from_le_bytes([buf[3], buf[4]]);
    Some((x, y, buf[5], buf[6]))
}

/// Auto-degrade ladder (design §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamQuality {
    pub fps: u32,
    pub quality: u32,
    pub max_w: u32,
}

impl StreamQuality {
    pub fn mvp_default() -> Self {
        Self {
            fps: 5,
            quality: 75,
            max_w: 1280,
        }
    }

    /// One step down; returns None if already at floor.
    pub fn degrade(self) -> Option<Self> {
        let next_fps = match self.fps {
            5 => 3,
            3 => 2,
            2 => 1,
            _ => self.fps,
        };
        let next_q = match self.quality {
            q if q > 60 => 60,
            q if q > 45 => 45,
            q if q > 30 => 30,
            _ => self.quality,
        };
        let next_w = match self.max_w {
            w if w > 1024 => 1024,
            w if w > 800 => 800,
            _ => self.max_w,
        };
        let n = Self {
            fps: next_fps.min(self.fps),
            quality: next_q.min(self.quality),
            max_w: next_w.min(self.max_w),
        };
        if n == self {
            None
        } else {
            Some(n)
        }
    }
}

/// Idempotent session lifecycle for STOP/EOF teardown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Active,
    Detaching,
    Detached,
}

#[derive(Debug)]
pub struct SessionGuard {
    state: SessionState,
}

impl SessionGuard {
    pub fn new() -> Self {
        Self {
            state: SessionState::Idle,
        }
    }

    pub fn attach(&mut self) -> bool {
        match self.state {
            SessionState::Idle | SessionState::Detached => {
                self.state = SessionState::Active;
                true
            }
            SessionState::Active => false,
            SessionState::Detaching => false,
        }
    }

    /// Idempotent: second call returns true (ok) without re-running work flag.
    /// Returns (should_run_teardown_work, ok).
    pub fn detach_once(&mut self) -> (bool, bool) {
        match self.state {
            SessionState::Active => {
                self.state = SessionState::Detaching;
                self.state = SessionState::Detached;
                (true, true)
            }
            SessionState::Detaching | SessionState::Detached => (false, true),
            SessionState::Idle => (false, true),
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }
}

impl Default for SessionGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// Minimal valid grayscale JPEG (8x8) for tests / synthetic frames.
pub fn minimal_jpeg_8x8() -> &'static [u8] {
    // Prebuilt minimal JPEG SOI…EOI (public domain style test fixture).
    static JPEG: &[u8] = &[
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06,
        0x05, 0x08, 0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B,
        0x0C, 0x19, 0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20,
        0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28, 0x37, 0x29, 0x2C, 0x30, 0x31,
        0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32, 0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF,
        0xC0, 0x00, 0x0B, 0x08, 0x00, 0x08, 0x00, 0x08, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00,
        0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
        0xFF, 0xC4, 0x00, 0xB5, 0x10, 0x00, 0x02, 0x01, 0x03, 0x03, 0x02, 0x04, 0x03, 0x05, 0x05,
        0x04, 0x04, 0x00, 0x00, 0x01, 0x7D, 0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21,
        0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xA1, 0x08,
        0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0A,
        0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x34, 0x35, 0x36, 0x37,
        0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56,
        0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75,
        0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93,
        0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9,
        0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6,
        0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE1, 0xE2,
        0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7,
        0xF8, 0xF9, 0xFA, 0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0x7F, 0x3F,
        0xFF, 0xD9,
    ];
    JPEG
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_parse_hello_roundtrip() {
        let body = br#"{"fps":5,"quality":75,"max_w":1280,"encode":"jpeg"}"#;
        let msg = encode_message(MSG_HELLO, 0, body).expect("encode");
        let hdr = parse_header(&msg).expect("header");
        assert_eq!(hdr.msg_type, MSG_HELLO);
        assert_eq!(hdr.payload_len as usize, body.len());
        assert_eq!(&msg[DESKTOP_HEADER_LEN..], body);
    }

    #[test]
    fn bad_magic_silent_close() {
        let mut bad = encode_message(MSG_HELLO, 0, b"{}").unwrap();
        bad[0] = b'X';
        assert_eq!(parse_header(&bad), Err(ParseHeaderError::SilentClose));
    }

    #[test]
    fn bad_version_silent_close() {
        let mut bad = encode_message(MSG_STOP, 0, &[]).unwrap();
        bad[4] = 99;
        assert_eq!(parse_header(&bad), Err(ParseHeaderError::SilentClose));
    }

    #[test]
    fn payload_over_2mib_rejected_on_encode_and_parse() {
        assert!(encode_message(MSG_FRAME, 0, &vec![0u8; DESKTOP_MAX_PAYLOAD as usize + 1]).is_none());
        let mut hdr = [0u8; 12];
        hdr[0..4].copy_from_slice(&DESKTOP_MAGIC);
        hdr[4] = DESKTOP_PROTO_VERSION;
        hdr[5] = MSG_FRAME;
        let too_big = DESKTOP_MAX_PAYLOAD + 1;
        hdr[8..12].copy_from_slice(&too_big.to_le_bytes());
        assert_eq!(parse_header(&hdr), Err(ParseHeaderError::PayloadTooLarge));
    }

    #[test]
    fn frame_header_roundtrip_with_jpeg() {
        let jpeg = minimal_jpeg_8x8();
        assert_eq!(jpeg[0], 0xFF);
        assert_eq!(jpeg[1], 0xD8);
        let fh = FrameHeader {
            w: 1280,
            h: 720,
            encode: ENCODE_JPEG,
            frame_flags: FRAME_FLAG_KEYFRAME,
            rect_count: 0,
            ts_ms: 42,
            seq: 7,
        };
        let payload = encode_frame_payload(&fh, jpeg);
        let msg = encode_message(MSG_FRAME, 0, &payload).unwrap();
        let env = parse_header(&msg).unwrap();
        assert_eq!(env.msg_type, MSG_FRAME);
        let (parsed, j) = parse_frame_payload(&msg[DESKTOP_HEADER_LEN..]).unwrap();
        assert_eq!(parsed, fh);
        assert_eq!(j, jpeg);
    }

    #[test]
    fn input_scale_4k_to_1280() {
        // Click at center of 1280x720 frame → center of 3840x2160 physical
        let (px, py) = map_input_to_physical(640, 360, 1280, 720, 3840, 2160).unwrap();
        assert_eq!(px, 1920);
        assert_eq!(py, 1080);
    }

    #[test]
    fn input_scale_rejects_zero_frame() {
        assert!(map_input_to_physical(10, 10, 0, 720, 1920, 1080).is_none());
    }

    #[test]
    fn mouse_btn_codec() {
        let b = encode_mouse_btn(100, 200, 1, 1);
        let (x, y, btn, down) = parse_mouse_btn(&b).unwrap();
        assert_eq!((x, y, btn, down), (100, 200, 1, 1));
    }

    #[test]
    fn degrade_ladder_reaches_floor() {
        let mut q = StreamQuality::mvp_default();
        let mut steps = 0;
        while let Some(n) = q.degrade() {
            q = n;
            steps += 1;
            assert!(steps < 20);
        }
        assert_eq!(q.fps, 1);
        assert_eq!(q.quality, 30);
        assert_eq!(q.max_w, 800);
    }

    #[test]
    fn detach_idempotent() {
        let mut g = SessionGuard::new();
        assert!(g.attach());
        let (work1, ok1) = g.detach_once();
        assert!(work1 && ok1);
        let (work2, ok2) = g.detach_once();
        assert!(!work2 && ok2);
        assert_eq!(g.state(), SessionState::Detached);
    }

    #[test]
    fn h264_encode_const_reserved_not_crash() {
        // Switch bit only — consumers treat ENCODE_H264 as unsupported without panic.
        assert_eq!(ENCODE_H264, 2);
        let fh = FrameHeader {
            w: 8,
            h: 8,
            encode: ENCODE_H264,
            frame_flags: FRAME_FLAG_KEYFRAME,
            rect_count: 0,
            ts_ms: 0,
            seq: 0,
        };
        let p = encode_frame_payload(&fh, minimal_jpeg_8x8());
        let (parsed, _) = parse_frame_payload(&p).unwrap();
        assert_eq!(parsed.encode, ENCODE_H264);
    }
}
