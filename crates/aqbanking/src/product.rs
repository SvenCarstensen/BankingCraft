//! HKVVB identity with an explicit machine-local test override.
use crate::wire::Frame;
use banking::Result;
#[cfg(any(feature = "local-fints-test", test))]
use banking::Error;

// Only forwarded to AqBanking's fintsRegistrationKey for HKVVB/Produktbezeichnung.
pub(super) const REGISTRATION: &str = "FC3BB6C907959400A3931FC75";
const _: () = assert!(REGISTRATION.len() == 25);

pub(crate) fn registration() -> Result<String> {
    #[cfg(feature = "local-fints-test")]
    {
        let base = directories::BaseDirs::new().ok_or_else(invalid_local)?;
        read_local(&base.config_dir().join("BankingCraft/local-tests/fints-product-id"))
    }
    #[cfg(not(feature = "local-fints-test"))]
    { Ok(REGISTRATION.into()) }
}

#[cfg(any(feature = "local-fints-test", test))]
fn invalid_local() -> Error {
    Error::Security("Temporäre FinTS-Produktkennung fehlt oder ist ungültig.".into())
}

#[cfg(any(feature = "local-fints-test", test))]
fn read_local(path: &std::path::Path) -> Result<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path).map_err(|_| invalid_local())?
        .take(28).read_to_string(&mut text).map_err(|_| invalid_local())?;
    let value = text.trim_end_matches(['\r', '\n']);
    if text.len() > 27 || value.len() != 25 || !value.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(invalid_local());
    }
    Ok(value.into())
}

pub(crate) fn configure(request: &mut Frame, registration: &str) {
    // FinTS allows five characters for the separate application version.
    // Fixed-width major/minor/patch encoding: 0.0.10 -> 00010.
    let major: u32 = env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap();
    let minor: u32 = env!("CARGO_PKG_VERSION_MINOR").parse().unwrap();
    let patch: u32 = env!("CARGO_PKG_VERSION_PATCH").parse().unwrap();
    assert!(major < 10 && minor < 100 && patch < 100, "FinTS version exceeds five characters");
    request.put("product", registration);
    request.put("product_version", format!("{major}{minor:02}{patch:02}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_registration_is_exact_and_invalid_files_fail_before_banking() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fints-product-id");
        assert!(read_local(&path).is_err());
        for value in ["", "X", &"X".repeat(24), &"X".repeat(26),
            "SYNTHETICREGISTRATION0001+", " SYNTHETICREGISTRATION0001", "SYNTHETICREGISTRATION0001 ",
            "SYNTHETICREGISTRATION0001\0", &"X".repeat(100)] {
            std::fs::write(&path, value).unwrap();
            assert!(read_local(&path).is_err());
        }
        let synthetic = "SYNTHETICREGISTRATION0001";
        for suffix in ["", "\n", "\r\n"] {
            std::fs::write(&path, format!("{synthetic}{suffix}")).unwrap();
            let selected = read_local(&path).unwrap();
            let mut request = Frame::new("open");
            configure(&mut request, &selected);
            assert_eq!(request.bytes("product"), synthetic.as_bytes());
            assert_eq!(request.entries().filter(|(_, v)| v.windows(25).any(|w| w == synthetic.as_bytes()))
                .map(|(key, _)| key).collect::<Vec<_>>(), ["product"]);
        }
    }

    #[cfg(not(feature = "local-fints-test"))]
    #[test]
    fn normal_build_uses_the_publishers_registration() {
        assert_eq!(registration().unwrap(), REGISTRATION);
    }
}
