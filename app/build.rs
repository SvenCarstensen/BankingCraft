use std::{collections::BTreeMap, path::PathBuf};

fn main() {
    let catalogue = "../crates/i18n/catalogue.json";
    println!("cargo:rerun-if-changed={catalogue}");
    let entries: BTreeMap<String, [String; 3]> = serde_json::from_slice(&std::fs::read(catalogue).unwrap()).unwrap();
    let translations = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("translations");
    for (index, language) in ["de", "en", "fr", "es"].iter().enumerate() {
        let folder = translations.join(language).join("LC_MESSAGES");
        std::fs::create_dir_all(&folder).unwrap();
        let mut po = String::from("msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\"Plural-Forms: nplurals=2; plural=(n != 1);\\n\"\n\n");
        for (key, values) in &entries {
            let translated = if index == 0 { key } else { &values[index - 1] };
            po.push_str(&format!("msgid {}\nmsgstr {}\n\n", serde_json::to_string(key).unwrap(), serde_json::to_string(translated).unwrap()));
        }
        std::fs::write(folder.join("bankingcraft.po"), po).unwrap();
    }
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .with_bundled_translations(translations)
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint build failed");
}
