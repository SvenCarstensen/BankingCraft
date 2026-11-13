//! High level FinTS client: dialog handling, strong customer authentication
//! (HKTAN process 4 / 2 / S) and the business transactions used by the app.

use crate::bpd::{Bpd, Upd};
use crate::error::{Error, Result};
use crate::model::*;
use crate::response::Response;
use crate::rdh::{BankKeys, KeyCheck, RdhCard, RdhContext, RdhSession};
use crate::security::{build_message, open_message, Auth, DdvCard, DdvKey, Envelope, Opener};
use crate::syntax::{fints_date, latin1_to_string, parse_fints_date, SegmentBuilder, Val};
use crate::transport::{HttpsTransport, TcpTransport, Transport};
use crate::{camt, mt940, pain};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[cfg(test)]
#[path = "business_flow_tests.rs"]
mod business_flow_tests;

/// Static access configuration of one bank login.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub blz: String,
    pub user_id: String,
    /// Kunden-ID; defaults to the user id when empty.
    pub customer_id: String,
    /// https://... for PIN/TAN, host[:port] for TCP (chip card).
    pub url: String,
    /// FinTS product registration number (Deutsche Kreditwirtschaft).
    pub product_id: String,
    pub product_version: String,
}

/// Data learned from the bank that should be persisted between sessions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    pub system_id: String,
    pub bpd: Bpd,
    pub upd: Upd,
    pub tan_method: Option<String>,
    pub tan_medium: Option<String>,
    pub allowed_methods: Vec<String>,
    /// Public keys of the bank (RDH only).
    #[serde(default)]
    pub bank_keys: Option<BankKeys>,
}

pub enum Medium {
    PinTan,
    Ddv(Box<dyn DdvCard>),
    /// RDH chip card (RDH-7/9/10). The `u8` selects the notepad entry (1-based, 0 = match by BLZ/user).
    Rdh(Box<dyn RdhCard>, u8),
}

/// Callbacks into the UI. Called from the worker thread and may block.
pub trait Interaction: Send + Sync {
    fn pin(&self, prompt: &str) -> Option<String>;
    fn choose_tan_method(&self, methods: &[TanMethod]) -> Option<String>;
    fn choose_tan_medium(&self, media: &[String]) -> Option<String>;
    fn tan(&self, challenge: &TanChallenge) -> TanAnswer;
    fn progress(&self, text: &str);
    /// Free text input (e.g. the name of a TAN medium). `None` = cancelled / unsupported.
    fn text(&self, _title: &str, _prompt: &str) -> Option<String> {
        None
    }
    /// Yes/no question (e.g. confirming a bank key fingerprint). Default: no.
    fn confirm(&self, _title: &str, _text: &str) -> bool {
        false
    }
}

struct RdhCtx {
    session: RdhSession,
    context: RdhContext,
}

struct DdvCtx {
    cid: Vec<u8>,
    sig_key: DdvKey,
    enc_key: DdvKey,
}

pub struct Client<'a> {
    pub cfg: Config,
    pub state: State,
    medium: Medium,
    ddv: Option<DdvCtx>,
    rdh: Option<RdhCtx>,
    transport: Box<dyn Transport>,
    ui: &'a dyn Interaction,
    pin: Option<Zeroizing<String>>,
    dialog_id: String,
    msg_num: u32,
    in_dialog: bool,
    /// Strong customer authentication completed (or waived) at dialog initialisation.
    dialog_authenticated: bool,
    /// (segment number, segment code) of the last request, to name segments in error messages.
    last_request: Vec<(u32, String)>,
    /// Segment identifiers of the last request/response pair, without message contents.
    pub last_trace: Option<String>,
}

const SAL_VERSIONS: &[u32] = &[5, 6, 7];
const ANONYMOUS_CUSTOMER: &str = "9999999999";
const KAZ_VERSIONS: &[u32] = &[5, 6, 7];

impl<'a> Client<'a> {
    pub fn new(cfg: Config, state: State, mut medium: Medium, ui: &'a dyn Interaction) -> Result<Self> {
        if matches!(medium, Medium::PinTan) {
            crate::transport::validate_https_url(&cfg.url)?;
        }
        let mut cfg = cfg;
        let mut ddv = None;
        if let Medium::Ddv(card) = &mut medium {
            let bank = card.bank_data(1)?;
            if cfg.blz.is_empty() {
                cfg.blz = bank.blz.clone();
            }
            if cfg.user_id.is_empty() {
                cfg.user_id = bank.user_id.clone();
            }
            if cfg.url.is_empty() {
                cfg.url = bank.host.clone();
            }
            let (sig_key, enc_key) = card.keys()?;
            ddv = Some(DdvCtx { cid: card.cid()?, sig_key, enc_key });
        }
        let mut rdh = None;
        if let Medium::Rdh(card, record) = &mut medium {
            card.verify_pin()?;
            let contexts = card.contexts()?;
            let wanted = *record;
            let context = contexts.iter().find(|c| {
                    (wanted == 0 || c.record == wanted)
                        && (cfg.blz.is_empty() || c.blz == cfg.blz)
                        && (cfg.user_id.is_empty() || c.user_id == cfg.user_id)
                })
                .cloned()
                .ok_or_else(|| Error::Security(i18n::tr("Der gewählte Bankzugang wurde auf der Chipkarte nicht gefunden").into()))?;
            if cfg.blz.is_empty() {
                cfg.blz = context.blz.clone();
            }
            if cfg.user_id.is_empty() {
                cfg.user_id = context.user_id.clone();
            }
            if cfg.customer_id.is_empty() && !context.customer_id.is_empty() {
                cfg.customer_id = context.customer_id.clone();
            }
            if cfg.url.is_empty() {
                cfg.url = context.host.clone();
            }
            let session = RdhSession {
                cid: card.cid()?,
                profile: context.profile,
                sign_key: card.sign_key()?,
                decipher_key: card.decipher_key()?,
            };
            if !matches!(session.profile, 7 | 9 | 10) {
                return Err(Error::NotSupported(i18n::tr_format!("RDH-Profil {}", session.profile)));
            }
            rdh = Some(RdhCtx { session, context });
        }
        if cfg.customer_id.is_empty() {
            cfg.customer_id = cfg.user_id.clone();
        }
        if cfg.product_version.is_empty() {
            cfg.product_version = env!("CARGO_PKG_VERSION").into();
        }
        let transport: Box<dyn Transport> = if cfg.url.starts_with("https://") || cfg.url.starts_with("http://") {
            Box::new(HttpsTransport::new(&cfg.url))
        } else {
            Box::new(TcpTransport::new(&cfg.url))
        };
        Ok(Self {
            cfg,
            state,
            medium,
            ddv,
            rdh,
            transport,
            ui,
            pin: None,
            dialog_id: "0".into(),
            msg_num: 1,
            in_dialog: false,
            dialog_authenticated: false,
            last_request: Vec::new(),
            last_trace: None,
        })
    }

    fn is_pintan(&self) -> bool {
        matches!(self.medium, Medium::PinTan)
    }

    /// Replace the transport (tests, simulators).
    #[doc(hidden)]
    pub fn set_transport(&mut self, t: Box<dyn Transport>) {
        self.transport = t;
    }

    fn is_rdh(&self) -> bool {
        matches!(self.medium, Medium::Rdh(..))
    }

    /// RDH profile and notepad data of the card (RDH only).
    pub fn rdh_context(&self) -> Option<&RdhContext> {
        self.rdh.as_ref().map(|r| &r.context)
    }

    pub fn rdh_profile(&self) -> Option<u32> {
        self.rdh.as_ref().map(|r| r.session.profile)
    }

    fn security_function(&self) -> String {
        self.state.tan_method.clone().unwrap_or_else(|| "999".into())
    }

    fn ensure_pin(&mut self) -> Result<Zeroizing<String>> {
        if let Some(p) = &self.pin {
            return Ok(p.clone());
        }
        let p = Zeroizing::new(self
            .ui
            .pin(&i18n::tr_format!("PIN für {} ({})", self.cfg.user_id, self.state.bpd.bank_name))
            .ok_or(Error::Cancelled)?);
        self.pin = Some(p.clone());
        Ok(p)
    }

    // ------------------------------------------------------------------ messaging

    fn exchange(&mut self, segments: Vec<SegmentBuilder>, tan: Option<&str>) -> Result<Response> {
        let pin = if self.is_pintan() { Some(self.ensure_pin()?) } else { None };
        self.last_request = segments.iter().enumerate().map(|(k, s)| (3 + k as u32, s.code.clone())).collect();
        let sf = self.security_function();
        let raw = {
            let env = Envelope {
                dialog_id: &self.dialog_id,
                message_number: self.msg_num,
                blz: &self.cfg.blz,
                user_id: &self.cfg.user_id,
                system_id: if self.state.system_id.is_empty() { "0" } else { &self.state.system_id },
            };
            let no_sysid = self.state.system_id.is_empty() || self.state.system_id == "0";
            let bank_crypt = self.state.bank_keys.as_ref().and_then(|k| k.crypt.as_ref());
            let auth = match (&mut self.medium, &self.ddv, &self.rdh) {
                (Medium::PinTan, _, _) => Auth::PinTan { pin: pin.as_ref().map(|p| p.as_str()).unwrap_or(""), tan, security_function: &sf },
                (Medium::Ddv(card), Some(ctx), _) => Auth::Ddv {
                    card: card.as_mut(),
                    cid: &ctx.cid,
                    sig_key: ctx.sig_key,
                    enc_key: ctx.enc_key,
                },
                (Medium::Rdh(card, _), _, Some(ctx)) => Auth::Rdh {
                    card: card.as_mut(),
                    session: &ctx.session,
                    bank: bank_crypt.ok_or_else(|| Error::Security(i18n::tr("Der öffentliche Schlüssel der Bank fehlt").into()))?,
                    no_sysid,
                },
                _ => return Err(Error::Security(i18n::tr("Chipkarte nicht initialisiert").into())),
            };
            Zeroizing::new(build_message(&env, auth, segments)?)
        };
        self.msg_num += 1;
        let answer = self.transport.send(&raw)?;
        crate::response::validate_envelope(&answer, &self.dialog_id, self.msg_num - 1)?;
        self.last_trace = Some(format!(">> {}\n\n<< {}", protocol_summary(&raw), protocol_summary(&answer)));
        let bank_sign = self.state.bank_keys.as_ref().and_then(|k| k.sign.as_ref());
        let segs = match (&mut self.medium, &self.ddv) {
            (Medium::Ddv(card), Some(ctx)) => open_message(&answer, Opener::Ddv(card.as_mut(), ctx.enc_key.num))?,
            (Medium::Rdh(card, _), _) => open_message(&answer, Opener::Rdh(card.as_mut(), bank_sign))?,
            _ => open_message(&answer, Opener::Plain)?,
        };
        let resp = Response::new(segs);
        if let Some(id) = &resp.dialog_id {
            if self.dialog_id == "0" && id != "0" {
                self.dialog_id = id.clone();
            }
        }
        self.state.bpd.update_from(&resp);
        self.state.upd.update_from(&resp);
        let allowed = resp.allowed_security_functions();
        if !allowed.is_empty() {
            self.state.allowed_methods = allowed;
        }
        for c in &resp.codes {
            log::info!("FinTS Rückmeldung {}", diagnostic_code(&c.code));
        }
        if let Some(e) = resp.errors().first() {
            if e.code == "9931" || e.code == "9942" || e.code == "9340" {
                // wrong PIN / locked: forget cached PIN
                self.pin = None;
            }
            let text = resp.errors().iter().map(|c| self.describe_code(c)).collect::<Vec<_>>().join("\n");
            return Err(Error::Bank { code: e.code.clone(), text });
        }
        Ok(resp)
    }

    /// "9110 Text (HKTAN, Feld 4)" – names the segment a return code refers to.
    fn describe_code(&self, c: &crate::response::ReturnCode) -> String {
        let seg = c.segment_ref.and_then(|n| {
            self.last_request
                .iter()
                .find(|(num, _)| *num == n)
                .map(|(_, code)| code.clone())
                .or_else(|| match n {
                    1 => Some("HNHBK".into()),
                    2 => Some("HNSHK".into()),
                    998 => Some("HNVSK".into()),
                    999 => Some("HNVSD".into()),
                    _ => None,
                })
        });
        let mut out = format!("{} {}", c.code, c.text);
        match (seg, &c.reference_element) {
            (Some(sg), Some(el)) => out.push_str(&i18n::tr_format!(" ({sg}, Element {el})")),
            (Some(sg), None) => out.push_str(&format!(" ({sg})")),
            (None, Some(el)) => out.push_str(&i18n::tr_format!(" (Element {el})")),
            _ => {}
        }
        out
    }

    /// Unsigned message for anonymous dialogs (bank parameter data without login).
    fn exchange_anonymous(&mut self, segments: Vec<SegmentBuilder>) -> Result<Response> {
        let raw = {
            let env = Envelope {
                dialog_id: &self.dialog_id,
                message_number: self.msg_num,
                blz: &self.cfg.blz,
                user_id: ANONYMOUS_CUSTOMER,
                system_id: "0",
            };
            build_message(&env, Auth::None, segments)?
        };
        self.msg_num += 1;
        let answer = self.transport.send(&raw)?;
        crate::response::validate_envelope(&answer, &self.dialog_id, self.msg_num - 1)?;
        self.last_trace = Some(format!(">> {}\n\n<< {}", protocol_summary(&raw), protocol_summary(&answer)));
        let resp = Response::new(open_message(&answer, Opener::Plain)?);
        if let Some(id) = &resp.dialog_id {
            if self.dialog_id == "0" && id != "0" {
                self.dialog_id = id.clone();
            }
        }
        self.state.bpd.update_from(&resp);
        for c in &resp.codes {
            log::info!("FinTS (anonym) Rückmeldung {}", diagnostic_code(&c.code));
        }
        if let Some(e) = resp.errors().first() {
            let text = resp.errors().iter().map(|c| format!("{} {}", c.code, c.text)).collect::<Vec<_>>().join("\n");
            return Err(Error::Bank { code: e.code.clone(), text });
        }
        Ok(resp)
    }

    /// Load the bank parameter data (incl. TAN methods) in an anonymous dialog.
    /// Banks that only accept two-step methods reject the one-step "999" login,
    /// so the TAN method has to be known before the first personal dialog.
    fn fetch_bpd_anonymous(&mut self) -> Result<()> {
        self.ui.progress(i18n::tr("Lade Bankparameter …"));
        self.reset_dialog();
        let segs = vec![
            SegmentBuilder::new("HKIDN", 2)
                .deg(vec!["280".into(), (&self.cfg.blz).into()])
                .de(ANONYMOUS_CUSTOMER)
                .de("0")
                .de("0"),
            SegmentBuilder::new("HKVVB", 3)
                .de(self.state.bpd.version)
                .de("0")
                .de("1")
                .de(self.product_id())
                .de(&self.cfg.product_version),
        ];
        let r = self.exchange_anonymous(segs);
        if self.dialog_id != "0" {
            let end = SegmentBuilder::new("HKEND", 1).de(self.dialog_id.clone());
            let _ = self.exchange_anonymous(vec![end]);
        }
        self.reset_dialog();
        r.map(|_| ())
    }

    fn product_id(&self) -> String {
        if self.cfg.product_id.is_empty() { "BANKINGCRAFT".to_string() } else { self.cfg.product_id.clone() }
    }

    fn hktan_version(&self) -> u32 {
        match self.state.bpd.hktan_version {
            7 => 7,
            6 => 6,
            _ => 0,
        }
    }

    fn uses_sca(&self) -> bool {
        self.is_pintan() && self.hktan_version() >= 6 && self.security_function() != "999"
    }

    fn current_method(&self) -> Option<TanMethod> {
        let sf = self.security_function();
        self.state.bpd.tan_methods.iter().find(|m| m.security_function == sf).cloned()
    }

    fn needs_medium(&self) -> bool {
        self.current_method().map(|m| m.needs_medium).unwrap_or(false)
    }

    /// TAN medium for HKTAN field 11: always sent when known (as AqBanking does).
    fn medium_val(&self) -> Val {
        Val::from(self.state.tan_medium.clone())
    }

    fn hktan_process4(&self, reference: &str) -> SegmentBuilder {
        let medium = self.medium_val();
        SegmentBuilder::new("HKTAN", self.hktan_version())
            .de("4")
            .de(reference)
            .de(Val::Empty)
            .de(Val::Empty)
            .de(Val::Empty)
            .de(Val::Empty)
            .de(Val::Empty)
            .de(Val::Empty)
            .de(Val::Empty)
            .de(Val::Empty)
            .de(medium)
    }

    /// HKTAN process 2 (TAN) or S (decoupled status): reference, "weitere TAN folgt" = N and –
    /// like AqBanking – the TAN medium in field 11.
    fn hktan_followup(&self, process: &str, task_ref: &str) -> SegmentBuilder {
        SegmentBuilder::new("HKTAN", self.hktan_version())
            .de(process)
            .de(Val::Empty) // segment id
            .de(Val::Empty) // account
            .de(Val::Empty) // order hash
            .de(task_ref)
            .de("N") // further TAN follows
            .de(Val::Empty) // cancel order
            .de(Val::Empty) // SMS account
            .de(Val::Empty) // challenge class
            .de(Val::Empty) // challenge parameters
            .de(self.medium_val())
    }

    /// Send business segments, performing SCA if the bank requires it.
    fn exchange_sca(&mut self, segments: Vec<SegmentBuilder>, reference: &str, force_tan: bool) -> Result<Response> {
        // Account information in a dialog whose initialisation was already strongly authenticated
        // is covered by that authentication (PSD2) – an extra HKTAN would make banks like the
        // Postbank push a new BestSign request for every single job.
        let covered = self.dialog_authenticated && !force_tan && reference != "HKIDN" && is_account_information(reference);
        if covered && self.state.bpd.needs_tan(reference) {
            match self.exchange_sca_inner(segments.clone(), reference, false, true) {
                Err(Error::Bank { code, text }) if code == "9075" || code == "9076" || text.contains("9075") || text.contains("9076") => {
                    log::info!("{reference}: bank requires SCA despite authenticated dialog, retrying with HKTAN");
                    if text.contains("9800") || code.starts_with("98") {
                        // dialog was aborted: open a new one
                        self.reset_dialog();
                        self.open()?;
                    }
                    return self.exchange_sca_inner(segments, reference, true, false);
                }
                r => return r,
            }
        }
        self.exchange_sca_inner(segments, reference, force_tan, false)
    }

    fn exchange_sca_inner(&mut self, mut segments: Vec<SegmentBuilder>, reference: &str, force_tan: bool, no_tan: bool) -> Result<Response> {
        let sca = !no_tan && self.uses_sca() && (force_tan || reference == "HKIDN" || self.state.bpd.needs_tan(reference));
        if sca {
            segments.push(self.hktan_process4(reference));
        }
        let resp = self.exchange(segments, None)?;
        let approval_pending = resp.code("0030").is_some() || resp.code("3955").is_some();
        if !approval_pending {
            return Ok(resp);
        }
        if !sca { return Err(Error::Security(i18n::tr("Die Bank fordert eine Freigabe, die nicht abgeschlossen wurde").into())); }
        let hitan = resp.first("HITAN").ok_or_else(|| Error::Other(i18n::tr("TAN erforderlich, aber HITAN fehlt").into()))?;
        let method = self.current_method().unwrap_or(TanMethod {
            security_function: self.security_function(),
            name: "TAN".into(),
            technical_id: String::new(),
            dk_id: String::new(),
            tan_process: "2".into(),
            decoupled: false,
            needs_medium: false,
            max_tan_length: 0,
            hhduc_response_required: false,
            decoupled_poll_wait_first: 2,
            decoupled_poll_wait_next: 2,
        });
        let task_reference = hitan.text_or_empty(3, 0);
        if task_reference.trim().is_empty() { return Err(Error::Security(i18n::tr("TAN-Auftragsreferenz fehlt").into())); }
        let mut challenge = TanChallenge {
            text: hitan.text_or_empty(4, 0),
            hhduc: hitan.bin(5, 0).map(|b| b.to_vec()).filter(|b| !b.is_empty()),
            // 3955 = "Sicherheitsfreigabe erfolgt über anderen Kanal" (decoupled), as AqBanking
            decoupled: method.decoupled || resp.code("3955").is_some(),
            method,
            task_reference: task_reference.clone(),
        };
        loop {
            match self.ui.tan(&challenge) {
                TanAnswer::Cancel => return Err(Error::Cancelled),
                TanAnswer::Tan(tan) => {
                    let tan = Zeroizing::new(tan);
                    let seg = self.hktan_followup("2", &task_reference);
                    let r = self.exchange(vec![seg], Some(&tan))?;
                    completed_approval(&r)?;
                    return Ok(merge(resp, r));
                }
                TanAnswer::Confirmed => {
                    let process = if self.hktan_version() >= 7 { "S" } else { "2" };
                    let seg = self.hktan_followup(process, &task_reference);
                    let r = self.exchange(vec![seg], None)?;
                    if r.code("3956").is_some() {
                        challenge.text = i18n::tr_format!(
                            "Die Freigabe ist noch nicht erfolgt. Bitte bestätigen Sie den Auftrag in Ihrer App.\n\n{}",
                            strip_html(&challenge.text)
                        );
                        continue;
                    }
                    completed_approval(&r)?;
                    return Ok(merge(resp, r));
                }
            }
        }
    }

    // ------------------------------------------------------------------ dialogs

    fn init_segments(&self, with_sync: bool) -> Vec<SegmentBuilder> {
        // PIN/TAN and RDH work with a system id (Kundensystem-Status 1), DDV without
        let pintan = self.is_pintan() || self.is_rdh();
        let sys_id = if pintan {
            if self.state.system_id.is_empty() { "0".to_string() } else { self.state.system_id.clone() }
        } else {
            "0".to_string()
        };
        let mut segs = vec![
            SegmentBuilder::new("HKIDN", 2)
                .deg(vec!["280".into(), (&self.cfg.blz).into()])
                .de(&self.cfg.customer_id)
                .de(sys_id)
                .de(if pintan { "1" } else { "0" }),
            SegmentBuilder::new("HKVVB", 3)
                .de(self.state.bpd.version)
                .de(self.state.upd.version)
                .de("1")
                .de(self.product_id())
                .de(&self.cfg.product_version),
        ];
        if with_sync {
            segs.push(SegmentBuilder::new("HKSYN", 3).de("0"));
        }
        segs
    }

    /// Fetch a system id (PIN/TAN only): HKIDN + HKVVB + HKSYN in a single-step dialog
    /// (security function 999, profile PIN:1, no HKTAN) – the same as AqBanking's JobSync.
    /// Also delivers the UPD and the TAN methods the user may use (code 3920).
    pub fn synchronize(&mut self) -> Result<()> {
        if self.is_rdh() {
            return self.synchronize_rdh();
        }
        if !self.is_pintan() {
            return Ok(());
        }
        self.ui.progress(i18n::tr("Synchronisiere Bankzugang …"));
        let saved = self.state.tan_method.replace("999".into());
        self.reset_dialog();
        let mut segs = self.init_segments(true);
        // the synchronisation is always done with system id 0
        segs[0] = SegmentBuilder::new("HKIDN", 2)
            .deg(vec!["280".into(), (&self.cfg.blz).into()])
            .de(&self.cfg.customer_id)
            .de("0")
            .de("1");
        let r = self.exchange(segs, None);
        if let Ok(resp) = &r {
            if let Some(sid) = resp.first("HISYN").and_then(|s| s.text(1, 0)) {
                self.state.system_id = sid;
            }
        }
        self.end_dialog().ok();
        self.state.tan_method = saved;
        r.map(|_| ())
    }

    /// RDH: system id via a signed and encrypted dialog without system id (AqBanking JobSync,
    /// security details `1::0`).
    fn synchronize_rdh(&mut self) -> Result<()> {
        self.ui.progress(i18n::tr("Synchronisiere Bankzugang (Chipkarte) …"));
        self.state.system_id.clear();
        self.reset_dialog();
        let mut segs = self.init_segments(true);
        segs[0] = SegmentBuilder::new("HKIDN", 2)
            .deg(vec!["280".into(), (&self.cfg.blz).into()])
            .de(&self.cfg.customer_id)
            .de("0")
            .de("1");
        let r = self.exchange(segs, None);
        let mut sid = None;
        if let Ok(resp) = &r {
            sid = resp.first("HISYN").and_then(|s| s.text(1, 0));
        }
        self.end_dialog().ok();
        if let Some(sid) = sid {
            self.state.system_id = sid;
        }
        r.map(|_| ())
    }

    /// RDH: fetch the bank's public keys in an anonymous dialog (HKISA) and check them
    /// against the hash on the card – or let the user compare the fingerprint with the INI letter.
    pub fn fetch_bank_keys(&mut self) -> Result<()> {
        let Some(ctx) = self.rdh.as_ref() else { return Ok(()) };
        let profile = ctx.session.profile;
        let card_hash = ctx.context.bank_key_hash.clone();
        self.ui.progress(i18n::tr("Lade öffentliche Schlüssel der Bank …"));
        self.reset_dialog();
        let segs = vec![
            SegmentBuilder::new("HKIDN", 2)
                .deg(vec!["280".into(), (&self.cfg.blz).into()])
                .de(ANONYMOUS_CUSTOMER)
                .de("0")
                .de("0"),
            SegmentBuilder::new("HKVVB", 3)
                .de(self.state.bpd.version)
                .de("0")
                .de("1")
                .de(self.product_id())
                .de(&self.cfg.product_version),
            crate::rdh::hkisa(profile, &self.cfg.blz, "V"),
            crate::rdh::hkisa(profile, &self.cfg.blz, "S"),
        ];
        let r = self.exchange_anonymous(segs);
        if self.dialog_id != "0" {
            let end = SegmentBuilder::new("HKEND", 1).de(self.dialog_id.clone());
            let _ = self.exchange_anonymous(vec![end]);
        }
        self.reset_dialog();
        let resp = r?;
        let mut keys = BankKeys::default();
        for seg in resp.find("HIISA") {
            if let Some(k) = crate::rdh::parse_hiisa(seg) {
                match k.kind.as_str() {
                    "V" => keys.crypt = Some(k),
                    "S" => keys.sign = Some(k),
                    _ => {}
                }
            }
        }
        let Some(crypt) = keys.crypt.clone() else {
            return Err(Error::Security(i18n::tr("Die Bank hat keinen Verschlüsselungsschlüssel geliefert").into()));
        };
        if keys.sign.is_none() {
            return Err(Error::Security(i18n::tr("Die Bank hat keinen Signaturschlüssel geliefert; RDH ohne geprüfte Banksignatur wird nicht unterstützt").into()));
        }
        // The hash on the card belongs to one key (number/version). Banks often use the same
        // number/version for both keys, so a non-matching key only counts as a mismatch when no
        // key matches the hash at all.
        let checked: Vec<(crate::rdh::BankKey, KeyCheck)> = [Some(&crypt), keys.sign.as_ref()]
            .into_iter()
            .flatten()
            .map(|k| (k.clone(), crate::rdh::check_bank_key(k, card_hash.as_ref())))
            .collect();
        let any_verified = checked.iter().any(|(_, c)| *c == KeyCheck::Verified);
        let mut unknown = Vec::new();
        let mut mismatch = Vec::new();
        for (k, c) in checked {
            match c {
                KeyCheck::Verified => log::info!("RDH: Bankschlüssel {} mit dem Hashwert der Chipkarte geprüft", k.kind),
                KeyCheck::Mismatch if !any_verified => mismatch.push(k),
                _ => unknown.push(k),
            }
        }
        let describe = |k: &crate::rdh::BankKey| {
            i18n::tr_format!(
                "\n{} (Nr. {}, Version {}, {} Bit), SHA-256:\n{}\n",
                if k.kind == "V" { i18n::tr("Verschlüsselungsschlüssel") } else { i18n::tr("Signaturschlüssel") },
                k.num,
                k.version,
                k.size() * 8,
                k.fingerprint()
            )
        };
        if !mismatch.is_empty() {
            // as AqBanking: strong warning, the user may only continue after comparing with the INI letter
            let mut text = String::from(
                i18n::tr("ACHTUNG: Der von der Bank übermittelte Schlüssel stimmt NICHT mit dem Hashwert auf Ihrer Chipkarte überein. \
                 Fahren Sie nur fort, wenn der Hashwert exakt mit Ihrem INI-Brief übereinstimmt – andernfalls brechen Sie ab und \
                 wenden Sie sich an Ihre Bank.\n"),
            );
            if let Some(h) = &card_hash {
                text.push_str(&i18n::tr_format!("\nHashwert auf der Chipkarte:\n{}\n", crate::rdh::hex_blocks(&h.hash)));
            }
            for k in &mismatch {
                text.push_str(&describe(k));
            }
            if !self.ui.confirm(i18n::tr("Bankschlüssel stimmt nicht überein"), &text) {
                return Err(Error::Security(
                    i18n::tr("Der Schlüssel der Bank stimmt nicht mit dem Hashwert auf der Chipkarte überein und wurde nicht übernommen.").into(),
                ));
            }
        }
        if !unknown.is_empty() {
            let mut text = String::from(
                i18n::tr("Bitte vergleichen Sie die Hashwerte mit dem INI-Brief Ihrer Bank. Nur bei exakter Übereinstimmung bestätigen.\n"),
            );
            for k in &unknown {
                text.push_str(&describe(k));
            }
            if !self.ui.confirm(i18n::tr("Bankschlüssel prüfen"), &text) {
                return Err(Error::Cancelled);
            }
        }
        keys.verified = true;
        self.state.bank_keys = Some(keys);
        Ok(())
    }

    fn reset_dialog(&mut self) {
        self.dialog_id = "0".into();
        self.msg_num = 1;
        self.in_dialog = false;
        self.dialog_authenticated = false;
    }

    /// Open a dialog; selects TAN method/medium interactively when needed.
    pub fn open(&mut self) -> Result<()> {
        if self.in_dialog {
            return Ok(());
        }
        if self.is_pintan() {
            // 1. bank parameter data (anonymous; the single-step sync below delivers it as well)
            if self.state.bpd.tan_methods.is_empty() && self.fetch_bpd_anonymous().is_err() {
                log::warn!("Anonymous BPD request failed");
            }
            // 2. system id via single-step synchronisation (no SCA, as AqBanking)
            if self.state.system_id.is_empty() {
                self.synchronize()?;
            }
            // 3. TAN method (restricted to the methods reported with 3920)
            self.select_tan_method()?;
            // 4. TAN medium, if the method needs one
            if self.needs_medium() && self.state.tan_medium.is_none() {
                self.state.tan_medium = self.determine_tan_medium()?;
            }
        }
        if self.is_rdh() {
            if !self.state.bank_keys.as_ref().map(|k| k.verified && k.crypt.is_some() && k.sign.is_some()).unwrap_or(false) {
                self.fetch_bank_keys()?;
            }
            if self.state.system_id.is_empty() {
                self.synchronize_rdh()?;
            }
        }
        self.ui.progress(i18n::tr("Dialog wird eröffnet …"));
        self.reset_dialog();
        let segs = self.init_segments(false);
        self.exchange_sca(segs, "HKIDN", false)?;
        self.in_dialog = true;
        self.dialog_authenticated = self.uses_sca();
        Ok(())
    }

    fn select_tan_method(&mut self) -> Result<()> {
        let allowed: Vec<TanMethod> = self
            .state
            .bpd
            .tan_methods
            .iter()
            .filter(|m| self.state.allowed_methods.is_empty() || self.state.allowed_methods.contains(&m.security_function))
            .filter(|m| m.security_function != "999")
            .cloned()
            .collect();
        let valid = self
            .state
            .tan_method
            .as_ref()
            .map(|sf| allowed.iter().any(|m| &m.security_function == sf))
            .unwrap_or(false);
        if !valid {
            self.state.tan_method = match allowed.len() {
                0 => None,
                1 => Some(allowed[0].security_function.clone()),
                _ => Some(self.ui.choose_tan_method(&allowed).ok_or(Error::Cancelled)?),
            };
        }
        Ok(())
    }

    /// Determine the TAN medium: HKTAB in a single-step dialog (HKTAB needs no SCA),
    /// otherwise ask the user – AqBanking always takes it from the user settings.
    fn determine_tan_medium(&mut self) -> Result<Option<String>> {
        let mut media: Vec<String> = Vec::new();
        if let Some(v) = self.state.bpd.version_for("HKTAB", &[4, 5]) {
            self.ui.progress(i18n::tr("Lade TAN-Medien …"));
            let saved = self.state.tan_method.replace("999".into());
            self.reset_dialog();
            let segs = self.init_segments(false);
            let r = self
                .exchange(segs, None)
                .and_then(|_| self.exchange(vec![SegmentBuilder::new("HKTAB", v).de("0").de("A")], None));
            match r {
                Ok(resp) => media = parse_hitab(&resp),
                Err(_) => log::warn!("TAN media lookup failed"),
            }
            self.end_dialog().ok();
            self.state.tan_method = saved;
        }
        let chosen = match media.len() {
            0 => None,
            1 => Some(media[0].clone()),
            _ => self.ui.choose_tan_medium(&media),
        };
        if chosen.is_some() {
            return Ok(chosen);
        }
        let name = self.current_method().map(|m| m.name).unwrap_or_default();
        Ok(self
            .ui
            .text(
                i18n::tr("TAN-Medium"),
                &i18n::tr_format!(
                    "Die Bank verlangt für „{name}“ die Bezeichnung Ihres TAN-Mediums (z. B. den Gerätenamen aus der BestSign- bzw. photoTAN-App). Bitte genau so eingeben, wie er in der App bzw. im Onlinebanking angezeigt wird."
                ),
            )
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()))
    }

    /// TAN media list (HKTAB). Opens its own short dialog when called outside a dialog.
    fn tan_media_raw(&mut self) -> Result<Vec<String>> {
        let Some(v) = self.state.bpd.version_for("HKTAB", &[4, 5]) else { return Ok(vec![]) };
        let was_open = self.in_dialog;
        let resp = if was_open {
            self.exchange(vec![SegmentBuilder::new("HKTAB", v).de("0").de("A")], None)?
        } else {
            self.reset_dialog();
            let segs = self.init_segments(false);
            let r = self
                .exchange_sca(segs, "HKIDN", false)
                .and_then(|_| self.exchange_sca(vec![SegmentBuilder::new("HKTAB", v).de("0").de("A")], "HKTAB", false));
            self.end_dialog().ok();
            r?
        };
        Ok(parse_hitab(&resp))
    }

    pub fn end_dialog(&mut self) -> Result<()> {
        if self.dialog_id == "0" {
            self.in_dialog = false;
            return Ok(());
        }
        let seg = SegmentBuilder::new("HKEND", 1).de(self.dialog_id.clone());
        let r = self.exchange(vec![seg], None);
        self.reset_dialog();
        r.map(|_| ())
    }

    // ------------------------------------------------------------------ jobs

    fn ktv(a: &Account) -> Vec<Val> {
        vec![(&a.account_number).into(), (&a.sub_account).into(), "280".into(), (&a.blz).into()]
    }

    fn kti(a: &Account) -> Vec<Val> {
        vec![
            (&a.iban).into(),
            (&a.bic).into(),
            (&a.account_number).into(),
            (&a.sub_account).into(),
            "280".into(),
            (&a.blz).into(),
        ]
    }

    /// Kontoverbindung international for SEPA orders – full KTI as AqBanking sends it.
    fn sepa_account(a: &Account) -> Vec<Val> {
        Self::kti(a)
    }

    /// Accounts from the UPD, enriched with BIC/IBAN via HKSPA.
    pub fn accounts(&mut self) -> Result<Vec<Account>> {
        // Older vaults discarded DK... permissions. Ask for fresh parameters when a
        // stored credit card has no usable read permission instead of trusting that cache.
        if !self.in_dialog && self.state.upd.accounts.iter().any(|a| (50..=59).contains(&a.kind)
            && !a.allowed.iter().any(|c| matches!(c.as_str(), "HKKKU" | "DKKKU" | "HKKAZ" | "HKCAZ"))) {
            self.state.upd.version = 0;
            self.state.bpd.version = 0;
        }
        self.open()?;
        if let Some(v) = self.state.bpd.version_for("HKSPA", &[1, 2, 3]) {
            self.ui.progress(i18n::tr("Lade SEPA-Kontoverbindungen …"));
            self.exchange_sca(vec![SegmentBuilder::new("HKSPA", v)], "HKSPA", false)?;
        }
        let mut accounts = self.state.upd.accounts.clone();
        for a in &mut accounts {
            if a.blz.is_empty() {
                a.blz = self.cfg.blz.clone();
            }
        }
        Ok(accounts)
    }

    pub fn balance(&mut self, a: &Account) -> Result<Option<Balance>> {
        self.open()?;
        if let Some((code, version)) = self.card_job(a, "HKKKS", 1, "DKKKS", 3)? {
            self.ui.progress(&i18n::tr_format!("Kreditkartensaldo {} …", display_account(a)));
            let mut seg = SegmentBuilder::new(code, version)
                .deg(if code == "HKKKS" { Self::kti(a) } else { Self::ktv(a) }).de(&a.account_number);
            if code == "HKKKS" { seg = seg.de(&a.sub_account); }
            let response = self.exchange_sca(vec![seg], code, false)?;
            let answer = if code == "HKKKS" { "HIKKS" } else { "DIKKS" };
            let s = response.first(answer).ok_or_else(|| Error::Syntax(i18n::tr("Kreditkartensaldo fehlt in der Bankantwort").into()))?;
            if s.version != version { return Err(Error::NotSupported(i18n::tr("Version der Kreditkartensaldenantwort").into())); }
            return crate::credit_card::balance(s);
        }
        if !a.allows("HKSAL") { return Err(Error::NotSupported(i18n::tr("Saldo für dieses Konto").into())); }
        let Some(v) = self.state.bpd.version_for("HKSAL", SAL_VERSIONS) else {
            return Err(Error::NotSupported(i18n::tr("Saldenabfrage (HKSAL)").into()));
        };
        self.ui.progress(&i18n::tr_format!("Saldo {} …", display_account(a)));
        let acc = if v >= 7 { Self::kti(a) } else { Self::ktv(a) };
        let resp = self.exchange_sca(vec![SegmentBuilder::new("HKSAL", v).deg(acc).de("N")], "HKSAL", false)?;
        let Some(s) = resp.first("HISAL") else { return Ok(None) };
        let parse_bal = |i: usize| -> Option<(rust_decimal::Decimal, NaiveDate, String)> {
            let sign = s.text(i, 0)?;
            let amount = crate::syntax::parse_amount(&s.text(i, 1)?)?;
            let ccy = s.text_or_empty(i, 2);
            let date = parse_fints_date(&s.text_or_empty(i, 3)).unwrap_or_else(|| chrono::Local::now().date_naive());
            Some((if sign == "D" { -amount } else { amount }, date, ccy))
        };
        let Some((booked, date, ccy)) = parse_bal(4) else { return Ok(None) };
        // HISAL: 4 gebuchter Saldo, 5 vorgemerkt, 6 Kreditlinie, 7 verfügbarer Betrag
        let available = s.text(7, 0).and_then(|t| crate::syntax::parse_amount(&t));
        Ok(Some(Balance {
            booked,
            available,
            currency: if ccy.is_empty() { s.text_or_empty(3, 0) } else { ccy },
            date,
        }))
    }

    pub fn transactions(&mut self, a: &Account, from: NaiveDate, to: Option<NaiveDate>) -> Result<Vec<Transaction>> {
        self.transaction_report(a, from, to).map(|r| r.transactions)
    }

    pub fn transaction_report(&mut self, a: &Account, from: NaiveDate, to: Option<NaiveDate>) -> Result<TransactionReport> {
        self.open()?;
        if let Some((code, version)) = self.card_job(a, "HKKKU", 1, "DKKKU", 2)? {
            return self.credit_card_report(a, code, version, from, to);
        }
        let camt_fmt = self.state.bpd.preferred_camt();
        let use_camt = a.allows("HKCAZ") && camt_fmt.is_some() && self.state.bpd.version_for("HKCAZ", &[1]).is_some() && a.is_sepa();
        if !use_camt && !a.allows("HKKAZ") { return Err(Error::NotSupported(i18n::tr("Umsätze für dieses Konto").into())); }
        self.ui.progress(&i18n::tr_format!("Umsätze {} …", display_account(a)));
        let mut out = Vec::new();
        let mut touchdown: Option<String> = None;
        let mut seen_pages = std::collections::HashSet::new();
        for _page in 0..50 {
            let seg = if use_camt {
                SegmentBuilder::new("HKCAZ", 1)
                    .deg(Self::kti(a))
                    .deg(vec![camt_fmt.clone().into()])
                    .de("N")
                    .de(fints_date(from))
                    .de(to.map(fints_date).unwrap_or(Val::Empty))
                    .de(Val::Empty)
                    .de(touchdown.clone())
            } else {
                let Some(v) = self.state.bpd.version_for("HKKAZ", KAZ_VERSIONS) else {
                    return Err(Error::NotSupported(i18n::tr("Umsatzabruf (HKKAZ/HKCAZ)").into()));
                };
                SegmentBuilder::new("HKKAZ", v)
                    .deg(if v >= 7 { Self::kti(a) } else { Self::ktv(a) })
                    .de("N")
                    .de(fints_date(from))
                    .de(to.map(fints_date).unwrap_or(Val::Empty))
                    .de(Val::Empty)
                    .de(touchdown.clone())
            };
            let code = if use_camt { "HKCAZ" } else { "HKKAZ" };
            let resp = self.exchange_sca(vec![seg], code, false)?;
            if resp.first(if use_camt { "HICAZ" } else { "HIKAZ" }).is_none() && resp.code("3010").is_none() {
                return Err(Error::Syntax(i18n::tr("Umsatzdaten fehlen in der Bankantwort").into()));
            }
            if use_camt {
                for s in resp.find("HICAZ") {
                    for f in s.de(3).iter().flatten() {
                        let r = camt::parse(f).map_err(Error::Syntax)?;
                        out.extend(r.transactions);
                    }
                    if let Some(p) = s.bin(4, 0) {
                        if !p.is_empty() {
                            let r = camt::parse_pending(p).map_err(Error::Syntax)?;
                            out.extend(r.transactions);
                        }
                    }
                }
            } else {
                for s in resp.find("HIKAZ") {
                    if let Some(b) = s.bin(1, 0) {
                        for st in mt940::parse(b, false) {
                            out.extend(st.transactions);
                        }
                    }
                    if let Some(b) = s.bin(2, 0) {
                        for st in mt940::parse(b, true) {
                            out.extend(st.transactions);
                        }
                    }
                }
            }
            touchdown = next_page(&resp, &mut seen_pages)?;
            if touchdown.is_none() {
                break;
            }
        }
        if touchdown.is_some() {
            return Err(Error::Other(i18n::tr("Abruf unvollständig: Seitenlimit erreicht; lokale Daten wurden nicht ersetzt").into()));
        }
        Ok(TransactionReport { transactions: out, balance: None })
    }

    fn card_job(&self, a: &Account, standard: &'static str, standard_version: u32, legacy: &'static str, legacy_version: u32) -> Result<Option<(&'static str, u32)>> {
        for (code, version) in [(standard, standard_version), (legacy, legacy_version)] {
            let explicit = a.allowed.iter().any(|c| c == code);
            let offered = a.allowed.is_empty() && (50..=59).contains(&a.kind) && self.state.bpd.supports(code);
            if explicit || offered {
                // Some banks advertise proprietary card jobs only in the UPD (as AqBanking).
                if self.state.bpd.supports(code) && self.state.bpd.version_for(code, &[version]).is_none() {
                    return Err(Error::NotSupported(i18n::tr_format!("Keine gemeinsame Version für {code}")));
                }
                return Ok(Some((code, version)));
            }
        }
        Ok(None)
    }

    fn credit_card_report(&mut self, a: &Account, code: &str, version: u32, from: NaiveDate, to: Option<NaiveDate>) -> Result<TransactionReport> {
        self.ui.progress(&i18n::tr_format!("Kreditkartenumsätze {} …", display_account(a)));
        let mut report = TransactionReport::default();
        let mut touchdown: Option<String> = None;
        let mut seen_pages = std::collections::HashSet::new();
        for _ in 0..50 {
            let seg = SegmentBuilder::new(code, version)
                .deg(if code == "HKKKU" { Self::kti(a) } else { Self::ktv(a) })
                .de(&a.account_number).de(&a.sub_account)
                .de(fints_date(from)).de(to.map(fints_date).unwrap_or(Val::Empty))
                .de(Val::Empty).de(touchdown.clone());
            let resp = self.exchange_sca(vec![seg], code, false)?;
            let answer = if code == "HKKKU" { "HIKKU" } else { "DIKKU" };
            let mut found = false;
            for s in resp.find(answer) {
                found = true;
                let page = crate::credit_card::statement(s)?;
                report.transactions.extend(page.transactions);
                if page.balance.is_some() { report.balance = page.balance; }
            }
            // An explicit empty-result code is valid; a missing/unknown payload otherwise
            // must not turn an unsuccessful response into an empty local statement.
            if !found && resp.code("3010").is_none() {
                return Err(Error::Syntax(i18n::tr("Kreditkartenumsätze fehlen in der Bankantwort").into()));
            }
            touchdown = match resp.code("3040") {
                Some(code) => Some(code.params.first().filter(|p| !p.is_empty()).cloned()
                    .ok_or_else(|| Error::Syntax(i18n::tr("Aufsetzpunkt des Kreditkartenabrufs fehlt").into()))?),
                None => None,
            };
            let Some(next) = &touchdown else { return Ok(report); };
            if next.is_empty() || !seen_pages.insert(next.clone()) { break; }
        }
        Err(Error::Other(i18n::tr("Kreditkartenabruf unvollständig; lokale Umsätze wurden nicht ersetzt").into()))
    }

    fn send_order(&mut self, code: &str, seg: SegmentBuilder) -> Result<Response> {
        self.open()?;
        // orders always require SCA under PSD2, even if HIPINS is incomplete
        let response = self.exchange_sca(vec![seg], code, true)?;
        completed_approval(&response)?;
        Ok(response)
    }

    pub fn transfer(&mut self, a: &Account, t: &SepaTransfer) -> Result<String> {
        pain::validate_transfer(t)?;
        validate_debtor(a, t)?;
        self.open()?;
        let descriptor = self.state.bpd.preferred_pain001();
        let (code, desc) = if t.instant {
            if !self.state.bpd.supports("HKIPZ") {
                return Err(Error::NotSupported(i18n::tr("Echtzeitüberweisung (HKIPZ)").into()));
            }
            ("HKIPZ", descriptor)
        } else {
            ("HKCCS", descriptor)
        };
        let pain = pain::build_pain001(&desc, t, None);
        let seg = SegmentBuilder::new(code, 1).deg(Self::sepa_account(a)).de(desc).bin(pain);
        let r = self.send_order(code, seg)?;
        Ok(r.summary())
    }

    pub fn scheduled_transfer(&mut self, a: &Account, t: &SepaTransfer) -> Result<String> {
        pain::validate_transfer(t)?;
        validate_debtor(a, t)?;
        if !t.execution_date.map(|d| d > chrono::Local::now().date_naive()).unwrap_or(false) {
            return Err(Error::Other(i18n::tr("Terminüberweisung benötigt ein zukünftiges Ausführungsdatum").into()));
        }
        self.open()?;
        if !self.state.bpd.supports("HKCSE") {
            return Err(Error::NotSupported(i18n::tr("Terminüberweisung (HKCSE)").into()));
        }
        let desc = self.state.bpd.preferred_pain001();
        let pain = pain::build_pain001(&desc, t, None);
        let seg = SegmentBuilder::new("HKCSE", 1).deg(Self::sepa_account(a)).de(desc).bin(pain);
        let r = self.send_order("HKCSE", seg)?;
        let id = r.first("HICSE").map(|s| s.text_or_empty(1, 0)).unwrap_or_default();
        Ok(format!("{}{}", r.summary(), if id.is_empty() { String::new() } else { i18n::tr_format!("\nAuftrags-ID: {id}") }))
    }

    fn standing_details(so: &StandingOrder) -> Vec<Val> {
        vec![
            fints_date(so.first_date),
            if so.interval == Interval::Monthly { "M" } else { "W" }.into(),
            so.every.into(),
            so.day.into(),
            so.last_date.map(fints_date).unwrap_or(Val::Empty),
        ]
    }

    pub fn create_standing_order(&mut self, a: &Account, so: &StandingOrder) -> Result<String> {
        pain::validate_transfer(&so.transfer)?;
        validate_debtor(a, &so.transfer)?;
        pain::validate_standing_schedule(so)?;
        self.open()?;
        if !self.state.bpd.supports("HKCDE") {
            return Err(Error::NotSupported(i18n::tr("Dauerauftrag anlegen (HKCDE)").into()));
        }
        let desc = self.state.bpd.preferred_pain001();
        // standing orders use the 1999-01-01 placeholder date in the pain document
        let mut t = so.transfer.clone();
        t.execution_date = None;
        let pain = pain::build_pain001(&desc, &t, None);
        let seg = SegmentBuilder::new("HKCDE", 1)
            .deg(Self::sepa_account(a))
            .de(desc)
            .bin(pain)
            .deg(Self::standing_details(so));
        let r = self.send_order("HKCDE", seg)?;
        Ok(r.summary())
    }

    pub fn standing_orders(&mut self, a: &Account) -> Result<Vec<StandingOrder>> {
        self.open()?;
        let version = self.state.bpd.version_for("HKCDB", &[1, 2])
            .ok_or_else(|| Error::NotSupported(i18n::tr("Daueraufträge abrufen: keine gemeinsame HKCDB-Version").into()))?;
        let formats = if self.state.bpd.standing_pain001_formats.is_empty() {
            &self.state.bpd.pain001_formats
        } else { &self.state.bpd.standing_pain001_formats };
        let format = ["pain.001.001.09", "pain.001.001.03", "pain.001.003.03"].iter()
            .find_map(|known| formats.iter().find(|f| f.ends_with(known))).cloned();
        let format = if let Some(format) = format { format } else {
            if !self.state.bpd.standing_pain001_formats.is_empty() {
                return Err(Error::NotSupported(i18n::tr("Kein unterstütztes Zahlungsformat für Daueraufträge").into()));
            }
            self.state.bpd.preferred_pain001()
        };
        self.ui.progress(i18n::tr("Lade Daueraufträge …"));
        let mut out = Vec::new();
        let mut touchdown: Option<String> = None;
        let mut seen_pages = std::collections::HashSet::new();
        for _ in 0..20 {
            let seg = SegmentBuilder::new("HKCDB", version)
                .deg(Self::sepa_account(a))
                .de(&format)
                .de(Val::Empty) // Auftragsidentifikation (all orders)
                .de(Val::Empty) // max. Anzahl Einträge
                .de(touchdown.clone());
            let resp = self.exchange_sca(vec![seg], "HKCDB", false)?;
            for s in resp.find("HICDB") {
                if !matches!(s.version, 1 | 2) {
                    return Err(Error::NotSupported(i18n::tr("Unbekannte Version der Dauerauftragsantwort").into()));
                }
                let raw = s.bin(3, 0).unwrap_or_default().to_vec();
                let transfer = pain::read_pain001(&raw).ok_or_else(|| Error::Syntax(i18n::tr("Dauerauftrag enthält ungültige Zahlungsdaten").into()))?;
                let first = parse_fints_date(&s.text_or_empty(5, 0))
                    .ok_or_else(|| Error::Syntax(i18n::tr("Ungültiges erstes Ausführungsdatum im Dauerauftrag").into()))?;
                out.push(StandingOrder {
                    order_id: s.text_or_empty(4, 0),
                    transfer,
                    first_date: first,
                    interval: if s.text_or_empty(5, 1) == "W" { Interval::Weekly } else { Interval::Monthly },
                    every: s.text_or_empty(5, 2).parse().unwrap_or(1),
                    day: s.text_or_empty(5, 3).parse().unwrap_or(first.day()),
                    last_date: parse_fints_date(&s.text_or_empty(5, if s.version >= 2 { 5 } else { 4 })),
                    execution_time: if s.version >= 2 {
                        let value = s.text_or_empty(5, 4);
                        if value.is_empty() { None } else {
                            Some(chrono::NaiveTime::parse_from_str(&value, "%H%M%S")
                                .map_err(|_| Error::Syntax(i18n::tr("Ungültige Ausführungszeit im Dauerauftrag").into()))?)
                        }
                    } else { None },
                    raw_pain: raw,
                    descriptor: s.text_or_empty(2, 0),
                });
            }
            touchdown = next_page(&resp, &mut seen_pages)?;
            if touchdown.is_none() {
                break;
            }
        }
        if touchdown.is_some() {
            return Err(Error::Other(i18n::tr("Abruf unvollständig: Seitenlimit erreicht; lokale Daten wurden nicht ersetzt").into()));
        }
        Ok(out)
    }

    pub fn delete_standing_order(&mut self, a: &Account, so: &StandingOrder) -> Result<String> {
        self.open()?;
        if !self.state.bpd.supports("HKCDL") {
            return Err(Error::NotSupported(i18n::tr("Dauerauftrag löschen (HKCDL)").into()));
        }
        let desc = if so.descriptor.is_empty() { self.state.bpd.preferred_pain001() } else { so.descriptor.clone() };
        let pain = if so.raw_pain.is_empty() { pain::build_pain001(&desc, &so.transfer, None) } else { so.raw_pain.clone() };
        let seg = SegmentBuilder::new("HKCDL", 1)
            .deg(Self::sepa_account(a))
            .de(desc)
            .bin(pain)
            .de(Val::Empty) // Datum der nächsten Ausführung (unchanged)
            .de(&so.order_id)
            .deg(Self::standing_details(so));
        let r = self.send_order("HKCDL", seg)?;
        Ok(r.summary())
    }

    pub fn tan_media(&mut self) -> Result<Vec<String>> {
        self.tan_media_raw()
    }

    /// Ends the dialog and returns the persistent state.
    pub fn finish(mut self) -> State {
        if self.in_dialog {
            self.end_dialog().ok();
        }
        self.state
    }
}

fn next_page(resp: &Response, seen: &mut std::collections::HashSet<String>) -> Result<Option<String>> {
    let Some(more) = resp.code("3040") else { return Ok(None) };
    let cursor = more.params.first().filter(|p| !p.trim().is_empty())
        .ok_or_else(|| Error::Syntax(i18n::tr("Weitere Daten angekündigt, aber Aufsetzpunkt fehlt").into()))?;
    if !seen.insert(cursor.clone()) {
        return Err(Error::Syntax(i18n::tr("Wiederholter Aufsetzpunkt; Abruf ist unvollständig").into()));
    }
    Ok(Some(cursor.clone()))
}

fn merge(mut a: Response, b: Response) -> Response {
    a.segments.extend(b.segments);
    a.codes.retain(|c| !matches!(c.code.as_str(), "0030" | "3955" | "3956"));
    a.codes.extend(b.codes);
    a
}

fn completed_approval(response: &Response) -> Result<()> {
    if response.codes.iter().any(|c| matches!(c.code.as_str(), "0030" | "3955" | "3956"))
        || !response.codes.iter().any(|c| c.code.len() == 4 && c.code.starts_with('0') && c.code != "0030" && c.code.bytes().all(|b| b.is_ascii_digit())) {
        return Err(Error::Security(i18n::tr("Keine abschließende Auftragsbestätigung der Bank; Status bitte bei der Bank prüfen und Auftrag nicht erneut senden").into()));
    }
    Ok(())
}

fn validate_debtor(account: &Account, transfer: &SepaTransfer) -> Result<()> {
    let normalize = |value: &str| value.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_uppercase();
    if account.iban.is_empty() || normalize(&account.iban) != normalize(&transfer.debtor_iban) {
        return Err(Error::Security(i18n::tr("Auftraggeber-IBAN stimmt nicht mit dem ausgewählten Konto überein").into()));
    }
    Ok(())
}

fn diagnostic_code(code: &str) -> &str {
    if code.len() == 4 && code.bytes().all(|b| b.is_ascii_digit()) { code } else { i18n::tr("[ungültig]") }
}

pub fn display_account(a: &Account) -> String {
    if a.product.is_empty() {
        if a.iban.is_empty() { a.account_number.clone() } else { a.iban.clone() }
    } else {
        a.product.clone()
    }
}

/// Banks may format challenge texts with a small HTML subset.
pub fn strip_html(s: &str) -> String {
    let s = s.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n");
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
}

/// Names of the active TAN media in a HITAB answer.
fn parse_hitab(resp: &Response) -> Vec<String> {
    let mut names = Vec::new();
    for s in resp.find("HITAB") {
        // v4: class, status, card no, card seq, card type, account(4), valid from, valid to, list no, NAME
        // v5: class, status, security function, ... (one more field before the name)
        let name_idx = if s.version >= 5 { 13 } else { 12 };
        for i in 2..=s.de_count() {
            let fields: Vec<String> = s
                .de(i)
                .iter()
                .map(|f| f.as_ref().map(|b| latin1_to_string(b)).unwrap_or_default())
                .collect();
            // only active media (status 1)
            if fields.get(1).map(|st| st != "1").unwrap_or(false) {
                continue;
            }
            let looks_like_name = |t: &String| {
                t.len() > 1 && t.chars().any(|c| c.is_alphabetic()) && !crate::pain::iban_valid(t) && parse_fints_date(t).is_none()
            };
            let name = fields
                .get(name_idx)
                .filter(|t| looks_like_name(t))
                .cloned()
                .or_else(|| fields.iter().skip(3).find(|t| looks_like_name(t)).cloned());
            if let Some(n) = name {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        }
    }
    names
}

/// Keep diagnostic segment identifiers without storing credentials, account data or binary payloads.
fn protocol_summary(raw: &[u8]) -> String {
    match crate::syntax::parse_segments(raw) {
        Ok(segs) => segs.iter().take(256).map(|s| {
            // Do not echo an arbitrary malformed "segment code" into a persistent log.
            if s.code.len() == 5 && s.code.bytes().all(|c| c.is_ascii_uppercase()) {
                format!("{}:{}:{}", s.code, s.number, s.version)
            } else {
                i18n::tr("[ungültiger Segmentkopf]").into()
            }
        }).collect::<Vec<_>>().join(" "),
        Err(_) => i18n::tr_format!("[nicht lesbare Nachricht: {} Bytes]", raw.len()),
    }
}

/// Business transactions that only read account information (no orders).
fn is_account_information(code: &str) -> bool {
    matches!(code, "HKSPA" | "HKSAL" | "HKKAZ" | "HKCAZ" | "HKCDB" | "HKTAB" | "HKKAU" | "HKWPD" | "HKEKA" | "HKECA" | "HKKKU" | "DKKKU" | "HKKKS" | "DKKKS")
}


#[cfg(test)]
mod security_regressions {
    use super::*;
    struct Ui;
    impl Interaction for Ui {
        fn pin(&self, _: &str) -> Option<String> { None }
        fn choose_tan_method(&self, _: &[TanMethod]) -> Option<String> { None }
        fn choose_tan_medium(&self, _: &[String]) -> Option<String> { None }
        fn tan(&self, _: &TanChallenge) -> TanAnswer { TanAnswer::Cancel }
        fn progress(&self, _: &str) {}
    }
    #[test]
    fn pintan_requires_https_before_prompting_for_credentials() {
        for url in ["http://localhost:3000", "localhost:3000", ""] {
            assert!(Client::new(Config {url:url.into(),..Default::default()},State::default(),Medium::PinTan,&Ui).is_err());
        }
        assert!(Client::new(Config {url:"https://bank.example".into(),..Default::default()},State::default(),Medium::PinTan,&Ui).is_ok());
    }
    #[test]
    fn trace_never_contains_pin_tan_or_malformed_data() {
        let env=Envelope { dialog_id:"0",message_number:1,blz:"12345678",user_id:"user",system_id:"0" };
        let raw=build_message(&env,Auth::PinTan {pin:"secret+PIN",tan:Some("secret'TAN"),security_function:"999"},vec![]).unwrap();
        let trace=protocol_summary(&raw);
        assert!(!trace.contains("secret") && !trace.contains("user"));
        assert!(!protocol_summary(b"secretPIN").contains("secretPIN"));
    }
}

#[cfg(test)]
mod standing_regressions {
    use super::*;
    use crate::syntax::parse_segments;
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    const FORMAT: &str = "urn:iso:std:iso:20022:tech:xsd:pain.001.001.09";
    struct Ui;
    impl Interaction for Ui {
        fn pin(&self, _: &str) -> Option<String> { panic!("no credentials needed for offline fixture") }
        fn choose_tan_method(&self, _: &[TanMethod]) -> Option<String> { None }
        fn choose_tan_medium(&self, _: &[String]) -> Option<String> { None }
        fn tan(&self, _: &TanChallenge) -> TanAnswer { panic!("read-only fixture must not ask for a TAN") }
        fn progress(&self, _: &str) {}
    }
    struct Bank { version: u32, calls: Arc<AtomicUsize> }
    impl Transport for Bank {
        fn send(&mut self, message: &[u8]) -> Result<Vec<u8>> {
            let page = self.calls.fetch_add(1, Ordering::SeqCst);
            let request = open_message(message, Opener::Plain)?;
            let request = request.iter().find(|s| s.code == "HKCDB").unwrap();
            assert_eq!(request.version, self.version);
            assert_eq!(request.text_or_empty(2, 0), FORMAT);
            assert_eq!(request.de(2).len(), 1);
            assert_eq!(request.text_or_empty(5, 0), if page == 0 { "" } else { "NEXT" });
            let pain = br#"<Document><CstmrCdtTrfInitn><PmtInf><PmtTpInf><LclInstrm><Cd>INST</Cd></LclInstrm></PmtTpInf><Dbtr><Nm>Sender</Nm></Dbtr><CdtTrfTxInf><Amt><InstdAmt Ccy="EUR">12.34</InstdAmt></Amt><Cdtr><Nm>Receiver</Nm></Cdtr></CdtTrfTxInf></PmtInf></CstmrCdtTrfInitn></Document>"#;
            let mut details: Vec<Val> = vec!["20270101".into(), "M".into(), "1".into(), "1".into()];
            if self.version == 2 { details.push("083000".into()); }
            details.push("20271231".into());
            let order = SegmentBuilder::new("HICDB", self.version)
                .de(Val::Empty).de(FORMAT).bin(pain.to_vec()).de(format!("order-{page}")).deg(details);
            let status = SegmentBuilder::new("HIRMS", 2).deg(if page == 0 {
                vec!["3040".into(), Val::Empty, "More entries".into(), "NEXT".into()]
            } else {
                vec!["0010".into(), Val::Empty, "OK".into()]
            });
            let env = Envelope { dialog_id: "TEST", message_number: page as u32 + 1, blz: "12345678", user_id: "fixture", system_id: "TEST" };
            build_message(&env, Auth::None, vec![order, status])
        }
    }
    fn client(versions: &[u32], calls: Arc<AtomicUsize>, expected: u32) -> Client<'static> {
        let mut state = State::default();
        state.bpd.versions.insert("HKCDB".into(), versions.to_vec());
        state.bpd.pain001_formats = vec!["urn:iso:std:iso:20022:tech:xsd:pain.001.001.03".into()];
        let bpd = SegmentBuilder::new("HICDBS", 2).de("1").de("1").de("1")
            .deg(vec!["J".into(), Val::Empty, FORMAT.into()]);
        let parsed = Response::new(parse_segments(&bpd.encode()).unwrap());
        if versions.contains(&2) { state.bpd.update_from(&parsed); }
        else { state.bpd.pain001_formats = vec![FORMAT.into()]; }
        let mut client = Client::new(Config { url: "https://bank.invalid".into(), ..Default::default() }, state, Medium::PinTan, &Ui).unwrap();
        client.in_dialog = true;
        client.dialog_id = "TEST".into();
        client.pin = Some(zeroize::Zeroizing::new("offline-fixture".into()));
        client.set_transport(Box::new(Bank { version: expected, calls }));
        client
    }
    #[test]
    fn standing_v1_and_v2_negotiate_and_parse_all_pages() {
        for versions in [vec![1], vec![2], vec![2, 1]] {
            let expected = *versions.iter().max().unwrap();
            let calls = Arc::new(AtomicUsize::new(0));
            let mut client = client(&versions, calls.clone(), expected);
            let orders = client.standing_orders(&Account::default()).unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            assert_eq!(orders.len(), 2);
            for (i, order) in orders.iter().enumerate() {
                assert_eq!(order.order_id, format!("order-{i}"));
                assert_eq!(order.first_date.to_string(), "2027-01-01");
                assert_eq!(order.last_date.unwrap().to_string(), "2027-12-31");
                assert_eq!(order.execution_time.map(|t| t.to_string()), (expected == 2).then(|| "08:30:00".to_string()));
                assert_eq!(order.transfer.amount.to_string(), "12.34");
                assert!(order.transfer.instant);
            }
        }
    }
    #[test]
    fn unsupported_standing_version_or_format_sends_no_request() {
        for unsupported_format in [false, true] {
            let calls = Arc::new(AtomicUsize::new(0));
            let mut client = client(if unsupported_format { &[2] } else { &[3] }, calls.clone(), 0);
            if unsupported_format { client.state.bpd.standing_pain001_formats = vec!["unknown".into()]; }
            assert!(matches!(client.standing_orders(&Account::default()), Err(Error::NotSupported(_))));
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }
    }
}

#[cfg(test)]
mod credit_card_regressions {
    use super::*;
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
    struct Ui;
    impl Interaction for Ui {
        fn pin(&self, _: &str) -> Option<String> { panic!("offline fixture") }
        fn choose_tan_method(&self, _: &[TanMethod]) -> Option<String> { None }
        fn choose_tan_medium(&self, _: &[String]) -> Option<String> { None }
        fn tan(&self, _: &TanChallenge) -> TanAnswer { panic!("unexpected TAN") }
        fn progress(&self, _: &str) {}
    }
    struct Bank { standard: bool, mode: u8, calls: Arc<AtomicUsize> }
    impl Transport for Bank {
        fn send(&mut self, raw: &[u8]) -> Result<Vec<u8>> {
            let page = self.calls.fetch_add(1, Ordering::SeqCst);
            let segments = open_message(raw, Opener::Plain)?;
            assert!(!segments.iter().any(|s| s.code == "HKTAN"), "read is covered by dialog SCA");
            let code = if self.standard { "HKKKU" } else { "DKKKU" };
            let s = segments.iter().find(|s| s.code == code).unwrap();
            assert_eq!(s.version, if self.standard { 1 } else { 2 });
            assert_eq!(s.text_or_empty(1, if self.standard { 2 } else { 0 }), "test-card");
            assert_eq!(s.text_or_empty(2, 0), "test-card");
            assert_eq!(s.text_or_empty(3, 0), "sub");
            assert_eq!(s.text_or_empty(4, 0), "20261101");
            assert_eq!(s.text_or_empty(7, 0), if page == 0 { "" } else { "NEXT" });
            let entry = crate::credit_card::tests::fixture(self.standard, "D", "12,34", "J");
            let mut status = vec!["0010".into(), Val::Empty, "OK".into()];
            if page == 0 || self.mode == 1 {
                status = vec!["3040".into(), Val::Empty, "more".into()];
                if self.mode != 2 { status.push("NEXT".into()); }
            }
            let mut reply = vec![SegmentBuilder::new("HIRMS", 2).deg(status)];
            if self.mode != 3 { reply.push(entry); }
            let env = Envelope { dialog_id: "TEST", message_number: page as u32 + 1, blz: "12345678", user_id: "fixture", system_id: "TEST" };
            build_message(&env, Auth::None, reply)
        }
    }
    fn client(standard: bool, mode: u8, calls: Arc<AtomicUsize>) -> (Client<'static>, Account) {
        let code = if standard { "HKKKU" } else { "DKKKU" };
        let mut state = State { tan_method: Some("900".into()), ..Default::default() };
        state.bpd.hktan_version = 6;
        state.bpd.tan_required.insert(code.into(), true);
        let mut client = Client::new(Config { url: "https://bank.invalid".into(), ..Default::default() }, state, Medium::PinTan, &Ui).unwrap();
        client.in_dialog = true; client.dialog_authenticated = true;
        client.dialog_id = "TEST".into(); client.pin = Some(zeroize::Zeroizing::new("offline-fixture".into()));
        client.set_transport(Box::new(Bank { standard, mode, calls }));
        let account = Account { account_number: "test-card".into(), sub_account: "sub".into(), kind: 50, allowed: vec![code.into()], ..Default::default() };
        (client, account)
    }
    #[test]
    fn card_dispatch_and_paging_work_without_iban_or_proprietary_bpd() {
        for standard in [false, true] {
            let calls = Arc::new(AtomicUsize::new(0));
            let (mut client, account) = client(standard, 0, calls.clone());
            let report = client.transaction_report(&account, NaiveDate::from_ymd_opt(2026,11,1).unwrap(), None).unwrap();
            assert_eq!(report.transactions.len(), 2);
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            assert_eq!(report.balance.unwrap().booked.to_string(), "-100.50");
        }
    }
    #[test]
    fn incomplete_card_responses_never_return_partial_or_empty_success() {
        for mode in [1, 2, 3] {
            let calls = Arc::new(AtomicUsize::new(0));
            let (mut client, account) = client(true, mode, calls.clone());
            assert!(client.transaction_report(&account, NaiveDate::from_ymd_opt(2026,11,1).unwrap(), None).is_err());
            assert!(calls.load(Ordering::SeqCst) <= 2);
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let (mut client, account) = client(true, 0, calls.clone());
        client.state.bpd.versions.insert("HKKKU".into(), vec![99]);
        assert!(matches!(client.transaction_report(&account, NaiveDate::from_ymd_opt(2026,11,1).unwrap(), None), Err(Error::NotSupported(_))));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
