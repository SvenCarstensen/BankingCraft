use std::{collections::BTreeMap, env, fs, path::PathBuf};
fn main() {
    println!("cargo:rerun-if-changed=catalogue.json");
    let entries: BTreeMap<String, [String; 3]> = serde_json::from_str(&fs::read_to_string("catalogue.json").unwrap()).unwrap();
    let mut code = String::from("pub fn text(lang: Language, key: &str) -> &str { match key {\n");
    for (key, [en, fr, es]) in entries {
        assert!(!en.is_empty() && !fr.is_empty() && !es.is_empty(), "empty translation: {key}");
        code.push_str(&format!("{key:?} => match lang {{ Language::German => {key:?}, Language::English => {en:?}, Language::French => {fr:?}, Language::Spanish => {es:?} }},\n"));
    }
    code.push_str("_ => key } }\n");
    fs::write(PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("catalogue.rs"), code).unwrap();
}
