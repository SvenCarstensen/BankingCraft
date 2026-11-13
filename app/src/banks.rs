//! Embedded directory of German banks with their FinTS endpoints, searchable by
//! name, city, BLZ or BIC.

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
                let f: Vec<&'static str> = l.split('\t').collect();
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
    if b.bic.starts_with("PBNK") {
        Some(i18n::tr("Anmeldename: Ihre Postbank ID, ggf. mit Profil (PostbankID#Profilname); TAN per BestSign oder photoTAN"))
    } else if b.bic.starts_with("DEUT") {
        Some(i18n::tr("Anmeldename: Filiale + Konto + Unterkonto (10-stellig), TAN per BestSign oder photoTAN"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
