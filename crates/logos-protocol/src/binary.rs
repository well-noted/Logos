//! Binary data frames (PROTOCOL.md section 6).

/// Bytes in the common header: type, workspace, run, sequence, ticks.
pub const HEADER_BYTES: usize = 1 + 4 + 4 + 8 + 8;

/// A decoded binary frame.
#[derive(Debug, Clone, PartialEq)]
pub enum DataFrame {
    View(ViewFrame),
    /// Boxed: an agent frame's columns make it much larger than a view frame's header.
    Agents(Box<AgentFrame>),
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

/// The agent-state layout version this crate reads (section 6.2, protocol 1).
pub const AGENT_LAYOUT_VERSION: u8 = 1;

/// Patch colors, turtle variables and link variables (section 6.2, layout version 1).
#[derive(Debug, Clone, PartialEq)]
pub struct AgentFrame {
    pub workspace: u32,
    pub run: u32,
    pub sequence: u64,
    /// None when the model's ticks have not been reset.
    pub ticks: Option<f64>,
    pub min_pxcor: i32,
    pub max_pxcor: i32,
    pub min_pycor: i32,
    pub max_pycor: i32,
    /// R, G, B, A bytes per patch, row-major from the top-left patch.
    pub patch_rgba: Vec<u8>,
    pub turtles: TurtleColumns,
    pub links: LinkColumns,
}

/// One entry per link in each column, ordered by `end1`, `end2` and breed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LinkColumns {
    /// `who` of each end.
    pub end1: Vec<f64>,
    pub end2: Vec<f64>,
    /// R, G, B, A.
    pub color: Vec<[u8; 4]>,
    pub thickness: Vec<f64>,
    /// Index into the model's link shapes: "default" first, then by name.
    pub shape: Vec<u16>,
    /// 0 for `links`, then link breeds in declaration order from 1.
    pub breed: Vec<u16>,
    pub hidden: Vec<bool>,
    pub directed: Vec<bool>,
}

impl LinkColumns {
    pub fn len(&self) -> usize {
        self.end1.len()
    }

    pub fn is_empty(&self) -> bool {
        self.end1.is_empty()
    }
}

/// One entry per turtle in each column, in `who` order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TurtleColumns {
    pub who: Vec<f64>,
    pub xcor: Vec<f64>,
    pub ycor: Vec<f64>,
    pub heading: Vec<f64>,
    /// R, G, B, A.
    pub color: Vec<[u8; 4]>,
    pub size: Vec<f64>,
    /// Index into the model's turtle shapes: "default" first, then by name.
    pub shape: Vec<u16>,
    /// 0 for `turtles`, then breeds in declaration order from 1.
    pub breed: Vec<u16>,
    pub hidden: Vec<bool>,
}

impl TurtleColumns {
    pub fn len(&self) -> usize {
        self.who.len()
    }

    pub fn is_empty(&self) -> bool {
        self.who.is_empty()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DataFrameError {
    #[error("binary frame is {0} bytes, shorter than its header")]
    Short(usize),
    #[error("unknown binary frame type {0}")]
    UnknownType(u8),
    #[error("agent frame: {0}")]
    Agents(String),
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
            0x02 => {
                let mut r = Reader {
                    bytes: payload,
                    at: HEADER_BYTES,
                };
                let version = r.take(1)?[0];
                if version != AGENT_LAYOUT_VERSION {
                    return Err(DataFrameError::Agents(format!(
                        "layout version {version}, expected {AGENT_LAYOUT_VERSION}"
                    )));
                }
                let patch_count = r.u32()? as usize;
                let (min_pxcor, max_pxcor) = (r.i32()?, r.i32()?);
                let (min_pycor, max_pycor) = (r.i32()?, r.i32()?);
                let patch_rgba = r.take(patch_count.checked_mul(4).ok_or_else(r.overflow())?)?;
                let n = r.u32()? as usize;
                let turtles = TurtleColumns {
                    who: r.f64s(n)?,
                    xcor: r.f64s(n)?,
                    ycor: r.f64s(n)?,
                    heading: r.f64s(n)?,
                    color: r.rgba(n)?,
                    size: r.f64s(n)?,
                    shape: r.u16s(n)?,
                    breed: r.u16s(n)?,
                    hidden: r.bools(n)?,
                };
                let m = r.u32()? as usize;
                let links = LinkColumns {
                    end1: r.f64s(m)?,
                    end2: r.f64s(m)?,
                    color: r.rgba(m)?,
                    thickness: r.f64s(m)?,
                    shape: r.u16s(m)?,
                    breed: r.u16s(m)?,
                    hidden: r.bools(m)?,
                    directed: r.bools(m)?,
                };
                if r.at != payload.len() {
                    return Err(DataFrameError::Agents(format!(
                        "{} bytes left over",
                        payload.len() - r.at
                    )));
                }
                Ok(DataFrame::Agents(Box::new(AgentFrame {
                    workspace,
                    run,
                    sequence,
                    ticks,
                    min_pxcor,
                    max_pxcor,
                    min_pycor,
                    max_pycor,
                    patch_rgba: patch_rgba.to_vec(),
                    turtles,
                    links,
                })))
            }
            other => Err(DataFrameError::UnknownType(other)),
        }
    }
}

/// Reads little-endian fields from an agent frame, failing instead of panicking when the
/// frame is shorter than its counts say.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], DataFrameError> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.bytes.len());
        let Some(end) = end else {
            return Err(DataFrameError::Agents(format!(
                "needs {n} bytes at offset {}, frame is {} bytes",
                self.at,
                self.bytes.len()
            )));
        };
        let s = &self.bytes[self.at..end];
        self.at = end;
        Ok(s)
    }

    fn overflow(&self) -> impl FnOnce() -> DataFrameError {
        || DataFrameError::Agents("count too large".into())
    }

    fn u32(&mut self) -> Result<u32, DataFrameError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, DataFrameError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn f64s(&mut self, n: usize) -> Result<Vec<f64>, DataFrameError> {
        let bytes = self.take(n.checked_mul(8).ok_or_else(self.overflow())?)?;
        Ok(bytes
            .chunks_exact(8)
            .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
            .collect())
    }

    fn rgba(&mut self, n: usize) -> Result<Vec<[u8; 4]>, DataFrameError> {
        let bytes = self.take(n.checked_mul(4).ok_or_else(self.overflow())?)?;
        Ok(bytes
            .chunks_exact(4)
            .map(|c| [c[0], c[1], c[2], c[3]])
            .collect())
    }

    fn bools(&mut self, n: usize) -> Result<Vec<bool>, DataFrameError> {
        Ok(self.take(n)?.iter().map(|&b| b != 0).collect())
    }

    fn u16s(&mut self, n: usize) -> Result<Vec<u16>, DataFrameError> {
        let bytes = self.take(n.checked_mul(2).ok_or_else(self.overflow())?)?;
        Ok(bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
            .collect())
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
        let DataFrame::View(v) = DataFrame::decode(&encode(12.0, 2, 1, &px)).unwrap() else {
            panic!("not a view frame");
        };
        assert_eq!(
            (v.workspace, v.run, v.sequence, v.ticks, v.width, v.height),
            (7, 2, 5, Some(12.0), 2, 1)
        );
        assert_eq!(v.rgba, px);
    }

    #[test]
    fn nan_ticks_mean_no_ticks() {
        let DataFrame::View(v) = DataFrame::decode(&encode(f64::NAN, 1, 1, &[0; 4])).unwrap()
        else {
            panic!("not a view frame");
        };
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

    /// Builds an agent frame the way the bridge's AgentState.encode does: a 2 x 1 world and
    /// two turtles.
    fn encode_agents() -> Vec<u8> {
        let mut b = vec![0x02];
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&4.0f64.to_le_bytes());
        b.push(AGENT_LAYOUT_VERSION);
        b.extend_from_slice(&2u32.to_le_bytes());
        for c in [0i32, 1, 0, 0] {
            b.extend_from_slice(&c.to_le_bytes());
        }
        b.extend_from_slice(&[0, 0, 0, 255, 1, 2, 3, 255]);
        b.extend_from_slice(&2u32.to_le_bytes());
        for column in [[0.0, 1.0], [0.5, -0.25], [0.0, 0.0], [90.0, 180.0]] {
            for v in column {
                b.extend_from_slice(&f64::to_le_bytes(v));
            }
        }
        b.extend_from_slice(&[215, 50, 41, 255, 10, 20, 30, 255]);
        for v in [1.0f64, 2.5] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [0u16, 3, 2, 1] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&[0, 1]);
        // One link, a directed one from turtle 0 to turtle 1.
        b.extend_from_slice(&1u32.to_le_bytes());
        for v in [0.0f64, 1.0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&[44, 209, 59, 255]);
        b.extend_from_slice(&0.25f64.to_le_bytes());
        for v in [0u16, 1] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&[0, 1]);
        b
    }

    #[test]
    fn decodes_an_agent_frame() {
        let DataFrame::Agents(a) = DataFrame::decode(&encode_agents()).unwrap() else {
            panic!("not an agent frame");
        };
        assert_eq!(
            (a.workspace, a.run, a.sequence, a.ticks),
            (3, 1, 0, Some(4.0))
        );
        assert_eq!(
            (a.min_pxcor, a.max_pxcor, a.min_pycor, a.max_pycor),
            (0, 1, 0, 0)
        );
        assert_eq!(a.patch_rgba, [0, 0, 0, 255, 1, 2, 3, 255]);
        let t = &a.turtles;
        assert_eq!(t.len(), 2);
        assert_eq!(t.who, [0.0, 1.0]);
        assert_eq!(t.xcor, [0.5, -0.25]);
        assert_eq!(t.heading, [90.0, 180.0]);
        assert_eq!(t.color, [[215, 50, 41, 255], [10, 20, 30, 255]]);
        assert_eq!(t.size, [1.0, 2.5]);
        assert_eq!(t.shape, [0, 3]);
        assert_eq!(t.breed, [2, 1]);
        assert_eq!(t.hidden, [false, true]);
        let l = &a.links;
        assert_eq!(l.len(), 1);
        assert_eq!((l.end1[0], l.end2[0]), (0.0, 1.0));
        assert_eq!(l.color, [[44, 209, 59, 255]]);
        assert_eq!(l.thickness, [0.25]);
        assert_eq!((l.shape[0], l.breed[0]), (0, 1));
        assert_eq!((l.hidden[0], l.directed[0]), (false, true));
    }

    #[test]
    fn rejects_another_layout_version() {
        let mut old = encode_agents();
        old[HEADER_BYTES] = 0;
        assert!(matches!(
            DataFrame::decode(&old),
            Err(DataFrameError::Agents(_))
        ));
    }

    #[test]
    fn rejects_truncated_or_padded_agent_frames() {
        let good = encode_agents();
        for cut in [HEADER_BYTES, HEADER_BYTES + 10, good.len() - 1] {
            assert!(
                matches!(
                    DataFrame::decode(&good[..cut]),
                    Err(DataFrameError::Agents(_))
                ),
                "cut at {cut}"
            );
        }
        let mut padded = good.clone();
        padded.push(0);
        assert!(matches!(
            DataFrame::decode(&padded),
            Err(DataFrameError::Agents(_))
        ));
        // A huge turtle count must fail cleanly, not allocate or panic.
        let mut huge = good;
        let at = HEADER_BYTES + 1 + 20 + 8;
        huge[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(DataFrame::decode(&huge).is_err());
    }
}
