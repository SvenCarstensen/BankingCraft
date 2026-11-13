//! Narrow, endpoint-based interoperability rules. Capabilities and TAN methods
//! still come from BPD/UPD; a profile never grants an account permission.
//! Sources and intentionally excluded legacy workarounds: docs/bank-compatibility.md.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankProfile {
    Generic, Postbank, DeutscheBank, Comdirect, Commerzbank, Ing, Dkb,
    Sparkasse, Atruvia, Sparda, ApoBank, Norisbank, Consorsbank, Hypovereinsbank, Direkt1822,
}

impl BankProfile {
    pub fn for_endpoint(endpoint: &str) -> Self {
        let Ok(url) = url::Url::parse(endpoint) else { return Self::Generic };
        if url.scheme() != "https" { return Self::Generic; }
        match url.host_str().unwrap_or_default() {
            "hbci.postbank.de" => Self::Postbank,
            "fints.deutsche-bank.de" => Self::DeutscheBank,
            "fints.comdirect.de" => Self::Comdirect,
            "fints.commerzbank.de" => Self::Commerzbank,
            "fints.ing.de" | "fints.ing-diba.de" => Self::Ing,
            "fints.dkb.de" | "banking-dkb.s-fints-pt-dkb.de" => Self::Dkb,
            "fints1.atruvia.de" | "fints2.atruvia.de" | "hbci-pintan.gad.de" | "hbci11.fiducia.de" => Self::Atruvia,
            "fints.bankingonline.de" => Self::Sparda,
            "fints.apobank.de" => Self::ApoBank,
            "fints.norisbank.de" => Self::Norisbank,
            "brokerage-hbci.consorsbank.de" => Self::Consorsbank,
            "hbci-01.hypovereinsbank.de" => Self::Hypovereinsbank,
            "fints.1822direkt.com" => Self::Direkt1822,
            host if host.split_once('.').is_some_and(|(_, domain)| domain.starts_with("s-fints-pt-")
                && domain.ends_with(".de") && domain.matches('.').count() == 1) => Self::Sparkasse,
            _ => Self::Generic,
        }
    }

    pub fn read_only(self) -> bool { self == Self::Ing }

    /// Bank-documented history when the BPD omit StoreDays. Other banks use BPD.
    pub fn statement_history_days(self) -> Option<u32> {
        (self == Self::Ing).then_some(90)
    }
}

/// Like AqBanking's preferCamtDownload: choose before sending a job, never replay
/// a rejected request. Auto prefers CAMT, including banks migrated during 2025/26.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatementFormat {
    #[default]
    Auto,
    Camt,
    Mt940,
}

impl StatementFormat {
    pub fn use_camt(self, camt_available: bool, mt940_available: bool) -> bool {
        camt_available && (self != Self::Mt940 || !mt940_available)
    }
}

/// Only explicitly documented old public endpoints are migrated. Custom paths,
/// ports, queries and proxies are left untouched. No network redirect is followed.
pub fn migrated_endpoint(endpoint: &str) -> Option<&'static str> {
    let url = url::Url::parse(endpoint).ok()?;
    if url.scheme() != "https" || url.port().is_some() || url.query().is_some()
        || url.fragment().is_some() || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    match (url.host_str()?, url.path().trim_end_matches('/')) {
        ("fints.ing-diba.de", "/fints") => Some("https://fints.ing.de/fints/"),
        ("banking-dkb.s-fints-pt-dkb.de", "/fints30") => Some("https://fints.dkb.de/fints"),
        ("hbci-pintan.gad.de", "/cgi-bin/hbciservlet") => Some("https://fints1.atruvia.de/cgi-bin/hbciservlet"),
        ("hbci11.fiducia.de", "/cgi-bin/hbciservlet") => Some("https://fints2.atruvia.de/cgi-bin/hbciservlet"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_do_not_match_lookalike_hosts_or_tcp_cards() {
        for url in ["https://fints.ing.de.attacker.invalid/fints", "https://fints.ing.de@bank.invalid", "fints.ing.de", "http://fints.ing.de/fints"] {
            assert_eq!(BankProfile::for_endpoint(url), BankProfile::Generic);
            assert_eq!(migrated_endpoint(url), None);
        }
        assert_eq!(BankProfile::for_endpoint("https://fints.comdirect.de/fints"), BankProfile::Comdirect);
        assert_eq!(BankProfile::for_endpoint("https://fints.commerzbank.de/fints"), BankProfile::Commerzbank);
    }

    #[test]
    fn only_known_retired_endpoints_are_migrated() {
        assert_eq!(migrated_endpoint("https://hbci11.fiducia.de/cgi-bin/hbciservlet"), Some("https://fints2.atruvia.de/cgi-bin/hbciservlet"));
        assert_eq!(migrated_endpoint("https://fints.ing-diba.de/fints/"), Some("https://fints.ing.de/fints/"));
        for url in ["https://hbci11.fiducia.de/custom", "https://fints.ing-diba.de:444/fints", "https://fints.ing-diba.de/fints?custom=1", "https://hbci.postbank.de/banking/hbci.do", "fints2.atruvia.de"] {
            assert_eq!(migrated_endpoint(url), None);
        }
    }
}
