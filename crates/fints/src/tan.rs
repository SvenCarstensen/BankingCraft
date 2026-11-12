//! TAN challenge helpers: HHD 1.3/1.4 flicker code (chipTAN optisch and USB),
//! and matrix codes (photoTAN / QR-TAN images).
//!
//! The flicker logic follows the reference implementation in hbci4java
//! (FlickerCode / FlickerRenderer), which in turn implements the ZKA
//! "HHD-Erweiterung für unidirektionale Kopplung" specification.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hhd {
    V13,
    V14,
}

#[derive(Debug, Clone, Default)]
struct De {
    data: Option<String>,
    forced_asc: bool,
}

impl De {
    fn parse<'a>(&mut self, s: &'a str, lde_len: usize) -> Result<&'a str, ()> {
        if s.is_empty() {
            return Ok(s);
        }
        let lde: u32 = s.get(..lde_len).ok_or(())?.parse().map_err(|_| ())?;
        let len = (lde & 0x3F) as usize;
        let rest = &s[lde_len..];
        self.data = Some(rest.get(..len).ok_or(())?.to_string());
        Ok(&rest[len..])
    }
    fn is_bcd(&self) -> bool {
        !self.forced_asc && self.data.as_deref().map(|d| d.chars().all(|c| c.is_ascii_digit())).unwrap_or(true)
    }
    fn render_data(&self) -> String {
        match &self.data {
            None => String::new(),
            Some(d) if self.is_bcd() => {
                if d.len() % 2 == 1 { format!("{d}F") } else { d.clone() }
            }
            Some(d) => d.chars().map(|c| format!("{:02X}", c as u32)).collect(),
        }
    }
    fn render_len(&self, v: Hhd) -> String {
        if self.data.is_none() {
            return String::new();
        }
        let len = self.render_data().len() / 2;
        if self.is_bcd() {
            format!("{:02X}", len)
        } else if v == Hhd::V14 {
            format!("{:02X}", len + (1 << 6))
        } else {
            format!("1{:X}", len)
        }
    }
}

#[derive(Debug, Clone)]
pub struct FlickerCode {
    version: Hhd,
    control_bytes: Vec<u8>,
    start: De,
    de: [De; 3],
}

impl FlickerCode {
    /// Parse the HHDuc data (as delivered in HITAN / the challenge text).
    pub fn parse(code: &str) -> Option<Self> {
        let cleaned = clean(code);
        // strict pass (LC must match the remaining length), then lenient like hbci4java
        for strict in [true, false] {
            let r = Self::parse_as(&cleaned, Hhd::V14, 2, strict)
                .or_else(|| Self::parse_as(&cleaned, Hhd::V14, 3, strict))
                .or_else(|| Self::parse_as(&cleaned, Hhd::V13, 2, strict));
            if r.is_some() {
                return r;
            }
        }
        None
    }

    fn parse_as(code: &str, version: Hhd, lde_len: usize, strict: bool) -> Option<Self> {
        let lc_len = if version == Hhd::V14 { 3 } else { 2 };
        let lc: usize = code.get(..lc_len)?.parse().ok()?;
        let mut s = &code[lc_len..];
        if strict && s.len() != lc {
            return None;
        }
        let lde = u8::from_str_radix(s.get(..2)?, 16).ok()?;
        s = &s[2..];
        let len = (lde & 0x3F) as usize;
        let mut control_bytes = Vec::new();
        let mut detected = Hhd::V13;
        if lde & 0x80 != 0 {
            detected = Hhd::V14;
            for _ in 0..10 {
                let cb = u8::from_str_radix(s.get(..2)?, 16).ok()?;
                control_bytes.push(cb);
                s = &s[2..];
                if cb & 0x80 == 0 {
                    break;
                }
            }
        }
        let start = De { data: Some(s.get(..len)?.to_string()), forced_asc: false };
        s = &s[len..];
        let mut de: [De; 3] = Default::default();
        for d in de.iter_mut() {
            s = d.parse(s, lde_len).ok()?;
        }
        if strict && !s.is_empty() {
            return None;
        }
        Some(Self { version: detected, control_bytes, start, de })
    }

    /// Render the transmission string: LC + payload + Luhn + XOR checksum (hex).
    /// This is also the HHDuc block sent to chipTAN USB readers.
    pub fn render(&self) -> String {
        let mut sb = String::new();
        let mut start_len = self.start.render_len(self.version);
        if self.version == Hhd::V14 && !self.control_bytes.is_empty() {
            let l = u32::from_str_radix(&start_len, 16).unwrap_or(0) + 0x80;
            start_len = format!("{:02X}", l);
        }
        sb.push_str(&start_len);
        for cb in &self.control_bytes {
            sb.push_str(&format!("{:02X}", cb));
        }
        sb.push_str(&self.start.render_data());
        for d in &self.de {
            sb.push_str(&d.render_len(self.version));
            sb.push_str(&d.render_data());
        }
        let len = (sb.len() + 2) / 2;
        let payload = format!("{:02X}{}", len, sb);
        let luhn = self.luhn();
        let xor = payload.chars().fold(0u32, |acc, c| acc ^ c.to_digit(16).unwrap_or(0));
        format!("{payload}{luhn:X}{xor:X}")
    }

    fn luhn(&self) -> u32 {
        let mut s = String::new();
        for cb in &self.control_bytes {
            s.push_str(&format!("{:02X}", cb));
        }
        s.push_str(&self.start.render_data());
        for d in &self.de {
            s.push_str(&d.render_data());
        }
        let digits: Vec<u32> = s.chars().map(|c| c.to_digit(16).unwrap_or(0)).collect();
        let mut sum = 0;
        for pair in digits.chunks(2) {
            sum += pair[0];
            if let Some(&b) = pair.get(1) {
                let d = 2 * b;
                sum += d / 10 + d % 10;
            }
        }
        (10 - sum % 10) % 10
    }

    /// Optical frames: each entry is 5 bits [clock, b1, b2, b3, b4].
    pub fn frames(&self) -> Vec<[bool; 5]> {
        let code = format!("0FFF{}", self.render());
        let chars: Vec<char> = code.chars().collect();
        let mut halfbytes = Vec::new();
        for pair in chars.chunks(2) {
            if pair.len() == 2 {
                halfbytes.push(pair[1].to_digit(16).unwrap_or(0));
                halfbytes.push(pair[0].to_digit(16).unwrap_or(0));
            }
        }
        let mut frames = Vec::with_capacity(halfbytes.len() * 2);
        for hb in halfbytes {
            for clock in [true, false] {
                frames.push([clock, hb & 1 != 0, hb & 2 != 0, hb & 4 != 0, hb & 8 != 0]);
            }
        }
        frames
    }

    /// Bytes for the chipTAN USB reader (hex string -> bytes).
    pub fn usb_payload(&self) -> Vec<u8> {
        let r = self.render();
        (0..r.len())
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(&r[i..i + 2], 16).ok())
            .collect()
    }
}

fn clean(code: &str) -> String {
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if let (Some(a), Some(b)) = (code.find("CHLGUC"), code.find("CHLGTEXT")) {
        if b > a {
            return format!("0{}", &code[a + 10..b]);
        }
    }
    code
}

/// photoTAN / QR-TAN: HHDuc = len(2) mime-type len(2) image.
pub fn parse_matrix_code(hhduc: &[u8]) -> Option<(String, Vec<u8>)> {
    if hhduc.len() < 4 {
        return None;
    }
    let mlen = u16::from_be_bytes([hhduc[0], hhduc[1]]) as usize;
    let mime = String::from_utf8_lossy(hhduc.get(2..2 + mlen)?).to_string();
    let p = 2 + mlen;
    let ilen = u16::from_be_bytes([*hhduc.get(p)?, *hhduc.get(p + 1)?]) as usize;
    let img = hhduc.get(p + 2..p + 2 + ilen)?.to_vec();
    if !mime.starts_with("image/") {
        return None;
    }
    Some((mime, img))
}
