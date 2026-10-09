//! Binary data frames (PROTOCOL.md section 6).

/// Bytes in the common header: type, workspace, run, sequence, ticks.
pub const HEADER_BYTES: usize = 1 + 4 + 4 + 8 + 8;

/// A decoded binary frame.
#[derive(Debug, Clone, PartialEq)]
pub enum DataFrame {
    View(ViewFrame),
}

/// NetLogo's rendered view (section 6.1).
#[derive(Debug, Clone, PartialEq)]
pub struct ViewFrame {
    pub workspace: u32,
    pub run: u32,
    pub sequence: u64,
    /// None when the model's ticks have not been reset.
    pub ticks: Option<f64>,
    pub width: u32,
    pub height: u32,
    /// R, G, B, A bytes, row-major from the top-left.
    pub rgba: Vec<u8>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DataFrameError {
    #[error("binary frame is {0} bytes, shorter than its header")]
    Short(usize),
    #[error("unknown binary frame type {0}")]
    UnknownType(u8),
    #[error("view frame of {width}x{height} needs {expected} pixel bytes, has {actual}")]
    PixelCount {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },
}

impl DataFrame {
    pub fn decode(payload: &[u8]) -> Result<DataFrame, DataFrameError> {
        if payload.len() < HEADER_BYTES {
            return Err(DataFrameError::Short(payload.len()));
        }
        let u32_at = |i: usize| u32::from_le_bytes(payload[i..i + 4].try_into().unwrap());
        let u64_at = |i: usize| u64::from_le_bytes(payload[i..i + 8].try_into().unwrap());
        let ty = payload[0];
        let workspace = u32_at(1);
        let run = u32_at(5);
        let sequence = u64_at(9);
        let ticks = f64::from_bits(u64_at(17));
        let ticks = (!ticks.is_nan()).then_some(ticks);
        match ty {
            0x01 => {
                if payload.len() < HEADER_BYTES + 8 {
                    return Err(DataFrameError::Short(payload.len()));
                }
                let width = u32_at(HEADER_BYTES);
                let height = u32_at(HEADER_BYTES + 4);
                let pixels = &payload[HEADER_BYTES + 8..];
                let expected = width as usize * height as usize * 4;
                if pixels.len() != expected {
                    return Err(DataFrameError::PixelCount {
                        width,
                        height,
                        expected,
                        actual: pixels.len(),
                    });
                }
                Ok(DataFrame::View(ViewFrame {
                    workspace,
                    run,
                    sequence,
                    ticks,
                    width,
                    height,
                    rgba: pixels.to_vec(),
                }))
            }
            other => Err(DataFrameError::UnknownType(other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a view frame the way the bridge's ViewFrame.encode does.
    fn encode(ticks: f64, width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
        let mut b = vec![0x01];
        b.extend_from_slice(&7u32.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&5u64.to_le_bytes());
        b.extend_from_slice(&ticks.to_le_bytes());
        b.extend_from_slice(&width.to_le_bytes());
        b.extend_from_slice(&height.to_le_bytes());
        b.extend_from_slice(rgba);
        b
    }

    #[test]
    fn decodes_a_view_frame() {
        let px = [1, 2, 3, 255, 4, 5, 6, 255];
        let DataFrame::View(v) = DataFrame::decode(&encode(12.0, 2, 1, &px)).unwrap();
        assert_eq!(
            (v.workspace, v.run, v.sequence, v.ticks, v.width, v.height),
            (7, 2, 5, Some(12.0), 2, 1)
        );
        assert_eq!(v.rgba, px);
    }

    #[test]
    fn nan_ticks_mean_no_ticks() {
        let DataFrame::View(v) = DataFrame::decode(&encode(f64::NAN, 1, 1, &[0; 4])).unwrap();
        assert_eq!(v.ticks, None);
    }

    #[test]
    fn rejects_bad_frames() {
        assert_eq!(DataFrame::decode(&[1, 2]), Err(DataFrameError::Short(2)));
        let mut wrong = encode(0.0, 2, 2, &[0; 4]);
        assert!(matches!(
            DataFrame::decode(&wrong),
            Err(DataFrameError::PixelCount {
                expected: 16,
                actual: 4,
                ..
            })
        ));
        wrong[0] = 9;
        assert_eq!(
            DataFrame::decode(&wrong),
            Err(DataFrameError::UnknownType(9))
        );
    }
}
