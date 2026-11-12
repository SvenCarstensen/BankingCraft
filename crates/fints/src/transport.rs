//! Transports: HTTPS (PIN/TAN, base64 body) and plain TCP port 3000 (classic HBCI,
//! used by many banks for chip card access).

use crate::error::{Error, Result};
use base64::Engine;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub trait Transport: Send {
    fn send(&mut self, message: &[u8]) -> Result<Vec<u8>>;
}

pub struct HttpsTransport {
    url: String,
    agent: ureq::Agent,
}

impl HttpsTransport {
    pub fn new(url: &str) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(20))
            .timeout(Duration::from_secs(120))
            .build();
        Self { url: url.to_string(), agent }
    }
}

impl Transport for HttpsTransport {
    fn send(&mut self, message: &[u8]) -> Result<Vec<u8>> {
        log::debug!("FinTS >> {}", crate::syntax::latin1_to_string(message));
        let body = base64::engine::general_purpose::STANDARD.encode(message);
        let resp = self
            .agent
            .post(&self.url)
            .set("Content-Type", "text/plain")
            .send_string(&body)
            .map_err(|e| Error::Transport(e.to_string()))?;
        let text = resp.into_string().map_err(|e| Error::Transport(e.to_string()))?;
        let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let raw = base64::engine::general_purpose::STANDARD
            .decode(cleaned)
            .map_err(|e| Error::Transport(format!("invalid base64 response: {e}")))?;
        log::debug!("FinTS << {}", crate::syntax::latin1_to_string(&raw));
        Ok(raw)
    }
}

pub struct TcpTransport {
    addr: String,
}

impl TcpTransport {
    pub fn new(host: &str) -> Self {
        let addr = if host.contains(':') { host.to_string() } else { format!("{host}:3000") };
        Self { addr }
    }
}

impl Transport for TcpTransport {
    fn send(&mut self, message: &[u8]) -> Result<Vec<u8>> {
        let mut s = TcpStream::connect(&self.addr).map_err(|e| Error::Transport(e.to_string()))?;
        s.set_read_timeout(Some(Duration::from_secs(120))).ok();
        s.write_all(message).map_err(|e| Error::Transport(e.to_string()))?;
        // HNHBK:1:3+000000000123+... -> read the header first, then the announced size.
        // "HNHBK:1:3+" is 10 bytes, followed by the 12 digit total size.
        let mut head = [0u8; 22];
        s.read_exact(&mut head).map_err(|e| Error::Transport(e.to_string()))?;
        let size: usize = std::str::from_utf8(&head[10..22])
            .ok()
            .and_then(|t| t.parse().ok())
            .ok_or_else(|| Error::Transport("invalid message header".into()))?;
        let mut rest = vec![0u8; size.saturating_sub(head.len())];
        s.read_exact(&mut rest).map_err(|e| Error::Transport(e.to_string()))?;
        let mut all = head.to_vec();
        all.extend(rest);
        Ok(all)
    }
}
