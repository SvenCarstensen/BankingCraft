//! Transports: HTTPS (PIN/TAN, base64 body) and plain TCP port 3000 (classic HBCI,
//! used by many banks for chip card access).

use crate::error::{Error, Result};
use base64::Engine;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;
use zeroize::Zeroizing;

pub const MAX_MESSAGE_SIZE: usize = 32 * 1024 * 1024;

pub fn validate_https_url(value: &str) -> Result<()> {
    let url = url::Url::parse(value).map_err(|_| Error::Security("Ungültige FinTS-Adresse".into()))?;
    if url.scheme() != "https" || url.host_str().is_none() || !url.username().is_empty()
        || url.password().is_some() || url.fragment().is_some() || value.contains('\\') || value.chars().any(char::is_control) {
        return Err(Error::Security("FinTS benötigt eine eindeutige HTTPS-Adresse ohne Benutzerinfo oder Fragment".into()));
    }
    Ok(())
}

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
            .https_only(true)
            .redirects(0)
            .timeout_connect(Duration::from_secs(20))
            .timeout(Duration::from_secs(120))
            .build();
        Self { url: url.to_string(), agent }
    }
}

impl Transport for HttpsTransport {
    fn send(&mut self, message: &[u8]) -> Result<Vec<u8>> {
        validate_https_url(&self.url)?;
        log::debug!("FinTS >> {} bytes", message.len());
        let body = Zeroizing::new(base64::engine::general_purpose::STANDARD.encode(message));
        let resp = self
            .agent
            .post(&self.url)
            .set("Content-Type", "text/plain")
            .send_string(&body)
            .map_err(|_| Error::Transport("HTTPS-Anfrage an die Bank fehlgeschlagen".into()))?;
        let mut text = String::new();
        let limit = MAX_MESSAGE_SIZE * 2;
        resp.into_reader().take((limit + 1) as u64).read_to_string(&mut text)
            .map_err(|e| Error::Transport(e.to_string()))?;
        if text.len() > limit {
            return Err(Error::Transport("Antwort der Bank überschreitet die Größenbegrenzung".into()));
        }
        let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let raw = base64::engine::general_purpose::STANDARD
            .decode(cleaned)
            .map_err(|e| Error::Transport(format!("invalid base64 response: {e}")))?;
        if raw.len() > MAX_MESSAGE_SIZE {
            return Err(Error::Transport("Antwort der Bank überschreitet die Größenbegrenzung".into()));
        }
        log::debug!("FinTS << {} bytes", raw.len());
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
        let addresses = self.addr.to_socket_addrs().map_err(|e| Error::Transport(e.to_string()))?;
        let mut connection = Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Keine Serveradresse gefunden"));
        for address in addresses {
            connection = TcpStream::connect_timeout(&address, Duration::from_secs(20));
            if connection.is_ok() { break; }
        }
        let mut s = connection.map_err(|e| Error::Transport(e.to_string()))?;
        s.set_read_timeout(Some(Duration::from_secs(120))).map_err(|e| Error::Transport(e.to_string()))?;
        s.set_write_timeout(Some(Duration::from_secs(120))).map_err(|e| Error::Transport(e.to_string()))?;
        s.write_all(message).map_err(|e| Error::Transport(e.to_string()))?;
        // HNHBK:1:3+000000000123+... -> read the header first, then the announced size.
        // "HNHBK:1:3+" is 10 bytes, followed by the 12 digit total size.
        let mut head = [0u8; 22];
        s.read_exact(&mut head).map_err(|e| Error::Transport(e.to_string()))?;
        if !head.starts_with(b"HNHBK:1:3+") {
            return Err(Error::Transport("invalid message header".into()));
        }
        let size: usize = std::str::from_utf8(&head[10..22])
            .ok()
            .and_then(|t| t.parse().ok())
            .ok_or_else(|| Error::Transport("invalid message header".into()))?;
        if !(head.len()..=MAX_MESSAGE_SIZE).contains(&size) {
            return Err(Error::Transport("Ungültige oder zu große FinTS-Nachricht".into()));
        }
        let mut rest = vec![0u8; size - head.len()];
        s.read_exact(&mut rest).map_err(|e| Error::Transport(e.to_string()))?;
        let mut all = head.to_vec();
        all.extend(rest);
        Ok(all)
    }
}
