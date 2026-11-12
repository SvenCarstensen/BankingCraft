//! High level FinTS client: dialog handling, strong customer authentication
//! (HKTAN process 4 / 2 / S) and the business transactions used by the app.

use crate::bpd::{Bpd, Upd};
use crate::error::{Error, Result};
use crate::model::*;
use crate::response::Response;
use crate::security::{build_message, open_message, Auth, DdvCard, DdvKey, Envelope};
use crate::syntax::{fints_date, latin1_to_string, parse_fints_date, SegmentBuilder, Val};
use crate::transport::{HttpsTransport, TcpTransport, Transport};
use crate::{camt, mt940, pain};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

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
}

pub enum Medium {
    PinTan,
    Ddv(Box<dyn DdvCard>),
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
    transport: Box<dyn Transport>,
    ui: &'a dyn Interaction,
    pin: Option<String>,
    dialog_id: String,
    msg_num: u32,
    in_dialog: bool,
    /// Strong customer authentication completed (or waived) at dialog initialisation.
    dialog_authenticated: bool,
    /// (segment number, segment code) of the last request, to name segments in error messages.
    last_request: Vec<(u32, String)>,
    /// Last request/response pair with PIN and TAN masked, for error reports.
    pub last_trace: Option<String>,
}

const SAL_VERSIONS: &[u32] = &[5, 6, 7];
const ANONYMOUS_CUSTOMER: &str = "9999999999";
const KAZ_VERSIONS: &[u32] = &[5, 6, 7];

impl<'a> Client<'a> {
    pub fn new(cfg: Config, state: State, mut medium: Medium, ui: &'a dyn Interaction) -> Result<Self> {
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

    fn security_function(&self) -> String {
        self.state.tan_method.clone().unwrap_or_else(|| "999".into())
    }

    fn ensure_pin(&mut self) -> Result<String> {
        if let Some(p) = &self.pin {
            return Ok(p.clone());
        }
        let p = self
            .ui
            .pin(&format!("PIN für {} ({})", self.cfg.user_id, self.state.bpd.bank_name))
            .ok_or(Error::Cancelled)?;
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
            let auth = match (&mut self.medium, &self.ddv) {
                (Medium::PinTan, _) => Auth::PinTan { pin: pin.as_deref().unwrap_or(""), tan, security_function: &sf },
                (Medium::Ddv(card), Some(ctx)) => Auth::Ddv {
                    card: card.as_mut(),
                    cid: &ctx.cid,
                    sig_key: ctx.sig_key,
                    enc_key: ctx.enc_key,
                },
                _ => return Err(Error::Security("chip card not initialised".into())),
            };
            build_message(&env, auth, segments)?
        };
        self.msg_num += 1;
        let answer = self.transport.send(&raw)?;
        self.last_trace = Some(format!(">> {}\n\n<< {}", mask_signature(&raw), truncate_trace(&answer)));
        let segs = match (&mut self.medium, &self.ddv) {
            (Medium::Ddv(card), Some(ctx)) => open_message(&answer, Some((card.as_mut(), ctx.enc_key.num)))?,
            _ => open_message(&answer, None)?,
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
            log::info!("FinTS {} {}", c.code, c.text);
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
            (Some(sg), Some(el)) => out.push_str(&format!(" ({sg}, Element {el})")),
            (Some(sg), None) => out.push_str(&format!(" ({sg})")),
            (None, Some(el)) => out.push_str(&format!(" (Element {el})")),
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
        self.last_trace = Some(format!(">> {}\n\n<< {}", crate::syntax::latin1_to_string(&raw), truncate_trace(&answer)));
        let resp = Response::new(open_message(&answer, None)?);
        if let Some(id) = &resp.dialog_id {
            if self.dialog_id == "0" && id != "0" {
                self.dialog_id = id.clone();
            }
        }
        self.state.bpd.update_from(&resp);
        for c in &resp.codes {
            log::info!("FinTS (anonym) {} {}", c.code, c.text);
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
        self.ui.progress("Lade Bankparameter …");
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
        if self.cfg.product_id.is_empty() { "KONTOR".to_string() } else { self.cfg.product_id.clone() }
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
        if !sca || resp.code("0030").is_none() {
            return Ok(resp);
        }
        let hitan = resp.first("HITAN").ok_or_else(|| Error::Other("TAN required but no HITAN".into()))?;
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
                    let seg = self.hktan_followup("2", &task_reference);
                    let r = self.exchange(vec![seg], Some(&tan))?;
                    return Ok(merge(resp, r));
                }
                TanAnswer::Confirmed => {
                    let process = if self.hktan_version() >= 7 { "S" } else { "2" };
                    let seg = self.hktan_followup(process, &task_reference);
                    let r = self.exchange(vec![seg], None)?;
                    if r.code("3956").is_some() {
                        challenge.text = format!(
                            "Die Freigabe ist noch nicht erfolgt. Bitte bestätigen Sie den Auftrag in Ihrer App.\n\n{}",
                            strip_html(&challenge.text)
                        );
                        continue;
                    }
                    return Ok(merge(resp, r));
                }
            }
        }
    }

    // ------------------------------------------------------------------ dialogs

    fn init_segments(&self, with_sync: bool) -> Vec<SegmentBuilder> {
        let pintan = self.is_pintan();
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
        if !self.is_pintan() {
            return Ok(());
        }
        self.ui.progress("Synchronisiere Bankzugang …");
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
            if self.state.bpd.tan_methods.is_empty() {
                if let Err(e) = self.fetch_bpd_anonymous() {
                    log::warn!("anonymous BPD request failed: {e}");
                }
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
        self.ui.progress("Dialog wird eröffnet …");
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
            self.ui.progress("Lade TAN-Medien …");
            let saved = self.state.tan_method.replace("999".into());
            self.reset_dialog();
            let segs = self.init_segments(false);
            let r = self
                .exchange(segs, None)
                .and_then(|_| self.exchange(vec![SegmentBuilder::new("HKTAB", v).de("0").de("A")], None));
            match r {
                Ok(resp) => media = parse_hitab(&resp),
                Err(e) => log::warn!("TAN media lookup failed: {e}"),
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
                "TAN-Medium",
                &format!(
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
        self.open()?;
        if let Some(v) = self.state.bpd.version_for("HKSPA", &[1, 2, 3]) {
            self.ui.progress("Lade SEPA-Kontoverbindungen …");
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
        let Some(v) = self.state.bpd.version_for("HKSAL", SAL_VERSIONS) else {
            return Err(Error::NotSupported("Saldenabfrage (HKSAL)".into()));
        };
        self.ui.progress(&format!("Saldo {} …", display_account(a)));
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
        self.open()?;
        let camt_fmt = self.state.bpd.preferred_camt();
        let use_camt = camt_fmt.is_some() && self.state.bpd.version_for("HKCAZ", &[1]).is_some() && a.is_sepa();
        self.ui.progress(&format!("Umsätze {} …", display_account(a)));
        let mut out = Vec::new();
        let mut touchdown: Option<String> = None;
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
                    return Err(Error::NotSupported("Umsatzabruf (HKKAZ/HKCAZ)".into()));
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
            if use_camt {
                for s in resp.find("HICAZ") {
                    for f in s.de(3).iter().flatten() {
                        if let Ok(r) = camt::parse(f) {
                            out.extend(r.transactions);
                        }
                    }
                    if let Some(p) = s.bin(4, 0) {
                        if let Ok(r) = camt::parse(p) {
                            out.extend(r.transactions.into_iter().map(|mut t| {
                                t.pending = true;
                                t
                            }));
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
            touchdown = resp.codes.iter().find(|c| c.code == "3040").and_then(|c| c.params.first().cloned());
            if touchdown.is_none() {
                break;
            }
        }
        Ok(out)
    }

    fn send_order(&mut self, code: &str, seg: SegmentBuilder) -> Result<Response> {
        self.open()?;
        // orders always require SCA under PSD2, even if HIPINS is incomplete
        self.exchange_sca(vec![seg], code, true)
    }

    pub fn transfer(&mut self, a: &Account, t: &SepaTransfer) -> Result<String> {
        self.open()?;
        let descriptor = self.state.bpd.preferred_pain001();
        let (code, desc) = if t.instant {
            if !self.state.bpd.supports("HKIPZ") {
                return Err(Error::NotSupported("Echtzeitüberweisung (HKIPZ)".into()));
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
        self.open()?;
        if !self.state.bpd.supports("HKCSE") {
            return Err(Error::NotSupported("Terminüberweisung (HKCSE)".into()));
        }
        let desc = self.state.bpd.preferred_pain001();
        let pain = pain::build_pain001(&desc, t, None);
        let seg = SegmentBuilder::new("HKCSE", 1).deg(Self::sepa_account(a)).de(desc).bin(pain);
        let r = self.send_order("HKCSE", seg)?;
        let id = r.first("HICSE").map(|s| s.text_or_empty(1, 0)).unwrap_or_default();
        Ok(format!("{}{}", r.summary(), if id.is_empty() { String::new() } else { format!("\nAuftrags-ID: {id}") }))
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
        self.open()?;
        if !self.state.bpd.supports("HKCDE") {
            return Err(Error::NotSupported("Dauerauftrag anlegen (HKCDE)".into()));
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
        if !self.state.bpd.supports("HKCDB") {
            return Err(Error::NotSupported("Daueraufträge abrufen (HKCDB)".into()));
        }
        self.ui.progress("Lade Daueraufträge …");
        let mut out = Vec::new();
        let mut touchdown: Option<String> = None;
        for _ in 0..20 {
            let seg = SegmentBuilder::new("HKCDB", 1)
                .deg(Self::sepa_account(a))
                .deg(self.state.bpd.pain001_formats.iter().map(|f| Val::from(f.as_str())).collect())
                .de(Val::Empty) // Auftragsidentifikation (all orders)
                .de(Val::Empty) // max. Anzahl Einträge
                .de(touchdown.clone());
            let resp = self.exchange_sca(vec![seg], "HKCDB", false)?;
            for s in resp.find("HICDB") {
                let raw = s.bin(3, 0).unwrap_or_default().to_vec();
                let Some(transfer) = pain::read_pain001(&raw) else { continue };
                let first = parse_fints_date(&s.text_or_empty(5, 0)).unwrap_or_else(|| chrono::Local::now().date_naive());
                out.push(StandingOrder {
                    order_id: s.text_or_empty(4, 0),
                    transfer,
                    first_date: first,
                    interval: if s.text_or_empty(5, 1) == "W" { Interval::Weekly } else { Interval::Monthly },
                    every: s.text_or_empty(5, 2).parse().unwrap_or(1),
                    day: s.text_or_empty(5, 3).parse().unwrap_or(first.day()),
                    last_date: parse_fints_date(&s.text_or_empty(5, 4)),
                    raw_pain: raw,
                    descriptor: s.text_or_empty(2, 0),
                });
            }
            touchdown = resp.codes.iter().find(|c| c.code == "3040").and_then(|c| c.params.first().cloned());
            if touchdown.is_none() {
                break;
            }
        }
        Ok(out)
    }

    pub fn delete_standing_order(&mut self, a: &Account, so: &StandingOrder) -> Result<String> {
        self.open()?;
        if !self.state.bpd.supports("HKCDL") {
            return Err(Error::NotSupported("Dauerauftrag löschen (HKCDL)".into()));
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

fn merge(mut a: Response, b: Response) -> Response {
    a.segments.extend(b.segments);
    a.codes.extend(b.codes);
    a
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

/// Replace the user signature (PIN/TAN) in a raw message for logging.
fn mask_signature(raw: &[u8]) -> String {
    let text = latin1_to_string(raw);
    let Some(start) = text.find("HNSHA:") else { return text };
    // end of segment = first unescaped apostrophe
    let bytes: Vec<char> = text[start..].chars().collect();
    let mut end = bytes.len();
    let mut k = 0;
    while k < bytes.len() {
        match bytes[k] {
            '?' => k += 2,
            '\'' => {
                end = k;
                break;
            }
            _ => k += 1,
        }
    }
    let seg: String = bytes[..end].iter().collect();
    let masked = match seg.rfind('+') {
        Some(p) => format!("{}+*****", &seg[..p]),
        None => seg.clone(),
    };
    format!("{}{}{}", &text[..start], masked, bytes[end..].iter().collect::<String>())
}

fn truncate_trace(raw: &[u8]) -> String {
    let t = latin1_to_string(raw);
    if t.chars().count() > 20_000 {
        format!("{} … [gekürzt]", t.chars().take(20_000).collect::<String>())
    } else {
        t
    }
}

/// Business transactions that only read account information (no orders).
fn is_account_information(code: &str) -> bool {
    matches!(code, "HKSPA" | "HKSAL" | "HKKAZ" | "HKCAZ" | "HKCDB" | "HKTAB" | "HKKAU" | "HKWPD" | "HKEKA" | "HKECA")
}
