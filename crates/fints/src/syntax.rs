//! FinTS 3.0 wire syntax: segments ('), data elements (+), group elements (:),
//! escape character (?) and binary data (@len@...).
//!
//! Text is ISO-8859-1 on the wire. Internally every value is kept as raw bytes and
//! converted with [`latin1_to_string`] / [`string_to_latin1`] at the edges.

use crate::error::{Error, Result};

const SEG_END: u8 = b'\'';
const DE_SEP: u8 = b'+';
const GE_SEP: u8 = b':';
const ESCAPE: u8 = b'?';
const BIN_MARK: u8 = b'@';
const MAX_SEGMENTS: usize = 8192;
const MAX_FIELDS: usize = 262_144;
const MAX_FIELDS_PER_SEGMENT: usize = 65_536;

fn check_message_size(data: &[u8]) -> Result<()> {
    if data.len() > crate::transport::MAX_MESSAGE_SIZE { return Err(Error::Syntax("FinTS-Nachricht zu groß".into())); }
    Ok(())
}

pub fn latin1_to_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

pub fn string_to_latin1(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| if (c as u32) < 256 { c as u8 } else { b'?' })
        .collect()
}

/// A single value inside a data element group. `None` means the field was empty.
pub type Field = Option<Vec<u8>>;

/// A parsed segment. `des[0]` is the segment header (code:number:version[:reference]).
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub code: String,
    pub number: u32,
    pub version: u32,
    pub reference: Option<u32>,
    pub des: Vec<Vec<Field>>,
}

impl Segment {
    /// Data element `i` (1-based, header excluded) as group fields.
    pub fn de(&self, i: usize) -> &[Field] {
        self.des.get(i).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Number of data elements excluding the header.
    pub fn de_count(&self) -> usize {
        self.des.len().saturating_sub(1)
    }

    /// Text of data element `i`, group field `j` (both 0-based for `j`, `i` 1-based).
    pub fn text(&self, i: usize, j: usize) -> Option<String> {
        self.de(i)
            .get(j)
            .and_then(|f| f.as_ref())
            .map(|b| latin1_to_string(b))
            .filter(|s| !s.is_empty())
    }

    pub fn text_or_empty(&self, i: usize, j: usize) -> String {
        self.text(i, j).unwrap_or_default()
    }

    pub fn bin(&self, i: usize, j: usize) -> Option<&[u8]> {
        self.de(i).get(j).and_then(|f| f.as_deref())
    }

    /// All fields of all data elements after the header, flattened (useful for
    /// parameter segments where DEGs are nested and repeated).
    pub fn flat_from(&self, i: usize) -> Vec<Field> {
        self.des.iter().skip(i).flat_map(|d| d.iter().cloned()).collect()
    }
}

/// Parse a raw FinTS message (or the plaintext contents of HNVSD) into segments.
pub fn parse_segments(data: &[u8]) -> Result<Vec<Segment>> {
    check_message_size(data)?;
    let mut segments = Vec::new();
    let mut fields = 0;
    let mut pos = 0usize;
    while pos < data.len() {
        // skip stray whitespace/newlines between segments some banks emit
        if matches!(data[pos], b'\r' | b'\n' | b' ') {
            pos += 1;
            continue;
        }
        let (des, next) = parse_one_segment(data, pos)?;
        fields += des.iter().map(Vec::len).sum::<usize>();
        if segments.len() >= MAX_SEGMENTS || fields > MAX_FIELDS { return Err(Error::Syntax("FinTS-Strukturgrenze überschritten".into())); }
        pos = next;
        if des.is_empty() || des[0].is_empty() {
            continue;
        }
        let header = &des[0];
        let get = |k: usize| -> String {
            header
                .get(k)
                .and_then(|f| f.as_ref())
                .map(|b| latin1_to_string(b))
                .unwrap_or_default()
        };
        let code = get(0);
        let number = get(1).parse().unwrap_or(0);
        let version = get(2).parse().unwrap_or(0);
        let reference = get(3).parse().ok();
        segments.push(Segment { code, number, version, reference, des });
    }
    Ok(segments)
}

/// Byte ranges `(start, end, code)` of the segments in `data` (end exclusive, including the
/// terminator). Needed to verify signatures over the exact bytes the bank signed.
pub fn segment_spans(data: &[u8]) -> Result<Vec<(usize, usize, String)>> {
    check_message_size(data)?;
    let mut out = Vec::new();
    let mut fields = 0;
    let mut pos = 0usize;
    while pos < data.len() {
        if matches!(data[pos], b'\r' | b'\n' | b' ') {
            pos += 1;
            continue;
        }
        let start = pos;
        let (des, next) = parse_one_segment(data, pos)?;
        fields += des.iter().map(Vec::len).sum::<usize>();
        if out.len() >= MAX_SEGMENTS || fields > MAX_FIELDS { return Err(Error::Syntax("FinTS-Strukturgrenze überschritten".into())); }
        pos = next;
        let code = des
            .first()
            .and_then(|h| h.first())
            .and_then(|f| f.as_ref())
            .map(|b| latin1_to_string(b))
            .unwrap_or_default();
        if !code.is_empty() {
            out.push((start, next, code));
        }
    }
    Ok(out)
}

#[allow(unused_assignments)]
fn parse_one_segment(data: &[u8], mut pos: usize) -> Result<(Vec<Vec<Field>>, usize)> {
    let mut des: Vec<Vec<Field>> = Vec::new();
    let mut group: Vec<Field> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut cur_set = false;
    let mut field_count = 0usize;

    macro_rules! push_field {
        () => {{
            field_count += 1;
            if field_count > MAX_FIELDS_PER_SEGMENT { return Err(Error::Syntax("Zu viele Felder im FinTS-Segment".into())); }
            group.push(if cur_set || !cur.is_empty() { Some(std::mem::take(&mut cur)) } else { None });
            cur_set = false;
        }};
    }

    while pos < data.len() {
        let b = data[pos];
        match b {
            ESCAPE => {
                if pos + 1 >= data.len() {
                    return Err(Error::Syntax("dangling escape character".into()));
                }
                cur.push(data[pos + 1]);
                pos += 2;
            }
            BIN_MARK if cur.is_empty() && !cur_set => {
                // @len@binary
                let rest = &data[pos + 1..];
                let end = rest
                    .iter()
                    .position(|&c| c == BIN_MARK)
                    .ok_or_else(|| Error::Syntax("unterminated binary length".into()))?;
                let len: usize = latin1_to_string(&rest[..end])
                    .parse()
                    .map_err(|_| Error::Syntax("invalid binary length".into()))?;
                let start = pos + 1 + end + 1;
                if len > data.len().saturating_sub(start) {
                    return Err(Error::Syntax("binary data exceeds message".into()));
                }
                cur.extend_from_slice(&data[start..start + len]);
                cur_set = true;
                pos = start + len;
            }
            GE_SEP => {
                push_field!();
                pos += 1;
            }
            DE_SEP => {
                push_field!();
                des.push(std::mem::take(&mut group));
                pos += 1;
            }
            SEG_END => {
                push_field!();
                des.push(std::mem::take(&mut group));
                pos += 1;
                return Ok((des, pos));
            }
            _ => {
                cur.push(b);
                pos += 1;
            }
        }
    }
    // tolerate a missing final terminator
    if !cur.is_empty() || !group.is_empty() || cur_set {
        push_field!();
        des.push(group);
    }
    Ok((des, pos))
}

/// One value to be serialized.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Text(String),
    Bin(Vec<u8>),
    Empty,
}

impl From<&str> for Val {
    fn from(s: &str) -> Self {
        if s.is_empty() { Val::Empty } else { Val::Text(s.to_string()) }
    }
}
impl From<String> for Val {
    fn from(s: String) -> Self {
        if s.is_empty() { Val::Empty } else { Val::Text(s) }
    }
}
impl From<&String> for Val {
    fn from(s: &String) -> Self {
        Val::from(s.as_str())
    }
}
impl From<u32> for Val {
    fn from(n: u32) -> Self {
        Val::Text(n.to_string())
    }
}
impl<T: Into<Val>> From<Option<T>> for Val {
    fn from(o: Option<T>) -> Self {
        o.map(Into::into).unwrap_or(Val::Empty)
    }
}

/// Builder for outgoing segments. Segment numbers are assigned by the message builder.
#[derive(Debug, Clone)]
pub struct SegmentBuilder {
    pub code: String,
    pub version: u32,
    pub number: u32,
    pub reference: Option<u32>,
    des: Vec<Vec<Val>>,
}

impl SegmentBuilder {
    pub fn new(code: &str, version: u32) -> Self {
        Self { code: code.into(), version, number: 0, reference: None, des: Vec::new() }
    }

    /// Append a data element made of a single value.
    pub fn de(mut self, v: impl Into<Val>) -> Self {
        self.des.push(vec![v.into()]);
        self
    }

    /// Append a data element group.
    pub fn deg(mut self, vals: Vec<Val>) -> Self {
        self.des.push(vals);
        self
    }

    pub fn bin(mut self, data: Vec<u8>) -> Self {
        self.des.push(vec![Val::Bin(data)]);
        self
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(128);
        out.extend_from_slice(self.code.as_bytes());
        out.push(GE_SEP);
        out.extend_from_slice(self.number.to_string().as_bytes());
        out.push(GE_SEP);
        out.extend_from_slice(self.version.to_string().as_bytes());
        if let Some(r) = self.reference {
            out.push(GE_SEP);
            out.extend_from_slice(r.to_string().as_bytes());
        }
        // trim trailing empty data elements and trailing empty group fields
        let mut des: Vec<Vec<Val>> = self
            .des
            .iter()
            .map(|g| {
                let mut g = g.clone();
                while matches!(g.last(), Some(Val::Empty)) {
                    g.pop();
                }
                g
            })
            .collect();
        while matches!(des.last(), Some(g) if g.is_empty()) {
            des.pop();
        }
        for g in &des {
            out.push(DE_SEP);
            for (k, v) in g.iter().enumerate() {
                if k > 0 {
                    out.push(GE_SEP);
                }
                encode_val(v, &mut out);
            }
        }
        out.push(SEG_END);
        out
    }
}

fn encode_val(v: &Val, out: &mut Vec<u8>) {
    match v {
        Val::Empty => {}
        Val::Text(s) => {
            for b in string_to_latin1(s) {
                if matches!(b, SEG_END | DE_SEP | GE_SEP | ESCAPE | BIN_MARK) {
                    out.push(ESCAPE);
                }
                out.push(b);
            }
        }
        Val::Bin(data) => {
            out.push(BIN_MARK);
            out.extend_from_slice(data.len().to_string().as_bytes());
            out.push(BIN_MARK);
            out.extend_from_slice(data);
        }
    }
}

/// Helpers for frequently used value formats.
pub fn yes_no(b: bool) -> Val {
    Val::Text(if b { "J" } else { "N" }.into())
}

pub fn fints_date(d: chrono::NaiveDate) -> Val {
    Val::Text(d.format("%Y%m%d").to_string())
}

pub fn parse_fints_date(s: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s, "%Y%m%d").ok()
}

/// FinTS amounts use ',' as decimal separator.
pub fn parse_amount(s: &str) -> Option<rust_decimal::Decimal> {
    s.replace(',', ".").parse().ok()
}

pub fn format_amount(d: rust_decimal::Decimal) -> String {
    format!("{:.2}", d).replace('.', ",")
}
