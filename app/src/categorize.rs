use crate::store::{Rule, StoredTx, Vault};

pub struct Category {
    pub name: &'static str,
    pub color: (u8, u8, u8),
}

pub const CATEGORIES: &[Category] = &[
    Category { name: "Lebensmittel", color: (0x2f, 0x9e, 0x44) },
    Category { name: "Wohnen", color: (0x19, 0x71, 0xc2) },
    Category { name: "Energie", color: (0xf0, 0x8c, 0x00) },
    Category { name: "Mobilität", color: (0x0c, 0x85, 0x99) },
    Category { name: "Versicherungen", color: (0x5f, 0x3d, 0xc4) },
    Category { name: "Gesundheit", color: (0xe0, 0x31, 0x31) },
    Category { name: "Freizeit", color: (0xc2, 0x25, 0x5c) },
    Category { name: "Shopping", color: (0xe8, 0x59, 0x0c) },
    Category { name: "Kommunikation", color: (0x34, 0x8a, 0xc7) },
    Category { name: "Bargeld", color: (0x86, 0x8e, 0x96) },
    Category { name: "Steuern & Abgaben", color: (0x49, 0x50, 0x57) },
    Category { name: "Gehalt", color: (0x09, 0x92, 0x68) },
    Category { name: "Einnahmen", color: (0x37, 0xb2, 0x4d) },
    Category { name: "Umbuchung", color: (0x4d, 0xab, 0xf7) },
    Category { name: "Sonstiges", color: (0x9a, 0xa5, 0xb1) },
];

const BUILTIN: &[(&str, &str)] = &[
    ("rewe", "Lebensmittel"),
    ("edeka", "Lebensmittel"),
    ("aldi", "Lebensmittel"),
    ("lidl", "Lebensmittel"),
    ("netto", "Lebensmittel"),
    ("penny", "Lebensmittel"),
    ("kaufland", "Lebensmittel"),
    ("famila", "Lebensmittel"),
    ("bäckerei", "Lebensmittel"),
    ("supermarkt", "Lebensmittel"),
    ("miete", "Wohnen"),
    ("wohnungsbau", "Wohnen"),
    ("hausverwaltung", "Wohnen"),
    ("nebenkosten", "Wohnen"),
    ("stadtwerke", "Energie"),
    ("strom", "Energie"),
    ("energie", "Energie"),
    ("gas ", "Energie"),
    ("tankstelle", "Mobilität"),
    ("aral", "Mobilität"),
    ("shell", "Mobilität"),
    ("esso", "Mobilität"),
    ("db vertrieb", "Mobilität"),
    ("deutsche bahn", "Mobilität"),
    ("deutschlandticket", "Mobilität"),
    ("nah.sh", "Mobilität"),
    ("kfz", "Mobilität"),
    ("versicherung", "Versicherungen"),
    ("huk", "Versicherungen"),
    ("allianz", "Versicherungen"),
    ("provinzial", "Versicherungen"),
    ("apotheke", "Gesundheit"),
    ("arzt", "Gesundheit"),
    ("krankenkasse", "Gesundheit"),
    ("netflix", "Freizeit"),
    ("spotify", "Freizeit"),
    ("kino", "Freizeit"),
    ("restaurant", "Freizeit"),
    ("amazon", "Shopping"),
    ("otto", "Shopping"),
    ("zalando", "Shopping"),
    ("ikea", "Shopping"),
    ("paypal", "Shopping"),
    ("telekom", "Kommunikation"),
    ("vodafone", "Kommunikation"),
    ("o2 ", "Kommunikation"),
    ("1&1", "Kommunikation"),
    ("mobilfunk", "Kommunikation"),
    ("bargeld", "Bargeld"),
    ("geldautomat", "Bargeld"),
    ("auszahlung", "Bargeld"),
    ("finanzamt", "Steuern & Abgaben"),
    ("rundfunk", "Steuern & Abgaben"),
    ("bundeskasse", "Steuern & Abgaben"),
    ("gemeinde", "Steuern & Abgaben"),
    ("lohn", "Gehalt"),
    ("gehalt", "Gehalt"),
    ("bezüge", "Gehalt"),
    ("umbuchung", "Umbuchung"),
    ("übertrag", "Umbuchung"),
];

pub fn color_of(name: &str) -> crate::ui::Color {
    let c = CATEGORIES.iter().find(|c| c.name == name).map(|c| c.color).unwrap_or((0x9a, 0xa5, 0xb1));
    crate::ui::Color::from_rgb(c.0, c.1, c.2)
}

pub fn categorize(t: &StoredTx, vault: &Vault) -> String {
    vault.resolved_category(&base_category(t, &vault.rules)).to_owned()
}

fn base_category(t: &StoredTx, rules: &[Rule]) -> String {
    if let Some(c) = &t.category {
        return c.clone();
    }
    let hay = format!(
        "{} {} {}",
        t.tx.counterparty_name.to_lowercase(),
        t.tx.purpose.to_lowercase(),
        t.tx.posting_text.to_lowercase()
    );
    let name = t.tx.counterparty_name.to_lowercase();
    for r in rules {
        if !r.pattern.is_empty() && name == r.pattern {
            return r.category.clone();
        }
    }
    for (pat, cat) in BUILTIN {
        if hay.contains(pat) {
            // salary keywords only for credits
            if *cat == "Gehalt" && t.tx.amount.is_sign_negative() {
                continue;
            }
            return cat.to_string();
        }
    }
    if t.tx.amount.is_sign_negative() { "Sonstiges".into() } else { "Einnahmen".into() }
}
