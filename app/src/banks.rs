//! Embedded directory of German banks with their FinTS endpoints, searchable by
//! name, city, BLZ or BIC.

pub(crate) mod profile;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct Bank {
    pub blz: &'static str,
    pub name: &'static str,
    pub city: &'static str,
    pub bic: &'static str,
    /// Host for HBCI chip card / key file access (TCP port 3000).
    pub host: &'static str,
    /// HTTPS endpoint for PIN/TAN.
    pub url: &'static str,
    norm: String,
}

const DATA: &str = include_str!("../data/banks.tsv");

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .replace('ä', "ae")
        .replace('ö', "oe")
        .replace('ü', "ue")
        .replace('ß', "ss")
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect()
}

pub fn all() -> &'static [Bank] {
    static BANKS: OnceLock<Vec<Bank>> = OnceLock::new();
    BANKS.get_or_init(|| {
        DATA.lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| {
                let f: Vec<&'static str> = l.split('\t').map(str::trim).collect();
                if f.len() < 6 {
                    return None;
                }
                Some(Bank {
                    blz: f[0],
                    name: f[1],
                    city: f[2],
                    bic: f[3],
                    host: f[4],
                    url: f[5],
                    norm: normalize(&format!("{} {} {} {}", f[1], f[2], f[0], f[3])),
                })
            })
            .collect()
    })
}

pub fn by_blz(blz: &str) -> Option<&'static Bank> {
    all().iter().find(|b| b.blz == blz)
}

/// Search by bank name, city, BLZ or BIC. Every word must match; branches of the
/// same institute that share an endpoint are collapsed into one hit.
pub fn search(query: &str, chip_card: bool, limit: usize) -> Vec<&'static Bank> {
    let q = normalize(query);
    let words: Vec<&str> = q.split_whitespace().collect();
    if words.is_empty() || (words.len() == 1 && words[0].len() < 2) {
        return vec![];
    }
    let digits: String = query.chars().filter(|c| c.is_ascii_digit()).collect();
    let mut hits: Vec<(i32, &'static Bank)> = all()
        .iter()
        .filter(|b| if chip_card { !b.host.is_empty() } else { !b.url.is_empty() })
        .filter(|b| words.iter().all(|w| b.norm.contains(w)))
        .map(|b| {
            let name = normalize(b.name);
            let mut score = 0;
            if digits.len() == 8 && b.blz == digits {
                score += 1000;
            } else if digits.len() >= 3 && b.blz.starts_with(&digits) {
                score += 200;
            }
            if name.starts_with(words[0]) {
                score += 100;
            }
            if name.split_whitespace().any(|t| t == words[0]) {
                score += 50;
            }
            // main offices (BIC branch code XXX) before branches
            if b.bic.ends_with("XXX") {
                score += 30;
            }
            score -= (b.name.len() as i32) / 8;
            (score, b)
        })
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(b.1.name)));
    let mut out: Vec<&'static Bank> = Vec::new();
    for (_, b) in hits {
        let dup = out.iter().any(|o| {
            let same_endpoint = if chip_card { o.host == b.host } else { o.url == b.url };
            same_endpoint && normalize(o.name) == normalize(b.name)
        });
        if !dup {
            out.push(b);
        }
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// Bank specific hints for the login name.
pub fn login_hint(b: &Bank) -> Option<&'static str> {
    use crate::banks::profile::BankProfile;
    match b.blz {
        "36020030" => return Some(i18n::tr("NATIONAL-BANK: Nach der IT-Umstellung den neuen NetKey und die neue PIN verwenden. Alte RDH-Schlüssel müssen bei der Bank neu eingerichtet werden.")),
        "20120100" | "10030500" | "25060180" | "29030400" | "60020100" => return Some(i18n::tr("Warburg: Seit Mai 2026 die zentrale BLZ 20120100, den neuen Warburg NetKey und die neue PIN verwenden. Alte Filialzugänge und HBCI-Signaturen müssen neu eingerichtet werden.")),
        "20030400" => return Some(i18n::tr("Marcard, Stein & Co: Seit Mai 2026 die neuen Onlinebanking-Zugangsdaten verwenden. HBCI-Zugangsdaten für das neue System erhalten Sie von der Bank.")),
        _ => {},
    }
    Some(i18n::tr(match BankProfile::for_endpoint(b.url) {
        BankProfile::Postbank => "Anmeldename: Ihre Postbank ID, ggf. mit Profil (PostbankID#Profilname); Aufträge in BestSign freigeben",
        BankProfile::DeutscheBank => "Deutsche Bank: Deutsche Bank ID oder Filial- und Kontonummer, dazu Onlinebanking-Passwort. Nach dem Banking-Upgrade neue Zugangsdaten verwenden; Freigabe per photoTAN oder BestSign.",
        BankProfile::Ing => "ING: Benutzername oder Zugangsnummer und Onlinebanking-Passwort; Kunden-ID leer lassen. Nur Lesezugriff, Girokonto-Umsätze bis 90 Tage. Alle 90 Tage im ING-Onlinebanking anmelden.",
        BankProfile::Dkb => "DKB: Anmeldename wie im Onlinebanking; Kunden-ID leer lassen. Freigabe über DKB-App, chipTAN manuell oder chipTAN QR.",
        BankProfile::Comdirect => "comdirect: 8-stellige Zugangsnummer und Onlinebanking-PIN.",
        BankProfile::Commerzbank => "Commerzbank: Teilnehmernummer oder Benutzername. FinTS mit photoTAN muss separat bei der Bank freigeschaltet sein. Umsätze werden bevorzugt als CAMT abgerufen.",
        BankProfile::Sparkasse => "Sparkasse: Anmeldename oder Legitimations-ID und Onlinebanking-PIN. Die Bank liefert die verfügbaren pushTAN- und chipTAN-Verfahren.",
        BankProfile::Atruvia if b.name.contains("Sparda") => "Sparda: Aktuellen Sparda-NetKey oder Alias und die neue Online-PIN verwenden. Nach einer IT-Umstellung zuerst im Onlinebanking anmelden und das Freigabeverfahren aktivieren.",
        BankProfile::Atruvia => "VR-/PSD-/Atruvia-Zugang: NetKey oder Alias und Onlinebanking-PIN verwenden; abweichende Kennungen Ihrer Bank beachten. SecureGo plus und Smart-TAN werden aus den Bankparametern ermittelt.",
        BankProfile::Sparda => "Sparda: FinTS-Zugangsdaten Ihrer regionalen Bank verwenden. Server und TAN-Verfahren unterscheiden sich zwischen den Sparda-Banken.",
        BankProfile::ApoBank => "apoBank: apoKennung oder Alias verwenden. Verfügbare TAN-Verfahren werden von der Bank geladen; der frühere Atruvia-Zugang ist veraltet.",
        BankProfile::Norisbank => "norisbank: Neue norisbank ID oder Filial- und Kontonummer sowie aktuelles Passwort verwenden. Kunden-ID leer lassen oder identisch eintragen. Freigabe per photoTAN oder BestSign.",
        BankProfile::Consorsbank => "Consorsbank: Kontonummer gefolgt von der dreistelligen Berechtigtennummer als Benutzerkennung. Die Bank verlangt eine TAN beim Login. FinTS 4.x wird derzeit nicht unterstützt.",
        BankProfile::Hypovereinsbank => "HypoVereinsbank: 10-stellige Direct-Banking-Nummer und den freigeschalteten FinTS-Zugang verwenden. Für PIN/TAN die zentrale BLZ 70020270 wählen.",
        BankProfile::Direkt1822 => "1822direkt: FinTS-Zugang mit BLZ 50050222 verwenden. Benutzerkennung und freigeschaltetes TAN-Verfahren den Bankunterlagen entnehmen.",
        BankProfile::Generic => return None,
    }))
}

/// HIUPD often has an IBAN but no BIC; some banks omit HKSPA entirely. Use the
/// exact German account BLZ from a checksum-valid IBAN, never the login's BIC or
/// a name match. Bank-supplied data always take precedence.
pub fn complete_account(account: &mut banking::Account) {
    if !account.iban.starts_with("DE")
        || account.iban.len() != 22 || !banking::validation::iban_valid(&account.iban) { return; }
    let Some(blz) = account.iban.get(4..12) else { return };
    if !account.blz.is_empty() && account.blz != blz { return; }
    account.blz = blz.into();
    if account.bic.is_empty() {
        if let Some(bank) = by_blz(blz) { account.bic = bank.bic.into(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn november_fourteenth_review_preserves_current_endpoints_and_trims_outer_whitespace() {
        assert_eq!(by_blz("25040066").unwrap().host, "hbci.commerzbank.de");
        assert_eq!(by_blz("36020030").unwrap().host, "fints2.atruvia.de");
        assert_eq!(by_blz("30010444").unwrap().url, "https://banking.ikb.de/eBankingFinTS/");
        assert_eq!(by_blz("30520037").unwrap().url, "https://fints1.atruvia.de/cgi-bin/hbciservlet");
        for blz in ["20120100", "20030400", "33060592", "36060591", "37060590", "40060560", "50090500", "60090800", "70090500", "76090500"] {
            assert_eq!(by_blz(blz).unwrap().url, "https://fints2.atruvia.de/cgi-bin/hbciservlet");
        }
        for blz in ["36020030", "20120100", "20030400"] { assert!(login_hint(by_blz(blz).unwrap()).is_some()); }
        for bank in all() { assert_eq!(bank.host, bank.host.trim()); assert_eq!(bank.url, bank.url.trim()); }
        assert!(by_blz("49999924").is_none());
    }

    #[test]
    fn finds_common_banks() {
        assert!(all().len() > 2000);
        let ing = search("ing", false, 5);
        assert!(ing.iter().any(|b| b.url.contains("fints.ing.de")));
        let foerde = search("förde sparkasse", false, 5);
        assert_eq!(foerde[0].blz, "21050170");
        let blz = search("12030000", false, 5);
        assert_eq!(blz[0].name, "Deutsche Kreditbank Berlin");
        assert!(by_blz("21661719").unwrap().url.contains("atruvia"));
    }

    #[test]
    fn verified_sparda_west_branches_use_the_new_server() {
        for blz in ["33060592", "36060591", "37060590", "40060560"] {
            let bank = by_blz(blz).unwrap();
            assert_eq!(bank.url, "https://fints2.atruvia.de/cgi-bin/hbciservlet");
            assert!(bank.name.contains("West"));
            assert!(login_hint(bank).is_some());
        }
        for blz in ["12030000", "30060601"] { assert!(by_blz(blz).unwrap().host.is_empty()); }
    }

    #[test]
    fn bic_fallback_uses_the_account_bank_and_preserves_explicit_data() {
        // Generated fixture IBAN: account number 1, no customer data.
        for (blz, bic) in [("12030000", "BYLADEM1001"), ("30520037", "RCIDDE3NPAY"), ("50050201", "HELADEF1822")] {
            let digits = format!("{blz}0000000001131400");
            let rem = digits.bytes().fold(0u32, |acc, b| (acc * 10 + u32::from(b - b'0')) % 97);
            let mut account = banking::Account { iban: format!("DE{:02}{blz}0000000001", 98 - rem), ..Default::default() };
            complete_account(&mut account);
            assert_eq!(account.bic, bic); assert_eq!(account.blz, blz);
            account.bic = "BANKSUPPLIED".into(); complete_account(&mut account); assert_eq!(account.bic, "BANKSUPPLIED");
            account.bic.clear(); account.blz = "99999999".into(); complete_account(&mut account); assert!(account.bic.is_empty());
        }
        for iban in ["DE00120300000000000001", "GB82WEST12345698765432", "DE😃", ""] {
            let mut account = banking::Account { iban: iban.into(), ..Default::default() };
            complete_account(&mut account); assert!(account.bic.is_empty());
        }
    }
}
