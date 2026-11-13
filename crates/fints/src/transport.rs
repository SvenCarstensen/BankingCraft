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
    let url = url::Url::parse(value).map_err(|_| Error::Security(i18n::tr("Ungültige FinTS-Adresse").into()))?;
    if url.scheme() != "https" || url.host_str().is_none() || !url.username().is_empty()
        || url.password().is_some() || url.fragment().is_some() || value.contains('\\') || value.chars().any(char::is_control) {
        return Err(Error::Security(i18n::tr("FinTS benötigt eine eindeutige HTTPS-Adresse ohne Benutzerinfo oder Fragment").into()));
    }
    Ok(())
}

pub trait Transport: Send {
    fn send(&mut self, message: &[u8]) -> Result<Vec<u8>>;
    /// End the transport session when a FinTS dialog ends or is reset.
    fn reset(&mut self) {}
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
            .map_err(|_| Error::Transport(i18n::tr("HTTPS-Anfrage an die Bank fehlgeschlagen").into()))?;
        let mut body = Vec::new();
        let limit = MAX_MESSAGE_SIZE * 2;
        resp.into_reader().take((limit + 1) as u64).read_to_end(&mut body)
            .map_err(|e| Error::Transport(e.to_string()))?;
        let raw = decode_https_body(&body)?;
        log::debug!("FinTS << {} bytes", raw.len());
        Ok(raw)
    }
}

/// AqBanking accepts both base64 and raw FinTS HTTP responses. Keep raw binary
/// fields byte-exact; envelope/authentication checks still run in open_message.
fn decode_https_body(body: &[u8]) -> Result<Vec<u8>> {
    if body.len() > MAX_MESSAGE_SIZE * 2 {
        return Err(Error::Transport(i18n::tr("Antwort der Bank überschreitet die Größenbegrenzung").into()));
    }
    let raw = if body.starts_with(b"HNHBK:") {
        body.to_vec()
    } else {
        let cleaned: Vec<u8> = body.iter().copied().filter(|c| !c.is_ascii_whitespace()).collect();
        base64::engine::general_purpose::STANDARD.decode(cleaned)
            .map_err(|e| Error::Transport(i18n::tr_format!("Ungültige Base64-Antwort: {e}")))?
    };
    if raw.len() > MAX_MESSAGE_SIZE {
        return Err(Error::Transport(i18n::tr("Antwort der Bank überschreitet die Größenbegrenzung").into()));
    }
    if !raw.starts_with(b"HNHBK:") {
        return Err(Error::Transport(i18n::tr("Ungültiger Nachrichtenkopf").into()));
    }
    Ok(raw)
}

#[cfg(test)]
mod https_body_tests {
    use super::*;
    #[test]
    fn preserves_binary_and_latin1_in_raw_and_base64_responses() {
        let raw = b"HNHBK:1:3+@8@\xe4\xff \n\r\t?'";
        assert_eq!(decode_https_body(raw).unwrap(), raw);
        let base64 = base64::engine::general_purpose::STANDARD.encode(raw);
        assert_eq!(decode_https_body(format!("\r\n{base64}\r\n").as_bytes()).unwrap(), raw);
        for data in [b"<html>HNHBK:fake</html>".as_slice(), b"aGVsbG8=", b"", b" \nHNHBK:fake"] {
            assert!(decode_https_body(data).is_err());
        }
    }
}

pub struct TcpTransport {
    addr: String,
    stream: Option<TcpStream>,
    failed: bool,
}

impl TcpTransport {
    pub fn new(host: &str) -> Self {
        let addr = if host.contains(':') { host.to_string() } else { format!("{host}:3000") };
        Self { addr, stream: None, failed: false }
    }

    fn connect(&self) -> Result<TcpStream> {
        let addresses = self.addr.to_socket_addrs().map_err(|e| Error::Transport(e.to_string()))?;
        let mut connection = Err(std::io::Error::new(std::io::ErrorKind::NotFound, i18n::tr("Keine Serveradresse gefunden")));
        for address in addresses {
            connection = TcpStream::connect_timeout(&address, Duration::from_secs(20));
            if connection.is_ok() { break; }
        }
        let s = connection.map_err(|e| Error::Transport(e.to_string()))?;
        s.set_read_timeout(Some(Duration::from_secs(120))).map_err(|e| Error::Transport(e.to_string()))?;
        s.set_write_timeout(Some(Duration::from_secs(120))).map_err(|e| Error::Transport(e.to_string()))?;
        Ok(s)
    }

    fn exchange(&mut self, message: &[u8]) -> Result<Vec<u8>> {
        if self.stream.is_none() { self.stream = Some(self.connect()?); }
        let s = self.stream.as_mut().expect("connected above");
        s.write_all(message).map_err(|e| Error::Transport(e.to_string()))?;
        // HNHBK:1:3+000000000123+... -> read the header first, then the announced size.
        // "HNHBK:1:3+" is 10 bytes, followed by the 12 digit total size.
        let mut head = [0u8; 22];
        s.read_exact(&mut head).map_err(|e| Error::Transport(e.to_string()))?;
        if !head.starts_with(b"HNHBK:1:3+") {
            return Err(Error::Transport(i18n::tr("Ungültiger Nachrichtenkopf").into()));
        }
        let size: usize = std::str::from_utf8(&head[10..22])
            .ok()
            .and_then(|t| t.parse().ok())
            .ok_or_else(|| Error::Transport(i18n::tr("Ungültiger Nachrichtenkopf").into()))?;
        if !(head.len()..=MAX_MESSAGE_SIZE).contains(&size) {
            return Err(Error::Transport(i18n::tr("Ungültige oder zu große FinTS-Nachricht").into()));
        }
        let mut rest = vec![0u8; size - head.len()];
        s.read_exact(&mut rest).map_err(|e| Error::Transport(e.to_string()))?;
        let mut all = head.to_vec();
        all.extend(rest);
        Ok(all)
    }
}

impl Transport for TcpTransport {
    fn send(&mut self, message: &[u8]) -> Result<Vec<u8>> {
        if self.failed {
            return Err(Error::Transport(i18n::tr("Die TCP-Verbindung ist nach einem Fehler geschlossen. Bitte den Bankabruf neu starten.").into()));
        }
        // HBCI gateways bind a dialog to its socket. Reuse it for every message;
        // reconnect only at an explicit dialog boundary, never replay a request.
        let result = self.exchange(message);
        if result.is_err() {
            self.stream = None;
            self.failed = true;
        }
        result
    }

    fn reset(&mut self) {
        self.stream = None;
        self.failed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn packet(dialog: &str, number: u32) -> Vec<u8> {
        crate::security::build_message(&crate::security::Envelope {
            dialog_id: dialog, message_number: number, blz: "12345678", user_id: "anonymous", system_id: "0",
        }, crate::security::Auth::None, vec![]).unwrap()
    }

    #[test]
    fn tcp_keeps_one_connection_for_all_messages_in_a_dialog() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let first = packet("0", 1);
        let second = packet("TEST", 2);
        let requests = [first.clone(), second.clone()];
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            for expected in requests {
                let mut received = vec![0; expected.len()];
                stream.read_exact(&mut received).expect("the second message must use the existing connection");
                assert_eq!(received, expected);
                // Response headers and bodies may span several socket reads.
                for chunk in expected.chunks(7) { stream.write_all(chunk).unwrap(); }
            }
        });
        let mut transport = TcpTransport::new(&address);
        assert_eq!(transport.send(&first).unwrap(), first);
        assert_eq!(transport.send(&second).unwrap(), second);
        server.join().unwrap();
    }

    #[test]
    fn tcp_reset_closes_the_old_dialog_and_opens_a_new_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut transport = TcpTransport::new(&listener.local_addr().unwrap().to_string());
        let request = packet("0", 1);
        let expected = request.clone();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut received = vec![0; expected.len()];
                stream.read_exact(&mut received).unwrap();
                assert_eq!(received, expected);
                stream.write_all(&expected).unwrap();
                assert_eq!(stream.read(&mut [0u8; 1]).unwrap(), 0, "dialog reset closes the socket");
            }
        });
        for _ in 0..2 {
            assert_eq!(transport.send(&request).unwrap(), request);
            transport.reset();
        }
        server.join().unwrap();
    }

    #[test]
    fn tcp_does_not_replay_or_reconnect_after_an_uncertain_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut transport = TcpTransport::new(&listener.local_addr().unwrap().to_string());
        let request = packet("TEST", 2);
        let expected = request.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut received = vec![0; expected.len()];
            stream.read_exact(&mut received).unwrap();
            assert_eq!(received, expected);
            // The request was received, but the response is truncated. Its
            // execution status is unknown: automatic resubmission is forbidden.
            stream.write_all(b"HNHBK:1:3+").unwrap();
            drop(stream);
            listener
        });
        assert!(transport.send(&request).is_err());
        let listener = server.join().unwrap();
        assert!(transport.send(&request).is_err());
        listener.set_nonblocking(true).unwrap();
        assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock,
            "no second connection or request without an explicit dialog reset");
    }
}
