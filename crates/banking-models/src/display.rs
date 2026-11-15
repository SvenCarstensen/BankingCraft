use crate::Account;

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
