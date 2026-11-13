//! Explicit export of fictional demo data and English screenshots for the README.
//! Run only with: cargo test -p bankingcraft export_english_showcase -- --ignored
use crate::{AppState, AppWindow, store::{Vault, VaultFile}, view};

use std::path::PathBuf;

fn english(text: &str) -> String {
    let translations = [
        ("Wohnbau Ostsee eG", "Harbor Housing"), ("Miete Wohnung 3 OG links", "Monthly apartment rent"),
        ("Max Mustermann", "Alex Morgan"), ("Sparrate Tagesgeld", "Monthly savings transfer"),
        ("Muster Werkstätten GmbH", "Example Studio Ltd"), ("Lohn/Gehalt Abrechnung", "Monthly salary"),
        ("Stadtwerke Förde", "Harbor Energy"), ("Abschlag Strom und Gas", "Electricity and heating"),
        ("Mobilfunk Nord", "North Mobile"), ("Rechnung Mobilfunkvertrag", "Mobile phone plan"),
        ("Streamingdienst Nordwind", "Northwind Streaming"), ("Monatsabo Premium", "Premium subscription"),
        ("Kfz-Versicherung Küste", "Coast Insurance"), ("Beitrag Kfz-Haftpflicht", "Car insurance"),
        ("Supermarkt Nordlicht", "Harbor Market"), ("Kartenzahlung girocard", "Card payment"),
        ("Tankstelle am Ring", "City Fuel"), ("Versandhaus Möwe", "Seabird Store"),
        ("Bestellung 2026-", "Online order"), ("Geldautomat Hafen", "Harbor ATM"),
        ("Bargeldauszahlung", "Cash withdrawal"), ("Apotheke am Markt", "Market Pharmacy"),
        ("Café Hafenblick", "Harbor Cafe"), ("DAUERAUFTRAG", "STANDING ORDER"),
        ("UMBUCHUNG", "INTERNAL TRANSFER"), ("LOHN GEHALT", "SALARY"),
        ("LASTSCHRIFT", "DIRECT DEBIT"), ("KARTENZAHLUNG", "CARD PAYMENT"), ("AUSZAHLUNG", "CASH WITHDRAWAL"),
    ];
    translations.iter().find(|(source, _)| *source == text).map(|(_, target)| *target).unwrap_or(text).to_string()
}

fn demo_vault() -> Vault {
    let mut vault = Vault::default();
    crate::demo::add_demo(&mut vault);
    vault.accesses[0].name = "Demo Bank (offline)".into();
    for (index, account) in vault.accounts.iter_mut().enumerate() {
        account.account.owner = "Alex Morgan".into();
        account.account.product = if index == 0 { "Everyday account" } else { "Savings account" }.into();
    }
    for stored in &mut vault.transactions {
        stored.category = Some(crate::categorize::categorize(stored, &vault.rules));
        stored.tx.counterparty_name = english(&stored.tx.counterparty_name);
        stored.tx.purpose = english(&stored.tx.purpose);
        stored.tx.posting_text = english(&stored.tx.posting_text);
        stored.tx.compute_fingerprint();
    }
    for standing in &mut vault.standing {
        let transfer = &mut standing.order.transfer;
        transfer.debtor_name = english(&transfer.debtor_name);
        transfer.creditor_name = english(&transfer.creditor_name);
        transfer.purpose = english(&transfer.purpose);
    }
    let group = vault.create_group("Personal finances").unwrap();
    for account_id in vault.accounts.iter().map(|a| a.id).collect::<Vec<_>>() {
        vault.assign_group(account_id, Some(group)).unwrap();
    }
    let account_id = vault.accounts[0].id;
    let mut transfer = vault.standing[0].order.transfer.clone();
    transfer.amount = "125.00".parse().unwrap();
    transfer.purpose = "Apartment maintenance".into();
    vault.queue_transfer(account_id, transfer, false, true).unwrap();
    vault.save_recipient(account_id, "Alex Morgan", &crate::demo::iban(crate::demo::DEMO_BLZ, "4711099"), "DEMODEXXXXX").unwrap();
    assert!(vault.accesses.iter().all(|a| a.kind == crate::store::AccessKind::Demo && a.config.url.is_empty() && a.saved_pin.is_none()));
    assert!(vault.product_id.is_empty());
    vault
}

#[test]
#[ignore = "exports a separate fictional demo vault and five English README screenshots"]
fn export_english_showcase() {
    let vault = demo_vault();
    if let Some(path) = std::env::var_os("BANKINGCRAFT_DEMO_VAULT") {
        let path = PathBuf::from(path);
        assert!(!path.exists(), "never overwrite an existing vault");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let password = std::env::var("BANKINGCRAFT_DEMO_PASSWORD").expect("explicit demo password required");
        let file = VaultFile::create(path.clone(), &password).unwrap();
        file.save(&vault).unwrap();
        let (_, restored) = VaultFile::open(path, &password).unwrap();
        assert_eq!(restored.transactions.len(), vault.transactions.len());
    }
    let app = AppWindow::new().unwrap();
    i18n::set_language(i18n::Language::English);
    let st = app.global::<AppState>();
    st.set_theme_mode(2);
    st.set_unlocked(true);
    st.set_active_vault_name("Demo".into());
    st.set_language_mode(2);
    st.set_tx_period(1);
    let mut view_state = view::ViewState::default();
    view::refresh_all(&app, &vault, &mut view_state);
    st.set_tx_account(1);
    view::refresh_transactions(&app, &vault, &view_state);
    st.set_analysis_offset(-1); // a complete month for meaningful reports
    view::refresh_analysis(&app, &vault);
    st.set_tf_name("Harbor Housing".into());
    st.set_tf_iban(crate::demo::iban("87654321", "100200"));
    st.set_tf_bic("DEMODEXXXXX".into());
    st.set_tf_amount("125,00".into());
    st.set_tf_purpose("Apartment maintenance".into());
    st.set_so_first(chrono::Local::now().format("%d.%m.%Y").to_string());
    let selected = vault.transactions.iter().position(|s| s.tx.pending).unwrap();
    view::show_detail(&app, &vault, selected);
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/screenshots");
    std::fs::create_dir_all(&output).unwrap();

    for (page, name) in [(0, "overview"), (1, "accounts"), (2, "transactions"), (3, "transfer"), (5, "reports")] {
        let (width, height) = (1440, if page == 0 { 1360 } else { 1000 });
        st.set_page(page);
        crate::ui::testing::snapshot(&app,width,height,output.join(format!("{name}.png")));
    }

    st.set_page(4);
    crate::ui::testing::snapshot(&app, 1440, 1000, std::env::temp_dir().join("bankingcraft-egui-standing-orders-en.png"));

    let light = std::env::temp_dir().join("bankingcraft-egui-light-review");
    std::fs::create_dir_all(&light).unwrap();
    st.set_theme_mode(1);
    for (page, name) in [(0, "overview"), (2, "transactions"), (3, "transfer"), (4, "standing"), (5, "reports"), (6, "accesses"), (7, "reader"), (8, "settings"), (9, "orders")] {
        st.set_page(page);
        crate::ui::testing::snapshot(&app, 1360, if page == 0 { 1360 } else { 860 }, light.join(format!("{name}.png")));
    }

}
