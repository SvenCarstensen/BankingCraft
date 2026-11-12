//! PC/SC access for Kontor.
//!
//! * HBCI chip card (DDV-0 / DDV-1, SECCOS) implementing [`fints::security::DdvCard`]
//! * chipTAN USB (HHDuc via `FEATURE_MCT_READER_DIRECT` or the `FF 91` pseudo APDU)
//! * Secure PIN entry on the reader's pin pad (`FEATURE_VERIFY_PIN_DIRECT`, class 2/3)
//!
//! REINER SCT cyberJack readers (incl. the secoder models) are detected by name and
//! preferred automatically. The APDU sequences mirror hbci4java's smartcardio services.

use fints::security::{DdvBankData, DdvCard, DdvKey};
use pcsc::{Card, Context, Protocol, Protocols, ReaderState, Scope, ShareMode, State};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::time::Duration;

pub type Result<T> = std::result::Result<T, fints::Error>;

fn err(msg: impl Into<String>) -> fints::Error {
    fints::Error::Security(msg.into())
}

/// PC/SC part 10 feature tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Feature {
    VerifyPinStart = 0x01,
    VerifyPinFinish = 0x02,
    ModifyPinStart = 0x03,
    ModifyPinFinish = 0x04,
    GetKeyPressed = 0x05,
    VerifyPinDirect = 0x06,
    ModifyPinDirect = 0x07,
    MctReaderDirect = 0x08,
    MctUniversal = 0x09,
    IfdPinProperties = 0x0a,
    Abort = 0x0b,
    SetSpeMessage = 0x0c,
    VerifyPinDirectAppId = 0x0d,
    ModifyPinDirectAppId = 0x0e,
    WriteDisplay = 0x0f,
    GetKey = 0x10,
    IfdDisplayProperties = 0x11,
    GetTlvProperties = 0x12,
    CcidEscCommand = 0x13,
    ExecutePace = 0x20,
}

impl Feature {
    fn from_tag(t: u8) -> Option<Self> {
        use Feature::*;
        Some(match t {
            0x01 => VerifyPinStart,
            0x02 => VerifyPinFinish,
            0x03 => ModifyPinStart,
            0x04 => ModifyPinFinish,
            0x05 => GetKeyPressed,
            0x06 => VerifyPinDirect,
            0x07 => ModifyPinDirect,
            0x08 => MctReaderDirect,
            0x09 => MctUniversal,
            0x0a => IfdPinProperties,
            0x0b => Abort,
            0x0c => SetSpeMessage,
            0x0d => VerifyPinDirectAppId,
            0x0e => ModifyPinDirectAppId,
            0x0f => WriteDisplay,
            0x10 => GetKey,
            0x11 => IfdDisplayProperties,
            0x12 => GetTlvProperties,
            0x13 => CcidEscCommand,
            0x20 => ExecutePace,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        use Feature::*;
        match self {
            VerifyPinDirect => "Sichere PIN-Eingabe (VERIFY_PIN_DIRECT)",
            ModifyPinDirect => "PIN-Änderung am Leser (MODIFY_PIN_DIRECT)",
            MctReaderDirect => "chipTAN USB / MKT (MCT_READER_DIRECT)",
            MctUniversal => "MKT universal (MCT_UNIVERSAL)",
            IfdPinProperties => "PIN-Pad-Eigenschaften",
            WriteDisplay => "Display beschreibbar",
            IfdDisplayProperties => "Display-Eigenschaften",
            GetTlvProperties => "TLV-Eigenschaften",
            CcidEscCommand => "CCID-Escape",
            ExecutePace => "PACE",
            VerifyPinStart | VerifyPinFinish => "PIN-Prüfung (zweistufig)",
            ModifyPinStart | ModifyPinFinish => "PIN-Änderung (zweistufig)",
            GetKeyPressed | GetKey => "Tastenabfrage",
            Abort => "Abbruch",
            SetSpeMessage => "SPE-Meldungen",
            VerifyPinDirectAppId | ModifyPinDirectAppId => "PIN mit App-ID",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReaderInfo {
    pub name: String,
    pub card_present: bool,
    pub cyberjack: bool,
    pub secoder: bool,
}

pub fn is_cyberjack(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("cyberjack") || n.contains("reiner sct") || n.contains("reiner-sct")
}

pub fn list_readers() -> Result<Vec<ReaderInfo>> {
    let ctx = Context::establish(Scope::User).map_err(|e| err(format!("PC/SC nicht verfügbar: {e}")))?;
    let names = match ctx.list_readers_owned() {
        Ok(n) => n,
        Err(pcsc::Error::NoReadersAvailable) => return Ok(vec![]),
        Err(e) => return Err(err(e.to_string())),
    };
    let mut states: Vec<ReaderState> = names.iter().map(|n| ReaderState::new(n.clone(), State::UNAWARE)).collect();
    ctx.get_status_change(Duration::from_millis(200), &mut states).ok();
    let mut out: Vec<ReaderInfo> = names
        .iter()
        .zip(states.iter())
        .map(|(n, s)| {
            let name = n.to_string_lossy().to_string();
            ReaderInfo {
                cyberjack: is_cyberjack(&name),
                secoder: name.to_lowercase().contains("secoder"),
                card_present: s.event_state().contains(State::PRESENT),
                name,
            }
        })
        .collect();
    // cyberJack first, then readers with a card
    out.sort_by_key(|r| (!r.cyberjack, !r.card_present));
    Ok(out)
}

/// An open connection to a card in a reader.
pub struct Session {
    _ctx: Context,
    card: Card,
    pub reader: String,
    pub protocol: Option<Protocol>,
    pub features: BTreeMap<Feature, u32>,
    pub atr: Vec<u8>,
}

impl Session {
    /// Connect to `reader` (or the preferred reader), waiting up to `wait` for a card.
    pub fn open(reader: Option<&str>, wait: Duration) -> Result<Self> {
        let ctx = Context::establish(Scope::User).map_err(|e| err(format!("PC/SC nicht verfügbar: {e}")))?;
        let name = match reader {
            Some(r) if !r.is_empty() => r.to_string(),
            _ => list_readers()?
                .into_iter()
                .next()
                .map(|r| r.name)
                .ok_or_else(|| err("Kein Kartenleser gefunden"))?,
        };
        let cname = CString::new(name.clone()).map_err(|_| err("ungültiger Readername"))?;
        let mut st = [ReaderState::new(cname.clone(), State::UNAWARE)];
        ctx.get_status_change(Duration::from_millis(200), &mut st).ok();
        if !st[0].event_state().contains(State::PRESENT) {
            log::info!("waiting for card in {name}");
            st[0].sync_current_state();
            let deadline = std::time::Instant::now() + wait;
            loop {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    return Err(err(format!("Keine Chipkarte in \"{name}\" gefunden")));
                }
                match ctx.get_status_change(left.min(Duration::from_secs(1)), &mut st) {
                    Ok(()) | Err(pcsc::Error::Timeout) => {}
                    Err(e) => return Err(err(e.to_string())),
                }
                if st[0].event_state().contains(State::PRESENT) {
                    break;
                }
                st[0].sync_current_state();
            }
        }
        let card = ctx
            .connect(&cname, ShareMode::Shared, Protocols::ANY)
            .map_err(|e| err(format!("Verbindung zur Karte fehlgeschlagen: {e}")))?;
        let status = card.status2_owned().map_err(|e| err(e.to_string()))?;
        let mut s = Self {
            protocol: status.protocol2(),
            atr: status.atr().to_vec(),
            _ctx: ctx,
            card,
            reader: name,
            features: BTreeMap::new(),
        };
        s.query_features();
        Ok(s)
    }

    fn query_features(&mut self) {
        let mut buf = [0u8; 256];
        match self.card.control(pcsc::ctl_code(3400), &[], &mut buf) {
            Ok(resp) => {
                for chunk in resp.chunks(6) {
                    if chunk.len() == 6 {
                        let ioctl = u32::from_be_bytes([chunk[2], chunk[3], chunk[4], chunk[5]]);
                        if let Some(f) = Feature::from_tag(chunk[0]) {
                            self.features.insert(f, ioctl);
                        }
                    }
                }
            }
            Err(e) => log::warn!("unable to query reader features: {e}"),
        }
    }

    pub fn has(&self, f: Feature) -> bool {
        self.features.contains_key(&f)
    }

    /// Raw transmit; returns (data, sw).
    pub fn transmit_raw(&self, apdu: &[u8]) -> Result<(Vec<u8>, u16)> {
        let mut buf = vec![0u8; pcsc::MAX_BUFFER_SIZE_EXTENDED];
        let r = self.card.transmit(apdu, &mut buf).map_err(|e| err(format!("Kartenzugriff: {e}")))?;
        if r.len() < 2 {
            return Err(err("Antwort der Karte zu kurz"));
        }
        let sw = u16::from_be_bytes([r[r.len() - 2], r[r.len() - 1]]);
        Ok((r[..r.len() - 2].to_vec(), sw))
    }

    /// Transmit with T=0 handling (61xx GET RESPONSE, 6Cxx resend with correct Le).
    pub fn transmit(&self, apdu: &[u8]) -> Result<(Vec<u8>, u16)> {
        let (mut data, mut sw) = self.transmit_raw(apdu)?;
        if sw >> 8 == 0x6C {
            let mut a = apdu.to_vec();
            if let Some(last) = a.last_mut() {
                *last = (sw & 0xFF) as u8;
            }
            let r = self.transmit_raw(&a)?;
            data = r.0;
            sw = r.1;
        }
        while sw >> 8 == 0x61 {
            let r = self.transmit_raw(&[0x00, 0xC0, 0x00, 0x00, (sw & 0xFF) as u8])?;
            data.extend(r.0);
            sw = r.1;
        }
        Ok((data, sw))
    }

    pub fn control(&self, feature: Feature, data: &[u8]) -> Result<Vec<u8>> {
        let code = *self.features.get(&feature).ok_or_else(|| err(format!("Leser unterstützt {} nicht", feature.label())))?;
        let mut buf = vec![0u8; 1024];
        let r = self
            .card
            .control(code as _, data, &mut buf)
            .map_err(|e| err(format!("Leser-Steuerbefehl fehlgeschlagen: {e}")))?;
        Ok(r.to_vec())
    }
}

fn check(sw: u16, ok_sw1: &[u8]) -> Result<()> {
    if ok_sw1.contains(&((sw >> 8) as u8)) {
        return Ok(());
    }
    Err(err(format!("Fehler {:04X}: {}", sw, status_text(sw))))
}

pub fn status_text(sw: u16) -> &'static str {
    match sw {
        0x6281 => "Die zurückgegebenen Daten können fehlerhaft sein",
        0x6282 => "Dateiende vorzeitig erreicht",
        0x6283 => "Die ausgewählte Datei ist gesperrt",
        0x6700 => "Länge (Lc oder Le) falsch",
        0x6982 => "Sicherheitszustand nicht erfüllt (PIN nicht geprüft?)",
        0x6983 => "Authentisierungsmethode ist gesperrt",
        0x6984 => "Referenzierte Daten sind gesperrt",
        0x6985 => "Nutzungsbedingungen sind nicht erfüllt",
        0x6A82 => "Datei nicht gefunden – keine HBCI-Chipkarte?",
        0x6A83 => "Record nicht gefunden",
        0x6A86 => "Inkorrekte Parameter P1/P2",
        0x6A88 => "Referenzierte Daten nicht gefunden",
        0x6D00 => "Kommando wird nicht unterstützt",
        0x6E00 => "Kommandoklasse wird nicht unterstützt",
        0x63C0 => "PIN falsch, kein Versuch mehr",
        0x63C1 => "PIN falsch, noch 1 Versuch",
        0x63C2 => "PIN falsch, noch 2 Versuche",
        0x6400 => "PIN-Eingabe: Zeitüberschreitung",
        0x6401 => "PIN-Eingabe abgebrochen",
        _ => "Kartenfehler",
    }
}

/// Build a short APDU (ISO 7816-4 case 1-4).
fn apdu(cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8], le: Option<u16>) -> Vec<u8> {
    let mut a = vec![cla, ins, p1, p2];
    if !data.is_empty() {
        a.push(data.len() as u8);
        a.extend_from_slice(data);
    }
    if let Some(le) = le {
        a.push(if le >= 256 { 0 } else { le as u8 });
    }
    a
}

// ---------------------------------------------------------------------------- DDV

const EF_ID: u8 = 0x19;
const EF_BNK: u8 = 0x1A;
const EF_MAC: u8 = 0x1B;
const EF_SEQ: u8 = 0x1C;

/// Supplies a PIN for readers without pin pad.
pub type SoftPinProvider = Box<dyn Fn() -> Option<String> + Send>;

pub struct DdvChipCard {
    pub session: Session,
    ddv1: bool,
    pin_ok: bool,
    soft_pin: Option<SoftPinProvider>,
    cid: Option<Vec<u8>>,
    /// Called with `true` before the reader waits for PIN entry on its pin pad and
    /// with `false` afterwards, so the UI can show a hint.
    pub pinpad_notify: Option<Box<dyn Fn(bool) + Send>>,
}

impl DdvChipCard {
    /// Open the HBCI application on the card. DDV-0 cards use T=0, DDV-1 cards T=1
    /// (same rule as hbci4java).
    pub fn open(reader: Option<&str>, soft_pin: Option<SoftPinProvider>) -> Result<Self> {
        let session = Session::open(reader, Duration::from_secs(60))?;
        let ddv1 = session.protocol == Some(Protocol::T1);
        let aid: [u8; 9] = [0xD2, 0x76, 0x00, 0x00, 0x25, 0x48, 0x42, if ddv1 { 0x02 } else { 0x01 }, 0x00];
        let me = Self { session, ddv1, pin_ok: false, soft_pin, cid: None, pinpad_notify: None };
        me.send(&apdu(0x00, 0xA4, 0x04, 0x0C, &aid, None))
            .map_err(|e| err(format!("Keine HBCI-Chipkarte (DDV) erkannt: {e}")))?;
        Ok(me)
    }

    pub fn kind(&self) -> &'static str {
        if self.ddv1 { "DDV-1 (SECCOS, T=1)" } else { "DDV-0 (T=0)" }
    }

    fn send(&self, a: &[u8]) -> Result<()> {
        let (_, sw) = self.session.transmit(a)?;
        check(sw, &[0x90, 0x61])
    }

    fn receive(&self, a: &[u8]) -> Result<Vec<u8>> {
        let (d, sw) = self.session.transmit(a)?;
        check(sw, &[0x90])?;
        Ok(d)
    }

    fn read_record(&self, sfi: u8, idx: u8) -> Result<Vec<u8>> {
        self.receive(&apdu(0x00, 0xB2, idx + 1, (sfi << 3) | 0x04, &[], Some(256)))
    }

    fn update_record(&self, sfi: u8, idx: u8, data: &[u8]) -> Result<()> {
        self.send(&apdu(0x00, 0xDC, idx + 1, (sfi << 3) | 0x04, data, None))
    }

    fn select_sub_file(&self, id: u16) -> Result<()> {
        self.send(&apdu(0x00, 0xA4, 0x02, 0x0C, &id.to_be_bytes(), None))
    }

    fn get_challenge(&self) -> Result<Vec<u8>> {
        self.receive(&apdu(0x00, 0x84, 0x00, 0x00, &[], Some(8)))
    }

    fn internal_authenticate(&self, key_num: u32, data: &[u8]) -> Result<Vec<u8>> {
        self.receive(&apdu(0x00, 0x88, 0x00, 0x80 | key_num as u8, data, Some(8)))
    }

    fn verify_hard_pin(&self) -> Result<()> {
        let verify_apdu: [u8; 13] = [0x00, 0x20, 0x00, 0x81, 0x08, 0x25, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        let mut s = vec![
            0x0f, // bTimeOut
            0x05, // bTimeOut2
            0x89, // bmFormatString: BCD, left justified, 1 byte offset
            0x07, // bmPINBlockString
            0x10, // bmPINLengthFormat
            0x08, 0x04, // wPINMaxExtraDigit (max 8, min 4)
            0x02, // bEntryValidationCondition: OK button
            0x01, // bNumberMessage
            0x04, 0x09, // wLangId
            0x00, // bMsgIndex
            0x00, 0x00, 0x00, // bTeoPrologue
        ];
        s.extend_from_slice(&(verify_apdu.len() as u32).to_le_bytes());
        s.extend_from_slice(&verify_apdu);
        let r = self.session.control(Feature::VerifyPinDirect, &s)?;
        if r.len() < 2 {
            return Err(err("Leser lieferte keine Antwort auf die PIN-Prüfung"));
        }
        let sw = u16::from_be_bytes([r[r.len() - 2], r[r.len() - 1]]);
        check(sw, &[0x90])
    }

    fn verify_soft_pin(&self, pin: &str) -> Result<()> {
        if pin.len() < 4 || pin.len() > 12 || !pin.chars().all(|c| c.is_ascii_digit()) {
            return Err(err("Die Karten-PIN muss aus 4 bis 12 Ziffern bestehen"));
        }
        let mut body = [0x25u8, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        for (i, c) in pin.bytes().enumerate() {
            let d = c - b'0';
            let idx = 1 + (i >> 1);
            if i & 1 == 0 {
                body[idx] = (body[idx] & 0x0F) | (d << 4);
            } else {
                body[idx] = (body[idx] & 0xF0) | d;
            }
        }
        self.send(&apdu(0x00, 0x20, 0x00, 0x81, &body, None))
    }
}

impl DdvCard for DdvChipCard {
    fn cid(&mut self) -> Result<Vec<u8>> {
        if self.cid.is_none() {
            self.cid = Some(self.read_record(EF_ID, 0)?);
        }
        Ok(self.cid.clone().unwrap())
    }

    fn bank_data(&mut self, record: u8) -> Result<DdvBankData> {
        let raw = self.read_record(EF_BNK, record.saturating_sub(1))?;
        if raw.len() < 88 {
            return Err(err("EF_BNK-Eintrag zu kurz"));
        }
        let s = |a: usize, b: usize| fints::syntax::latin1_to_string(&raw[a..b]).trim().to_string();
        let mut blz = String::new();
        for &ch in &raw[20..24] {
            for nib in [(ch >> 4) & 0x0F, ch & 0x0F] {
                let n = if nib > 9 { nib ^ 0x0F } else { nib };
                blz.push((b'0' + n) as char);
            }
        }
        Ok(DdvBankData {
            record,
            short_name: s(0, 20),
            blz,
            comm_type: raw[24],
            host: s(25, 53),
            country: s(55, 58),
            user_id: s(58, 88),
        })
    }

    fn keys(&mut self) -> Result<(DdvKey, DdvKey)> {
        if self.ddv1 {
            let k1 = self.receive(&apdu(0xB0, 0xEE, 0x80, 1, &[], Some(256)))?;
            let k2 = self.receive(&apdu(0xB0, 0xEE, 0x80, 2, &[], Some(256)))?;
            Ok((
                DdvKey { num: 2, version: *k1.last().unwrap_or(&0) as u32 },
                DdvKey { num: 3, version: *k2.last().unwrap_or(&0) as u32 },
            ))
        } else {
            self.select_sub_file(0x0013)?;
            let s = self.read_record(0x00, 0)?;
            self.select_sub_file(0x0014)?;
            let e = self.read_record(0x00, 0)?;
            if s.len() < 5 || e.len() < 4 {
                return Err(err("Schlüsselinformationen unvollständig"));
            }
            Ok((DdvKey { num: s[0] as u32, version: s[4] as u32 }, DdvKey { num: e[0] as u32, version: e[3] as u32 }))
        }
    }

    fn sig_id(&mut self) -> Result<u32> {
        let r = self.read_record(EF_SEQ, 0)?;
        if r.len() < 2 {
            return Err(err("Signatur-ID nicht lesbar"));
        }
        Ok(((r[0] as u32) << 8) | r[1] as u32)
    }

    fn set_sig_id(&mut self, id: u32) -> Result<()> {
        self.update_record(EF_SEQ, 0, &[(id >> 8) as u8, id as u8])
    }

    fn verify_pin(&mut self) -> Result<()> {
        if self.pin_ok {
            return Ok(());
        }
        if self.session.has(Feature::VerifyPinDirect) {
            if let Some(n) = &self.pinpad_notify {
                n(true);
            }
            let r = self.verify_hard_pin();
            if let Some(n) = &self.pinpad_notify {
                n(false);
            }
            r?;
        } else {
            let provider = self.soft_pin.as_ref().ok_or_else(|| err("Leser ohne PIN-Pad und keine PIN-Abfrage verfügbar"))?;
            let pin = provider().ok_or(fints::Error::Cancelled)?;
            self.verify_soft_pin(&pin)?;
        }
        self.pin_ok = true;
        Ok(())
    }

    fn sign(&mut self, hash: &[u8; 20]) -> Result<[u8; 8]> {
        let (left, right) = hash.split_at(8);
        self.update_record(EF_MAC, 0, right)?;
        let mut out = [0u8; 8];
        if self.ddv1 {
            let mut body = vec![0xBA, 0x0C, 0xB4, 0x0A, 0x87, 0x08];
            body.extend_from_slice(left);
            body.extend_from_slice(&[0x96, 0x01, 0x00]);
            let d = self.receive(&apdu(0x08, 0xB2, 0x01, (EF_MAC << 3) | 0x04, &body, Some(256)))?;
            out.copy_from_slice(d.get(16..24).ok_or_else(|| err("MAC-Antwort zu kurz"))?);
        } else {
            self.send(&apdu(0x00, 0xDA, 0x01, 0x00, left, None))?;
            let d = self.receive(&apdu(0x04, 0xB2, 0x01, (EF_MAC << 3) | 0x04, &[], Some(256)))?;
            out.copy_from_slice(d.get(12..20).ok_or_else(|| err("MAC-Antwort zu kurz"))?);
        }
        Ok(out)
    }

    fn session_keys(&mut self, enc_key_num: u32) -> Result<([u8; 16], [u8; 16])> {
        let mut plain = [0u8; 16];
        let mut enc = [0u8; 16];
        for i in 0..2 {
            let ch = self.get_challenge()?;
            let e = self.internal_authenticate(enc_key_num, &ch)?;
            if ch.len() < 8 || e.len() < 8 {
                return Err(err("Sitzungsschlüssel unvollständig"));
            }
            plain[i * 8..i * 8 + 8].copy_from_slice(&ch[..8]);
            enc[i * 8..i * 8 + 8].copy_from_slice(&e[..8]);
        }
        Ok((plain, enc))
    }

    fn decrypt_key(&mut self, enc_key_num: u32, encrypted: &[u8]) -> Result<[u8; 16]> {
        if encrypted.len() < 16 {
            return Err(err("verschlüsselter Schlüssel zu kurz"));
        }
        let mut plain = [0u8; 16];
        for i in 0..2 {
            let p = self.internal_authenticate(enc_key_num, &encrypted[i * 8..i * 8 + 8])?;
            plain[i * 8..i * 8 + 8].copy_from_slice(p.get(..8).ok_or_else(|| err("Schlüssel zu kurz"))?);
        }
        Ok(plain)
    }
}

// ---------------------------------------------------------------------------- chipTAN USB

/// Generate a TAN with the girocard in a chipTAN USB capable reader.
/// `hhduc` are the bytes of the rendered flicker code (see [`fints::tan::FlickerCode::usb_payload`]).
/// The user confirms the transaction data on the reader display.
pub fn chiptan_usb(reader: Option<&str>, hhduc: &[u8]) -> Result<String> {
    let s = Session::open(reader, Duration::from_secs(60))?;
    let direct = s.has(Feature::MctReaderDirect);
    let len = (4 + 1 + 1 + 2 + hhduc.len()) as u8;
    let mut cmd = if direct { vec![0x20, 0x76, 0x00, 0x00] } else { vec![0xFF, 0x91, 0x06, 0x00] };
    cmd.extend_from_slice(&[0x00, 0x00, len]); // extended Lc
    cmd.extend_from_slice(&[0, 0, 0, 0]); // activation id
    cmd.push(0x01); // POF
    cmd.push(0x00); // control byte
    cmd.extend_from_slice(&[0x00, hhduc.len() as u8]);
    cmd.extend_from_slice(hhduc);
    cmd.extend_from_slice(&[0x00, 0x00]); // extended Le

    let (data, sw) = if direct {
        let r = s.control(Feature::MctReaderDirect, &cmd)?;
        if r.len() < 2 {
            return Err(err("Leser lieferte keine TAN"));
        }
        (r[..r.len() - 2].to_vec(), u16::from_be_bytes([r[r.len() - 2], r[r.len() - 1]]))
    } else {
        s.transmit_raw(&cmd)?
    };
    check(sw, &[0x90, 0x91])?;
    let tan = parse_bcd_tan(&data).ok_or_else(|| err("Keine TAN vom Leser erhalten (abgebrochen?)"))?;

    // finalize the reader transaction; failures here are not fatal
    let mut fin = if direct { vec![0x20, 0x77, 0x00, 0x00] } else { vec![0xFF, 0x91, 0x07, 0x00] };
    fin.extend_from_slice(&[0x00, 0x00, 0x06, 0, 0, 0, 0, 0x00, 0x00, 0x00, 0x00]);
    let r = if direct { s.control(Feature::MctReaderDirect, &fin).map(|_| ()) } else { s.transmit_raw(&fin).map(|_| ()) };
    if let Err(e) = r {
        log::debug!("chipTAN finalize failed (ignored): {e}");
    }
    Ok(tan)
}

fn parse_bcd_tan(data: &[u8]) -> Option<String> {
    let len = *data.first()? as usize;
    if len == 0 || data.len() < 1 + len {
        return None;
    }
    let bytes = &data[1..1 + len];
    let mut tan = String::new();
    for (i, b) in bytes.iter().enumerate() {
        let hi = b >> 4;
        let lo = b & 0x0F;
        tan.push(char::from_digit(hi as u32, 16)?);
        if i + 1 == bytes.len() && lo == 0x0F {
            break;
        }
        tan.push(char::from_digit(lo as u32, 16)?);
    }
    Some(tan.to_uppercase())
}

// ---------------------------------------------------------------------------- diagnostics

/// Human readable report about readers, reader features and the inserted card.
pub fn diagnose(reader: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let readers = match list_readers() {
        Ok(r) => r,
        Err(e) => return vec![format!("PC/SC: {e}")],
    };
    if readers.is_empty() {
        return vec!["Kein Kartenleser gefunden. Treiber installiert und Leser angeschlossen?".into()];
    }
    for r in &readers {
        out.push(format!(
            "Leser: {}{}{} – Karte {}",
            r.name,
            if r.cyberjack { " [REINER SCT cyberJack]" } else { "" },
            if r.secoder { " [secoder]" } else { "" },
            if r.card_present { "eingesteckt" } else { "nicht eingesteckt" }
        ));
    }
    let target = reader
        .map(|s| s.to_string())
        .or_else(|| readers.iter().find(|r| r.card_present).map(|r| r.name.clone()));
    let Some(target) = target else {
        out.push("Für Feature-Abfrage und Kartentest bitte eine Karte einstecken.".into());
        return out;
    };
    match Session::open(Some(&target), Duration::from_secs(2)) {
        Err(e) => out.push(format!("Verbindung zu {target}: {e}")),
        Ok(s) => {
            out.push(format!("Protokoll: {:?}", s.protocol));
            out.push(format!("ATR: {}", hex(&s.atr)));
            if s.features.is_empty() {
                out.push("Leser meldet keine PC/SC-Part-10-Features (Klasse 1, keine sichere PIN-Eingabe)".into());
            }
            for (f, code) in &s.features {
                out.push(format!("Feature: {} (IOCTL 0x{:08X})", f.label(), code));
            }
            out.push(format!(
                "Sichere PIN-Eingabe: {} · chipTAN USB: {}",
                if s.has(Feature::VerifyPinDirect) { "ja" } else { "nein" },
                if s.has(Feature::MctReaderDirect) { "ja (MCT)" } else { "über FF91-APDU, falls vom Leser unterstützt" }
            ));
            drop(s);
            match DdvChipCard::open(Some(&target), None) {
                Ok(mut c) => {
                    out.push(format!("HBCI-Chipkarte erkannt: {}", c.kind()));
                    if let Ok(cid) = c.cid() {
                        out.push(format!("Karten-ID: {}", hex(&cid)));
                    }
                    for rec in 1..=5u8 {
                        match c.bank_data(rec) {
                            Ok(b) if !b.blz.trim_matches('0').is_empty() => out.push(format!(
                                "Bankzugang {rec}: {} · BLZ {} · Kennung {} · Host {}",
                                b.short_name, b.blz, b.user_id, b.host
                            )),
                            _ => {}
                        }
                    }
                    if let Ok(id) = c.sig_id() {
                        out.push(format!("Signatur-Zähler: {id}"));
                    }
                }
                Err(_) => out.push("Keine HBCI-Chipkarte (DDV) – für chipTAN USB genügt die girocard".into()),
            }
        }
    }
    out
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02X}", x)).collect::<Vec<_>>().join(" ")
}
