//! Vault-local categories and atomic updates of their transaction/rule references.
use crate::{categorize::CATEGORIES, store::{Rule, Vault}};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum CategoryOverride {
    Renamed(String),
    ReplacedBy(String),
}

pub fn is_builtin(name: &str) -> bool {
    CATEGORIES.iter().any(|c| c.name == name)
}

impl Vault {
    pub fn custom_category_names(&self) -> Vec<String> {
        // Keep custom assignments from older vaults available even without a registry.
        let mut names: Vec<_> = self.custom_categories.iter()
            .chain(self.transactions.iter().filter_map(|t| t.category.as_ref()))
            .chain(self.rules.iter().map(|r| &r.category))
            .filter(|name| !name.is_empty() && !is_builtin(name)).cloned().collect();
        names.sort();
        names.dedup();
        names
    }

    pub fn category_names(&self) -> Vec<String> {
        CATEGORIES.iter().filter(|c| !matches!(self.category_overrides.get(c.name), Some(CategoryOverride::ReplacedBy(_))))
            .map(|c| c.name.to_owned()).chain(self.custom_category_names()).collect()
    }

    pub fn category_label<'a>(&'a self, name: &'a str) -> &'a str {
        match self.category_overrides.get(name) {
            Some(CategoryOverride::Renamed(label)) => label,
            _ if is_builtin(name) => i18n::tr(name),
            _ => name,
        }
    }

    /// Redirects always point directly at an active category, including after repeated edits.
    pub fn resolved_category<'a>(&'a self, name: &'a str) -> &'a str {
        match self.category_overrides.get(name) {
            Some(CategoryOverride::ReplacedBy(replacement)) => replacement,
            _ => name,
        }
    }

    fn validate_category_name(&self, name: &str, original: Option<&str>) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 60 || name.chars().any(char::is_control) {
            return Err(i18n::tr("Bitte einen Kategorienamen mit 1 bis 60 Zeichen ohne Zeilenumbrüche eingeben.").into());
        }
        let folded = name.to_lowercase();
        let reserved = CATEGORIES.iter().filter(|c| Some(c.name) != original).any(|c| [i18n::Language::German, i18n::Language::English,
            i18n::Language::French, i18n::Language::Spanish].into_iter()
            .any(|language| i18n::text(language, c.name).to_lowercase() == folded));
        if reserved || self.category_names().iter()
            .any(|c| Some(c.as_str()) != original && self.category_label(c).to_lowercase() == folded) {
            return Err(i18n::tr("Dieser Kategoriename ist bereits vergeben.").into());
        }
        Ok(name.into())
    }

    pub fn create_category(&mut self, name: &str) -> Result<(), String> {
        let name = self.validate_category_name(name, None)?;
        self.custom_categories.push(name);
        Ok(())
    }

    fn require_category(&self, name: &str) -> Result<(), String> {
        if !self.category_names().iter().any(|c| c == name) {
            return Err(i18n::tr("Kategorie nicht gefunden.").into());
        }
        Ok(())
    }

    fn replace_category_references(&mut self, original: &str, replacement: &str) {
        for tx in &mut self.transactions {
            if tx.category.as_deref() == Some(original) { tx.category = Some(replacement.into()); }
        }
        for rule in &mut self.rules {
            if rule.category == original { rule.category = replacement.into(); }
        }
        for customization in self.category_overrides.values_mut() {
            if let CategoryOverride::ReplacedBy(target) = customization {
                if target == original { *target = replacement.into(); }
            }
        }
    }

    pub fn rename_category(&mut self, original: &str, name: &str) -> Result<String, String> {
        self.require_category(original)?;
        if name.trim() == self.category_label(original) { return Ok(original.into()); }
        let name = self.validate_category_name(name, Some(original))?;
        if is_builtin(original) {
            // Keep the built-in identity, color and automatic matching rules.
            self.category_overrides.insert(original.into(), CategoryOverride::Renamed(name));
            Ok(original.into())
        } else {
            self.custom_categories.retain(|c| c != original);
            self.custom_categories.push(name.clone());
            self.replace_category_references(original, &name);
            Ok(name)
        }
    }

    pub fn delete_category(&mut self, original: &str, replacement: &str) -> Result<(), String> {
        self.require_category(original)?;
        if original == replacement || !self.category_names().iter().any(|c| c == replacement) {
            return Err(i18n::tr("Bitte eine andere vorhandene Ersatzkategorie wählen.").into());
        }
        self.custom_categories.retain(|c| c != original);
        self.replace_category_references(original, replacement);
        if is_builtin(original) {
            self.category_overrides.insert(original.into(), CategoryOverride::ReplacedBy(replacement.into()));
        }
        Ok(())
    }

    pub fn assign_category(&mut self, index: usize, category: &str) -> Result<(), String> {
        if !self.category_names().iter().any(|c| c == category) {
            return Err(i18n::tr("Kategorie nicht gefunden.").into());
        }
        let tx = self.transactions.get_mut(index).ok_or(i18n::tr("Umsatz nicht gefunden."))?;
        tx.category = Some(category.into());
        let pattern = tx.tx.counterparty_name.to_lowercase();
        if !pattern.is_empty() {
            self.rules.retain(|r| r.pattern != pattern);
            self.rules.push(Rule { pattern, category: category.into() });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{business_tests::tx, categorize::categorize};

    #[test]
    fn names_are_validated_and_legacy_assignments_remain_manageable() {
        let mut json = serde_json::to_value(Vault::default()).unwrap();
        json.as_object_mut().unwrap().remove("custom_categories");
        json.as_object_mut().unwrap().remove("category_overrides");
        let mut v: Vault = serde_json::from_value(json).unwrap();
        v.merge_transactions(1, vec![tx(-100, "2026-11-01")]);
        v.transactions[0].category = Some("Altbestand".into());
        v.rules.push(Rule { pattern: "anderer empfänger".into(), category: "Nur Regel".into() });
        v.create_category("  Urlaub  ").unwrap();
        assert_eq!(v.custom_category_names(), ["Altbestand", "Nur Regel", "Urlaub"]);
        let before = serde_json::to_value(&v).unwrap();
        for name in ["", "   ", "UrLaUb", "Altbestand", "Wohnen", "housing", "Logement", "Vivienda", "a\nb", "a\0b", &"x".repeat(61)] {
            assert!(v.create_category(name).is_err(), "accepted {name:?}");
            assert_eq!(serde_json::to_value(&v).unwrap(), before);
        }
        v.rename_category("Altbestand", "Archiv").unwrap();
        v.delete_category("Nur Regel", "Archiv").unwrap();
        assert_eq!(v.transactions[0].category.as_deref(), Some("Archiv"));
        assert_eq!(v.rules[0].category, "Archiv");
        assert!(!v.category_names().contains(&"Altbestand".to_owned()));
        v.rename_category("Wohnen", "Zuhause").unwrap();
        assert_eq!(v.category_label("Wohnen"), "Zuhause");
        v.delete_category("Wohnen", "Sonstiges").unwrap();
        assert!(!v.category_names().contains(&"Wohnen".to_owned()));
    }

    #[test]
    fn rename_and_delete_move_manual_and_learned_assignments_without_changing_bank_data() {
        let mut v = Vault::default();
        v.create_category("Urlaub").unwrap();
        v.create_category("Sparen").unwrap();
        v.merge_transactions(1, vec![tx(-100, "2026-11-01"), tx(-200, "2026-11-02")]);
        v.assign_category(0, "Urlaub").unwrap();
        assert_eq!(categorize(&v.transactions[1], &v), "Urlaub");
        let bank_data: Vec<_> = v.transactions.iter().map(|t| serde_json::to_value(&t.tx).unwrap()).collect();
        v.rename_category("Urlaub", "Reisen").unwrap();
        for t in &v.transactions { assert_eq!(categorize(t, &v), "Reisen"); }
        assert!(crate::export::csv_content(&v, &[0, 1]).contains("Reisen"));
        for replacement in ["Reisen", "Unbekannt"] {
            let before = serde_json::to_value(&v).unwrap();
            assert!(v.delete_category("Reisen", replacement).is_err());
            assert_eq!(serde_json::to_value(&v).unwrap(), before);
        }
        v.delete_category("Reisen", "Sparen").unwrap();
        for t in &v.transactions { assert_eq!(categorize(t, &v), "Sparen"); }
        v.delete_category("Sparen", "Sonstiges").unwrap();
        for t in &v.transactions { assert_eq!(categorize(t, &v), "Sonstiges"); }
        assert!(v.custom_category_names().is_empty());
        assert_eq!(bank_data, v.transactions.iter().map(|t| serde_json::to_value(&t.tx).unwrap()).collect::<Vec<_>>());
    }
    #[test]
    fn builtin_names_keep_identity_color_and_automatic_classification_across_languages() {
        let mut v = Vault::default();
        let mut t = crate::store::StoredTx { account_id: 1, tx: tx(-100, "2026-11-01"), category: None };
        t.tx.counterparty_name = "REWE".into();
        assert_eq!(v.rename_category("Lebensmittel", "Einkäufe").unwrap(), "Lebensmittel");
        for language in [i18n::Language::German, i18n::Language::English, i18n::Language::French, i18n::Language::Spanish] {
            i18n::with_language(language, || {
                assert_eq!(categorize(&t, &v), "Lebensmittel");
                assert_eq!(v.category_label(&categorize(&t, &v)), "Einkäufe");
            });
        }
        assert_eq!(v.category_names().iter().filter(|c| *c == "Lebensmittel").count(), 1);
        assert!(v.create_category("einkäufe").is_err());
        assert!(v.rename_category("Wohnen", "Einkäufe").is_err());
        // Users can restore the original name as well as save an unchanged translation.
        v.rename_category("Lebensmittel", "Lebensmittel").unwrap();
        assert_eq!(v.category_label("Lebensmittel"), "Lebensmittel");
        i18n::with_language(i18n::Language::English, || {
            let mut fresh = Vault::default();
            fresh.rename_category("Wohnen", "Housing").unwrap();
            assert!(fresh.category_overrides.is_empty());
        });
    }

    #[test]
    fn deleting_builtins_redirects_fallbacks_and_survives_repeated_edits_and_reload() {
        let mut v = Vault::default();
        v.create_category("Alltag").unwrap();
        v.merge_transactions(1, vec![tx(-100, "2026-11-01"), tx(200, "2026-11-02")]);
        v.transactions[0].tx.counterparty_name = "REWE".into();
        v.assign_category(1, "Gehalt").unwrap();
        for builtin in CATEGORIES {
            v.delete_category(builtin.name, "Alltag").unwrap();
        }
        assert_eq!(v.category_names(), ["Alltag"]);
        for t in &v.transactions { assert_eq!(categorize(t, &v), "Alltag"); }
        let mut unknown = crate::store::StoredTx { account_id: 1, tx: tx(-100, "2026-11-03"), category: None };
        unknown.tx.counterparty_name.clear();
        assert_eq!(categorize(&unknown, &v), "Alltag");
        unknown.tx.amount = rust_decimal::Decimal::ONE;
        assert_eq!(categorize(&unknown, &v), "Alltag");
        v.rename_category("Alltag", "Haushalt").unwrap();
        v.create_category("Ausgaben").unwrap();
        v.delete_category("Haushalt", "Ausgaben").unwrap();
        let restored: Vault = serde_json::from_slice(&serde_json::to_vec(&v).unwrap()).unwrap();
        assert_eq!(restored.category_names(), ["Ausgaben"]);
        for t in &restored.transactions { assert_eq!(categorize(t, &restored), "Ausgaben"); }
        assert_eq!(categorize(&unknown, &restored), "Ausgaben");
        assert!(v.assign_category(0, "Lebensmittel").is_err());
        let before = serde_json::to_value(&v).unwrap();
        assert!(v.delete_category("Ausgaben", "Lebensmittel").is_err());
        assert!(v.delete_category("Ausgaben", "Ausgaben").is_err());
        assert_eq!(before, serde_json::to_value(&v).unwrap());
    }

}
