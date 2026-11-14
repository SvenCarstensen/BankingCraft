//! Read-only NVP client, independently implemented from PayPal's public API reference.
//! No redirects, wire logging or automatic retries. Never return provider text in errors.
use chrono::{DateTime, Duration, Utc};
use fints::Transaction;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet}, io::Read, str::FromStr};
use zeroize::{Zeroize, Zeroizing};

const LIVE: &str = "https://api-3t.paypal.com/nvp";
const SANDBOX: &str = "https://api-3t.sandbox.paypal.com/nvp";
const MAX_BODY: usize = 4 * 1024 * 1024;
const MAX_CALLS: usize = 2048;
type Fields = BTreeMap<String, String>;
pub type Result<T> = std::result::Result<T, String>;

#[derive(Serialize, Deserialize)]
pub struct Credentials {
    pub user: String,
    pub password: String,
    pub signature: String,
}
impl Drop for Credentials {
    fn drop(&mut self) {
        self.user.zeroize();
        self.password.zeroize();
        self.signature.zeroize();
    }
}
impl Credentials {
    pub fn validate(&self) -> Result<()> {
        if [&self.user, &self.password, &self.signature].iter().any(|s|
            s.is_empty() || s.len() > 1024 || s.chars().any(char::is_whitespace) || s.chars().any(char::is_control)) {
            return Err(i18n::tr("Bitte API-Benutzernamen, API-Passwort und API-Signatur von PayPal eingeben.").into());
        }
        Ok(())
    }
}
fn malformed() -> String { i18n::tr("Unvollständige oder ungültige PayPal-Antwort; keine Daten übernommen.").into() }
fn incomplete() -> String { i18n::tr("PayPal-Suchergebnis unvollständig; keine Daten übernommen. Bitte später erneut versuchen.").into() }

pub trait Transport {
    fn post(&mut self, endpoint: &str, body: &str) -> Result<Vec<u8>>;
}
pub struct Http(ureq::Agent);
impl Http {
    pub fn new() -> Self {
        Self(ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(60)).build())
    }
}
impl Transport for Http {
    fn post(&mut self, endpoint: &str, body: &str) -> Result<Vec<u8>> {
        let response = self.0.post(endpoint)
            .set("Content-Type", "application/x-www-form-urlencoded; charset=utf-8")
            .send_string(body).map_err(|_| i18n::tr("PayPal-Verbindung fehlgeschlagen. Es wurden keine Daten übernommen.").to_string())?;
        if response.status() != 200 { return Err(malformed()); }
        let mut data = Vec::new();
        response.into_reader().take((MAX_BODY + 1) as u64).read_to_end(&mut data)
            .map_err(|_| malformed())?;
        if data.len() > MAX_BODY { return Err(malformed()); }
        Ok(data)
    }
}

#[derive(Debug)]
pub struct Snapshot {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub balances: BTreeMap<String, Decimal>,
    pub balance_date: chrono::NaiveDate,
    pub transactions: Vec<Transaction>,
}

pub struct Client<T> { transport: T, credentials: Credentials, sandbox: bool, calls: usize }
impl<T: Transport> Client<T> {
    pub fn new(transport: T, credentials: Credentials, sandbox: bool) -> Result<Self> {
        credentials.validate()?;
        Ok(Self { transport, credentials, sandbox, calls: 0 })
    }
    fn request(&mut self, method: &str, fields: &[(&str, String)], cancelled: &impl Fn() -> bool) -> Result<Fields> {
        // Enforce the read-only contract at the only network boundary.
        if !matches!(method, "GetBalance" | "TransactionSearch") { return Err(malformed()); }
        if cancelled() { return Err(i18n::tr("Vorgang abgebrochen.").into()); }
        if self.calls >= MAX_CALLS { return Err(incomplete()); }
        self.calls += 1;
        let body = Zeroizing::new(url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([("METHOD", method), ("VERSION", "204.0"),
                ("USER", &self.credentials.user), ("PWD", &self.credentials.password),
                ("SIGNATURE", &self.credentials.signature)])
            .extend_pairs(fields.iter().map(|(k, v)| (*k, v.as_str()))).finish());
        let bytes = Zeroizing::new(self.transport.post(if self.sandbox { SANDBOX } else { LIVE }, &body)?);
        let response = parse(&bytes)?;
        let codes: Vec<_> = response.iter().filter(|(k, _)| k.starts_with("L_ERRORCODE")).map(|(_, v)| v.as_str()).collect();
        let success = matches!(response.get("ACK").map(String::as_str), Some("Success" | "SuccessWithWarning"));
        if !success || codes.iter().any(|c| *c != "11002") || (!codes.is_empty() && method != "TransactionSearch") {
            // Codes are restricted to digits; neither remote prose nor credentials reach the UI/logs.
            let safe = codes.into_iter().filter(|s| !s.is_empty() && s.len() <= 8 && s.bytes().all(|c| c.is_ascii_digit())).collect::<Vec<_>>().join(", ");
            return Err(i18n::tr_format!("PayPal hat den Abruf abgelehnt (API-Code: {}). Bitte Berechtigungen und API-Zugangsdaten prüfen.", safe));
        }
        if response.get("ACK").is_some_and(|v| v == "SuccessWithWarning") && codes.is_empty() { return Err(malformed()); }
        Ok(response)
    }
    pub fn fetch(&mut self, from: DateTime<Utc>, to: DateTime<Utc>, currencies: Option<&BTreeSet<String>>, cancelled: impl Fn() -> bool) -> Result<Snapshot> {
        if from >= to { return Err(malformed()); }
        let mut transactions = BTreeMap::new();
        // Currency-scoped refreshes only search selected wallet currencies.
        if let Some(currencies) = currencies {
            for currency in currencies { self.search(from, to, Some(currency), &cancelled, &mut transactions)?; }
        } else { self.search(from, to, None, &cancelled, &mut transactions)?; }
        let response = self.request("GetBalance", &[("RETURNALLCURRENCIES", "1".into())], &cancelled)?;
        let balance_date = timestamp(required(&response, "TIMESTAMP")?)?.date_naive();
        let mut balances = BTreeMap::new();
        for i in indices(&response, "L_AMT")? {
            let currency = currency(required(&response, &format!("L_CURRENCYCODE{i}"))?)?;
            let amount = amount(required(&response, &format!("L_AMT{i}"))?)?;
            if balances.insert(currency, amount).is_some() { return Err(malformed()); }
        }
        if balances.is_empty() { return Err(malformed()); }
        if cancelled() { return Err(i18n::tr("Vorgang abgebrochen.").into()); }
        Ok(Snapshot { from, to, balance_date, balances, transactions: transactions.into_values().collect() })
    }
    fn search(&mut self, from: DateTime<Utc>, to: DateTime<Utc>, currency_filter: Option<&str>, cancelled: &impl Fn() -> bool, out: &mut BTreeMap<String, Transaction>) -> Result<()> {
        let mut fields = vec![("STARTDATE", from.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)), ("ENDDATE", to.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))];
        if let Some(c) = currency_filter { fields.push(("CURRENCYCODE", c.into())); }
        let response = self.request("TransactionSearch", &fields, cancelled)?;
        let rows = indices(&response, "L_TRANSACTIONID")?;
        let truncated = rows.len() >= 100 || response.iter().any(|(k, v)| k.starts_with("L_ERRORCODE") && v == "11002");
        if truncated {
            let seconds = (to - from).num_seconds();
            if seconds <= 1 { return Err(incomplete()); }
            let middle = from + Duration::seconds(seconds / 2);
            // Overlap the boundary: PayPal's endpoints are inclusive. Stable IDs remove duplicates.
            self.search(from, middle, currency_filter, cancelled, out)?;
            self.search(middle, to, currency_filter, cancelled, out)?;
            return Ok(());
        }
        for i in rows {
            let get = |key: &str| required(&response, &format!("{key}{i}"));
            let id = get("L_TRANSACTIONID")?;
            if id.is_empty() || id.len() > 128 || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') { return Err(malformed()); }
            let time = timestamp(get("L_TIMESTAMP")?)?;
            if time < from || time > to { return Err(malformed()); }
            let currency = currency(get("L_CURRENCYCODE")?)?;
            if currency_filter.is_some_and(|c| c != currency) { return Err(malformed()); }
            let net = amount(get("L_NETAMT")?)?;
            let gross = amount(get("L_AMT")?)?;
            let fee = amount(get("L_FEEAMT")?)?;
            let kind = clean(get("L_TYPE")?)?;
            if kind.is_empty() { return Err(malformed()); }
            let status = get("L_STATUS")?;
            // Orders/authorizations/invoices are not posted payments. Unknown types
            // fail closed instead of contaminating financial totals.
            if matches!(kind.as_str(), "Order" | "Authorization" | "Invoice" | "Money Request" | "Subscription") { continue; }
            if !matches!(kind.as_str(), "Currency Conversion (credit)" | "Currency Conversion (debit)" | "Payment" | "Recurring Payment" | "Temporary Hold" | "Transfer" | "Donation" | "Refund" | "Reversal" | "Fee" | "Bank Transfer" | "Withdrawal") { return Err(malformed()); }
            if kind == "Temporary Hold" && matches!(status, "Reversed" | "Removed" | "Completed") { continue; }
            let pending = match status {
                "Pending" | "Processing" | "In-Progress" => true,
                "Completed" | "Refunded" | "Partially-Refunded" | "PartiallyRefunded" | "Reversed" => false,
                "Denied" | "Failed" | "Voided" | "Expired" | "Canceled" | "Cancelled" | "Removed" => continue,
                _ => return Err(malformed()),
            };
            // Distinct currency-conversion legs may share a transaction ID.
            let fingerprint = format!("paypal:{id}:{currency}:{kind}");
            let purpose = format!("PayPal {id} · {status} · Gross: {gross} {currency} · Fee: {fee} {currency}");
            let name = response.get(&format!("L_NAME{i}")).filter(|s| !s.is_empty())
                .or_else(|| response.get(&format!("L_EMAIL{i}"))).map(|s| clean(s)).transpose()?.unwrap_or_else(|| "PayPal".into());
            let tx = Transaction { booking_date: time.date_naive(), value_date: time.date_naive(), amount: net,
                currency, counterparty_name: name, purpose, posting_text: kind, end_to_end_id: id.into(), pending,
                fingerprint: fingerprint.clone(), ..Default::default() };
            if let Some(previous) = out.insert(fingerprint, tx.clone()) {
                // A changing record during pagination needs a fresh consistent retrieval.
                if previous != tx { return Err(incomplete()); }
            }
        }
        Ok(())
    }
}
fn clean(s: &str) -> Result<String> {
    if s.len() > 8192 || s.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) { return Err(malformed()); }
    Ok(s.split_whitespace().collect::<Vec<_>>().join(" "))
}
fn required<'a>(fields: &'a Fields, key: &str) -> Result<&'a str> { fields.get(key).map(String::as_str).ok_or_else(malformed) }
fn currency(s: &str) -> Result<String> {
    if s.len() != 3 || !s.bytes().all(|b| b.is_ascii_uppercase()) { return Err(malformed()); }
    Ok(s.into())
}
fn amount(s: &str) -> Result<Decimal> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    if digits.is_empty() || digits.starts_with('.') || digits.ends_with('.') || digits.len() > 28 || !digits.bytes().all(|c| c.is_ascii_digit() || c == b'.') || digits.matches('.').count() > 1 || !digits.bytes().any(|c| c.is_ascii_digit()) { return Err(malformed()); }
    Decimal::from_str_exact(s).map_err(|_| malformed())
}
fn timestamp(s: &str) -> Result<DateTime<Utc>> { DateTime::from_str(s).map_err(|_| malformed()) }
fn indices(fields: &Fields, prefix: &str) -> Result<Vec<usize>> {
    let mut result = BTreeSet::new();
    for key in fields.keys().filter(|k| k.starts_with(prefix)) {
        let suffix = &key[prefix.len()..];
        let index = suffix.parse::<usize>().map_err(|_| malformed())?;
        if suffix != index.to_string() || index > 1000 { return Err(malformed()); }
        result.insert(index);
    }
    if result.iter().copied().ne(0..result.len()) { return Err(malformed()); }
    let row_prefixes: &[&str] = if prefix == "L_TRANSACTIONID" {
        &["L_TIMESTAMP", "L_TIMEZONE", "L_TYPE", "L_EMAIL", "L_NAME", "L_STATUS", "L_AMT", "L_CURRENCYCODE", "L_FEEAMT", "L_NETAMT"]
    } else { &["L_CURRENCYCODE"] };
    for key in fields.keys() {
        if let Some(suffix) = row_prefixes.iter().find_map(|p| key.strip_prefix(p)) {
            let index = suffix.parse::<usize>().map_err(|_| malformed())?;
            if suffix != index.to_string() || !result.contains(&index) { return Err(malformed()); }
        }
    }
    // A missing ID must never turn a non-empty result into a successful empty history.
    if result.is_empty() && fields.keys().any(|k| k.starts_with("L_") && !k.starts_with("L_ERROR") && !k.starts_with("L_SHORTMESSAGE") && !k.starts_with("L_LONGMESSAGE") && !k.starts_with("L_SEVERITYCODE")) { return Err(malformed()); }
    Ok(result.into_iter().collect())
}
fn decode(s: &[u8]) -> Result<String> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'+' => out.push(b' '),
            b'%' => {
                if i + 2 >= s.len() { return Err(malformed()); }
                let a = (s[i + 1] as char).to_digit(16).ok_or_else(malformed)?;
                let b = (s[i + 2] as char).to_digit(16).ok_or_else(malformed)?;
                out.push((a * 16 + b) as u8);
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8(out).map_err(|_| malformed())
}
fn parse(bytes: &[u8]) -> Result<Fields> {
    if bytes.is_empty() || bytes.len() > MAX_BODY { return Err(malformed()); }
    let mut fields = Fields::new();
    for pair in bytes.split(|c| *c == b'&') {
        let pos = pair.iter().position(|c| *c == b'=').ok_or_else(malformed)?;
        let key = decode(&pair[..pos])?;
        if key.is_empty() || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') { return Err(malformed()); }
        if fields.insert(key, decode(&pair[pos + 1..])?).is_some() { return Err(malformed()); }
    }
    Ok(fields)
}

#[cfg(test)]
mod tests;
