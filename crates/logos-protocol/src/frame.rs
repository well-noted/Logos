//! Frame encoding (PROTOCOL.md section 2).
//!
//! A frame is a 4-byte little-endian length, a 1-byte kind, and `length - 1` bytes of payload.

use std::io::{self, Read, Write};

/// Largest allowed value of the length field (256 MiB).
pub const MAX_LENGTH: u32 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Json = 0x01,
    Binary = 0x02,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: Kind,
    pub payload: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("stream ended inside a frame")]
    Truncated,
    #[error("frame length must be at least 1")]
    ZeroLength,
    #[error("frame length {0} exceeds the maximum")]
    TooLong(u64),
    #[error("unknown frame kind {0}")]
    UnknownKind(u8),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Reads one frame. Returns `Ok(None)` if the stream ends cleanly before a frame starts.
pub fn read_frame(r: &mut impl Read) -> Result<Option<Frame>, FrameError> {
    let mut header = [0u8; 4];
    let mut filled = 0;
    while filled < 4 {
        match r.read(&mut header[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => return Err(FrameError::Truncated),
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    let length = u32::from_le_bytes(header);
    if length == 0 {
        return Err(FrameError::ZeroLength);
    }
    if length > MAX_LENGTH {
        return Err(FrameError::TooLong(length.into()));
    }
    let mut body = vec![0u8; length as usize];
    r.read_exact(&mut body).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof => FrameError::Truncated,
        _ => FrameError::Io(e),
    })?;
    let kind = match body[0] {
        0x01 => Kind::Json,
        0x02 => Kind::Binary,
        other => return Err(FrameError::UnknownKind(other)),
    };
    body.remove(0);
    Ok(Some(Frame {
        kind,
        payload: body,
    }))
}

/// Writes one frame and flushes.
pub fn write_frame(w: &mut impl Write, kind: Kind, payload: &[u8]) -> Result<(), FrameError> {
    let length = payload.len() as u64 + 1;
    if length > u64::from(MAX_LENGTH) {
        return Err(FrameError::TooLong(length));
    }
    let mut header = [0u8; 5];
    header[..4].copy_from_slice(&(length as u32).to_le_bytes());
    header[4] = kind as u8;
    w.write_all(&header)?;
    w.write_all(payload)?;
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_frames() {
        let mut buf = Vec::new();
        write_frame(&mut buf, Kind::Json, b"{}").unwrap();
        write_frame(&mut buf, Kind::Binary, &[1, 2, 3]).unwrap();
        let mut r = &buf[..];
        assert_eq!(
            read_frame(&mut r).unwrap(),
            Some(Frame {
                kind: Kind::Json,
                payload: b"{}".to_vec()
            })
        );
        assert_eq!(read_frame(&mut r).unwrap().unwrap().payload, vec![1, 2, 3]);
        assert_eq!(read_frame(&mut r).unwrap(), None);
    }

    /// Same bytes as the Java side's FramesTest.lengthIsLittleEndianAndCountsTheKindByte.
    #[test]
    fn header_layout_matches_the_java_side() {
        let mut buf = Vec::new();
        write_frame(&mut buf, Kind::Binary, &[0u8; 300]).unwrap();
        assert_eq!(&buf[..5], &[0x2D, 0x01, 0, 0, 0x02]);
        assert_eq!(buf.len(), 4 + 301);
    }

    #[test]
    fn rejects_bad_frames() {
        assert!(matches!(
            read_frame(&mut &[0u8, 0, 0, 0][..]),
            Err(FrameError::ZeroLength)
        ));
        assert!(matches!(
            read_frame(&mut &[0xFF, 0xFF, 0xFF, 0x7F, 1][..]),
            Err(FrameError::TooLong(_))
        ));
        assert!(matches!(
            read_frame(&mut &[1u8, 0, 0, 0, 9][..]),
            Err(FrameError::UnknownKind(9))
        ));
        assert!(matches!(
            read_frame(&mut &[5u8, 0][..]),
            Err(FrameError::Truncated)
        ));
        assert!(matches!(
            read_frame(&mut &[5u8, 0, 0, 0, 1, b'a'][..]),
            Err(FrameError::Truncated)
        ));
    }
}
