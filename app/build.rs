fn main() {
    println!("cargo:rerun-if-changed=src/session_macos.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new().file("src/session_macos.m").flag("-fobjc-arc").compile("bankingcraft_session");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=Foundation");
    }
}
