//! Endpoint classification for user-facing setup hints only.

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

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_do_not_match_lookalike_hosts_or_tcp_cards() {
        for url in ["https://fints.ing.de.attacker.invalid/fints", "https://fints.ing.de@bank.invalid", "fints.ing.de", "http://fints.ing.de/fints"] {
            assert_eq!(BankProfile::for_endpoint(url), BankProfile::Generic);
        }
        assert_eq!(BankProfile::for_endpoint("https://fints.comdirect.de/fints"), BankProfile::Comdirect);
        assert_eq!(BankProfile::for_endpoint("https://fints.commerzbank.de/fints"), BankProfile::Commerzbank);
    }

}
