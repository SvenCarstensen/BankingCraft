//! Diagnostics deliberately exclude formatted log messages and panic payloads:
//! upstream errors can contain credentials, account details or bank responses.
struct MetadataLogger;
static LOGGER: MetadataLogger = MetadataLogger;
impl log::Log for MetadataLogger {
    fn enabled(&self, meta:&log::Metadata<'_>) -> bool { meta.level()<=log::Level::Warn }
    fn log(&self, record:&log::Record<'_>) {
        if self.enabled(record.metadata()) {
            // Module paths and line numbers are compile-time metadata. Do not
            // evaluate record.args(), dynamic targets, or arbitrary error text.
            eprintln!("BankingCraft {} {}:{}",record.level(),record.module_path_static().unwrap_or("dependency"),record.line().unwrap_or(0));
        }
    }
    fn flush(&self) {}
}
pub fn init() {
    let _=log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    std::panic::set_hook(Box::new(|_| {
        eprintln!("BankingCraft: interner Fehler; vor erneutem Senden den Auftragsstatus bei der Bank prüfen.");
    }));
    #[cfg(unix)] unsafe {
        // Prevent conventional core files containing the decrypted vault. This
        // does not promise exclusion from OS-managed crash collection or swap.
        let limit=libc::rlimit {rlim_cur:0,rlim_max:0};
        libc::setrlimit(libc::RLIMIT_CORE,&limit);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn diagnostics_exclude_log_and_panic_payloads() {
        let result=std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","diagnostics::tests::diagnostics_child","--ignored","--nocapture"])
            .output().unwrap();
        assert!(result.status.success());
        let output=format!("{}{}",String::from_utf8_lossy(&result.stdout),String::from_utf8_lossy(&result.stderr));
        assert!(output.contains("BankingCraft WARN"));
        assert!(!output.contains("SYNTHETIC_SECRET_"));
    }
    #[test]
    #[ignore = "subprocess helper: changes global logger and panic hook"]
    fn diagnostics_child() {
        super::init();
        log::warn!(target: "SYNTHETIC_SECRET_TARGET", "SYNTHETIC_SECRET_PIN_AND_ACCOUNT");
        let _=std::panic::catch_unwind(||panic!("SYNTHETIC_SECRET_PANIC"));
    }
}
