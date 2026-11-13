//! Optional machine-local test configuration. No product ID is compiled in or saved to a vault.

pub(crate) fn resolve(configured: &str) -> String {
    resolve_with(configured, || {
        #[cfg(feature = "local-fints-test")]
        {
            let base = directories::BaseDirs::new()?;
            read_local(&base.config_dir().join("BankingCraft/local-tests/fints-product-id"))
        }
        #[cfg(not(feature = "local-fints-test"))]
        { None }
    })
}

fn resolve_with(configured: &str, local: impl FnOnce() -> Option<String>) -> String {
    if !configured.is_empty() { return configured.to_string(); }
    if cfg!(feature = "local-fints-test") { local().unwrap_or_default() } else { String::new() }
}

#[cfg(any(feature = "local-fints-test", test))]
fn read_local(path: &std::path::Path) -> Option<String> {
    use std::io::Read;
    let mut value = String::new();
    std::fs::File::open(path).ok()?.take(129).read_to_string(&mut value).ok()?;
    if value.len() > 128 { return None; }
    let value = value.trim();
    if value.is_empty() || value.len() > 25 || !value.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_id_takes_precedence_without_reading_local_configuration() {
        assert_eq!(resolve_with("OWNREGISTEREDID", || panic!("must not read local file")), "OWNREGISTEREDID");
    }

    #[test]
    fn only_explicit_test_builds_use_a_local_id_and_do_not_mutate_the_vault_value() {
        let vault_id = String::new();
        let local_read = std::cell::Cell::new(false);
        let result = resolve_with(&vault_id, || {
            local_read.set(true);
            Some("SYNTHETICTESTID".into())
        });
        assert_eq!(local_read.get(), cfg!(feature = "local-fints-test"), "normal builds must not read local configuration");
        assert_eq!(result, if cfg!(feature = "local-fints-test") { "SYNTHETICTESTID" } else { "" });
        assert!(vault_id.is_empty());
        assert_eq!(resolve_with("", || None), "");
    }

    #[test]
    fn local_file_is_required_and_rejects_invalid_or_oversized_values() {
        let dir = std::env::temp_dir().join(format!("bankingcraft-product-id-{:016x}", rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("fints-product-id");
        assert!(read_local(&path).is_none());
        for value in ["", "\n", "TEST+SEGMENT", "TEST ID", "ÜMLAUT", &"A".repeat(129)] {
            std::fs::write(&path, value).unwrap();
            assert!(read_local(&path).is_none());
        }
        std::fs::write(&path, "SYNTHETICTESTID\n").unwrap();
        assert_eq!(read_local(&path).as_deref(), Some("SYNTHETICTESTID"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
