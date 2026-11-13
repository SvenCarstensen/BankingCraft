# Fachliche Tests

Ausführen im Projektverzeichnis:

```sh
./scripts/test-business.sh
```

Das Skript führt die Tests aller Rust-Pakete und anschließend Clippy mit Warnungen als Fehler aus. Es verändert keinen produktiven Tresor, stellt keine Bankverbindung her und benötigt keinen angeschlossenen Kartenleser. Die Tests verwenden erfundene Konten, eigene temporäre Tresore und einen simulierten FinTS-Transport. Ein unerwarteter weiterer Bankaufruf lässt die Ablaufprüfung fehlschlagen.

## Abgedeckte Teilprozesse

| Teilprozess | Geprüfte Fälle | Tests |
|---|---|---|
| Bankzugang und Dialog | Synchronisation, System-ID, Dialogeröffnung und -ende, wiederholtes Öffnen, fehlende Zugänge, Demo ohne Bankaufruf | `client::business_flow_tests`, `jobs::business_tests` |
| Konten entdecken und aktualisieren | IBAN ergänzen, Unterkonten, getrennte Bankzugänge, stabile IDs, Saldo, Gruppenzuordnung und Sortierung erhalten | `business_tests`, `client::business_flow_tests`, BPD-Tests |
| Salden | HKSAL-Versionen 5/6/7, Soll/Haben, verfügbarer Betrag, Kontoberechtigungen, Standard- und proprietäre Kreditkartensalden | `client::business_flow_tests` |
| Giro-Umsätze | MT940/MT942, CAMT, mehrere Seiten, vorgemerkte Daten, leeres Ergebnis gegenüber fehlenden Daten, fehlender/wiederholter Aufsetzpunkt | `client::business_flow_tests`, `business_data_tests` |
| Kreditkarten-Umsätze | HKKKU/DKKKU, Feldpositionen, Buchungs-/Valutadatum, Vorzeichen, Referenzen, vorgemerkt, Saldo im Bericht, mehrere Seiten, beschädigte/unvollständige Antworten | `credit_card::tests`, `client::credit_card_regressions` |
| Lokales Zusammenführen | Wiederholte und überlappende Abrufe, legitime gleiche Buchungen, vorgemerkt zu gebucht, kontenbezogenes Ersetzen, manuelle Kategorien erhalten | `business_tests` |
| Einzelkonto aktualisieren | Saldo-/Umsatzanfragen nur für das ausgewählte Konto, keine Kontoermittlung oder Zahlungsübermittlung, andere Konten/Zugänge/Aufträge unverändert, Demo und ungültige Konto-ID | `jobs::account_refresh_tests` |
| Gruppenabruf | Nur Konten der gewählten Gruppe, ein Dialog pro Bankzugang, getrennte Banken, „Ohne Gruppe“, leere/ungültige Gruppen, unveränderte Zahlungsaufträge | `jobs::account_refresh_tests` |
| Umsatznavigation | Aktualisieren verwendet die aktuelle Auswahl und stabile Konto-ID; ungültige Auswahl löst keinen Rundruf aus; Zurück führt zur Kontenliste | `transactions_ui::tests` |
| Überweisung | Betrags-, IBAN- und Textvalidierung, normale und Echtzeitüberweisung, kein Rückfall auf normale Überweisung, keine automatische Wiederholung bei Bankfehler/Antwortverlust | `client::business_flow_tests`, `business_data_tests`, `regression_tests` |
| Auftragsliste | Verschlüsselte Persistenz und Neustart, Versand nur für den synchronisierten Zugang, einzelner Sofortversand, persistente Markierung vor Versand, keine Doppelüberweisung bei Antwortverlust/Absturz/Schreibfehler, Abbruch und abgelaufene Termine | `jobs::outgoing_order_tests`, `payments::tests` |
| Empfängeradressbuch | Ausschließlich per Checkbox speichern/aktualisieren, gleiche IBAN je Konto getrennt, Namens-/IBAN-Prüfung, unabhängige Auftragssnapshots, Auswahl und Kontowechsel, Rücksetzen der Checkbox, alte Tresore und Entfernen eines Zugangs | `payments::tests`, `payments_ui::tests` |
| Standardaktion | Eingabetaste im Überweisungsformular speichert ohne Bankaufruf, Empfängerauswahl füllt Name/IBAN/BIC, verschlüsselter Rundlauf von Aufträgen und Adressbuch | `payments_ui::tests` |
| Terminüberweisung | Fehlendes/heutiges Datum abweisen, zukünftiges Datum im Zahlungsdokument, Auftrags-ID auswerten, nicht unterstützte Bankfunktion | `client::business_flow_tests` |
| Dauerauftrag anlegen | Monatlich/wöchentlich, Rhythmus, Enddatum, Ultimo, Grenzen, Platzhalterdatum im Zahlungsdokument, TAN | `client::business_flow_tests`, `business_data_tests` |
| Dauerauftrag abrufen | HKCDB 1/2, Formataushandlung, mehrere Seiten, Ausführungszeit und letztes Datum, ungültige Versionen/Formate, unvollständige Seiten | `client::standing_regressions`, `client::business_flow_tests` |
| Dauerauftrag löschen | Originaldaten der Bank und Auftrags-ID erhalten, TAN, fehlende Bankunterstützung, fehlender lokaler Auftrag | `client::business_flow_tests`, `jobs::business_tests` |
| Nächste Ausführung | Schaltjahr, kurze Monate, Quartal, Zweiwochenrhythmus, inklusives Enddatum, Ultimo minus 1/2 Tage | `business_tests` |
| TAN und Freigabe | Zulässige Verfahrensauswahl, TAN-Folgeauftrag ohne Zahlungswiederholung, Abbruch, App-Freigabe mit Statusabfrage, fehlerhafte Challenge, SCA bei lesenden Aufträgen, PIN-Verwerfung | `client::business_flow_tests` |
| TAN-Darstellung und Kartenprotokoll | Matrix-Nutzdaten und Längen, fehlerhafte Flickercodes, USB-BCD-TAN, Kartenstatus, APDU/TLV, DDV-Verschlüsselung und Signaturzähler | `business_data_tests`, `regression_tests`, `cardreader` |
| RDH | Simulierter kompletter RDH-7-Dialog, Bankschlüssel, Hashvergleich, Signatur/Ver- und Entschlüsselung, manipulierte/unsignierte Antworten | `rdh_flow_test`, `rdh::tests`, `regression_tests` |
| Empfängerprüfung (VoP) | Match/Close-Match/No-Match/nicht möglich, vollständige Bankhinweise, XML-Bericht und native Antwort, Bindung an Zahlung/IBAN, Abbruch vor TAN, identische zweite Einreichung, Polling ohne Zahlungswiederholung, Chipkartenablauf, fehlende Bankunterstützung und ungültige Antworten | `vop::tests`, `client::business_flow_tests`, `payee_ui::tests` |
| Mehrere Tresore | Anlegen ohne Überschreiben (auch bei Symlinks), getrennte Passwörter und Konten, bestehender Standardtresor, letzte Auswahl, fehlerhafte Auswahldatei, Abbrechen und falsches Passwort, Sperre bei laufendem Vorgang, Formular-/PIN-Cache zurücksetzen, Speicherfehler beim Wechsel | `vaults::tests` |
| Tresor | Verschlüsseltes Speichern/Öffnen, frische Nonces, Dateirechte, falsches Passwort, Manipulation/Abschneiden, fehlgeschlagener Dateitausch | `business_tests` |
| Passwort und gespeicherte PIN | Passwortwechsel mit PIN-Neuverschlüsselung, Rollback bei Fehlern, beschädigte PIN, Tresorbindung, Speicherung nur nach Auswahl, Entfernen nach Bankablehnung | `store::tests`, `business_tests`, `jobs::business_tests` |
| Gruppen und Kontensortierung | Erstellen/Umbenennen/Löschen/Zuweisen, Dubletten und ungültige Namen, wiederholte Zuweisung, Vor-/Zurückverschieben, ungültige Ziele, Persistenz | `groups::tests`, `business_tests` |
| Gruppensalden | Gebuchte Salden, getrennte Währungen, Teilbestände, fehlender Saldo gegenüber Nullsaldo | `view::amount_tests` |
| Umsatzansicht | Konto/Datum/Kategorie/Mehrwortsuche kombinieren, Groß-/Kleinschreibung, unbekannte Datumswerte, vorgemerkte zuerst, Betrags-/Datumsdarstellung | `view::amount_tests`, `business_tests` |
| Kategorien und Auswertung | Manuell vor gelernt vor Standardregel, Gehalt nur bei Gutschrift, Monatsgrenzen, vorgemerkt/unbekannt/Umbuchung ausschließen, Monatsreihe | `business_tests`, `view::amount_tests` |
| Bank-X-Import | Wiederholbarkeit, Normalisierung, bestehende Konfiguration/Gruppen erhalten, Format-/Feld-/Kontoprüfung, Größenlimit, verschlüsselte Sicherung, keine Teilübernahme | `bankx_import::tests` |
| CSV-/CAMT-Export | Auswahl/Reihenfolge, ungültige Indizes, Sonderzeichen einschließlich CR, fehlende Datumswerte, vorgemerkt, Debit/Credit, Referenzen, CAMT-Rückimport | `business_tests` |
| Bankzugang entfernen | Konten, Umsätze, Daueraufträge und Gruppenzuordnung entfernen; anderen Zugang erhalten | `business_tests` |
| Bankensuche | Bekannte Banken und Suchbegriffe | `banks::tests` |
| Sicherheit | Offene TAN-Freigabe, Auftraggeberkonto, HTTPS-Ziele, Parser-/XML-/Bildlimits, Bankschlüssel, DDV-Umschlag/Padding, RDH-10-Vektor, CSV-Formeln, private Dateien und Symlinks | `security_tests`, `client::business_flow_tests`, `rdh::tests`, `bridge::image_security_tests`, `private_file::tests`, `business_tests` |
| Fenster wiederherstellen | Position und logische Größe, erneutes Speichern, ungültige/zu große Einstellungsdatei, negative Bildschirmkoordinaten, Retina-Skalierung, abgezogener Monitor | `window_state::tests` |
| Erscheinungsbild | Systemstandard beim ersten Start, fehlende/ungültige/zu große Einstellungsdatei, wiederholter Wechsel und Persistenz aller drei Modi, Schreibfehler, Aufheben der Widget- und Fensterdesign-Vorgaben | `appearance::tests` |
| RDH-Karten-PIN | ASCII-PC/SC-Referenzvektor ohne Füllzeichen, exakte Software-PIN-Länge, FPIN2-Längennibble/Füllung, Auswahl nur der Zugriffs-PIN, unbekannte/fehlende Metadaten, kein zweiter Versuch bei Fehler/Abbruch/Transportverlust, Erfolgs-Cache, Fehlversuchszähler und Leserantworten | `cardreader::rdh::tests` |

## Lokalisierung

Die Sprachtests prüfen Deutsch, Englisch, Französisch und Spanisch, regionale Systemsprachen,
Englisch als Rückfall bei unbekannter Sprache, fehlende/fehlerhafte Einstellungsdateien,
Speicherung und Wiederherstellung sowie Schreibfehler. Alle Katalogeinträge müssen die
Formatplatzhalter unverändert erhalten. Formatierungstests decken Unicode, benannte und
positionale Werte sowie sprachunabhängige Abbruchsteuerung ab.

Ein Slint-Test lädt alle vier eingebetteten Übersetzungen und aktualisiert die Ansichtsmodelle.
Dabei bleiben Kategorien-IDs, Filter, Kontonamen, Gruppen, Buchungstexte und Formularentwürfe
unverändert. Die Tests verwenden synthetische Daten und ändern keine produktiven Einstellungen.

## Kontonamen

`accounts_ui::tests` prüft lokale Anzeigenamen, unveränderte Bankstammdaten,
Kontenabruf und alte Tresorformate, verschlüsseltes Speichern und erneutes Öffnen,
Abbrechen und Zurücksetzen sowie Rollback bei fehlgeschlagener Speicherung.
Die Ansichtsmodelle übernehmen den Namen bei erhaltener Kontoauswahl.
Drag-and-drop wird mit tatsächlich umgebrochenen Zeilen, nach Umordnung und
Fenstergrößenänderung in beide Richtungen geprüft.

Für die Offline-Sichtprüfung von Übersicht, Konten und Umbenennungsdialog:

```sh
cargo test -p bankingcraft --locked render_account_names -- --ignored
```

Der Export verwendet synthetische Konten mit langen Namen und schreibt deutsche
und französische Ansichten bei 1100 und 1360 Pixel Breite nach
`bankingcraft-account-name-review` im temporären Systemverzeichnis.
Die Sichtprüfung am 13.11.2026 bestätigte vollständig sichtbare Namen,
passende Zeilenhöhen und lesbare Aktionen sowie den Umbenennungsdialog.
Der Export umfasst außerdem die DDV- und RDH-Bankzugangsformulare. Die Controls
für Kartenleser, Karteneintrag und Kartenabruf sind bei beiden Fensterbreiten
vertikal zentriert und überlappen sich nicht; Hinweistexte haben eine eigene Zeile.

## Englische Demo-Screenshots

Die fünf README-Ansichten werden mit den echten Slint-Seiten und ausschließlich synthetischen,
englisch beschrifteten Demo-Daten gerendert:

```sh
cargo test -p bankingcraft --locked export_english_showcase -- --ignored
```

Der Export schreibt Übersicht, Konten, Umsätze, Überweisung und Auswertungen als PNG nach
`docs/screenshots`. Er öffnet keinen vorhandenen Tresor, liest keine lokale Produkt-ID und
kontaktiert keine Bank. Optional erzeugen `BANKINGCRAFT_DEMO_VAULT` (ein neuer Pfad außerhalb
des Repositorys) und `BANKINGCRAFT_DEMO_PASSWORD` einen separaten verschlüsselten Demo-Tresor.
Vorhandene Dateien werden niemals überschrieben. Ohne diese Variablen werden nur die Bilder
neu erzeugt; die Daten verwenden Datumswerte relativ zum Ausführungstag.

## Grenzen und Sichtprüfung

Für eine reproduzierbare Offline-Sichtprüfung von Überweisungsformular und Auftragsliste:

```sh
cargo test -p bankingcraft --locked render_payment_pages -- --ignored
```

Der Test umfasst auch die Umsatzansicht mit dem Zurück-Button zur Kontenliste sowie
den VoP-Ergebnisdialog mit langen Bankhinweisen und schmalem Fenster.
Der Test rendert ausschließlich synthetische Demo-Daten als PNG-Dateien nach
`bankingcraft-payment-review` im temporären Systemverzeichnis. Geprüft werden Deutsch
und Französisch bei 1100 und 1360 Pixel Breite. Die reguläre Testsuite überspringt
diesen reinen Export; die separate Ausführung und Sichtprüfung am 13.11.2026 war erfolgreich.
Checkbox, beide Überweisungsbuttons, kontobezogenes Adressbuch und alle Auftragsstatus
waren im dunklen Design lesbar und ohne abgeschnittene Beschriftungen.

Die Tests prüfen fachliches Verhalten und Protokollabläufe, nicht jede Codezeile. Eine erfolgreiche Bankantwort im Simulator ersetzt weder Bankzertifizierung noch einen Hardwaretest. Reale Zahlungen werden für diese Tests nicht ausgelöst. Der Test mit einem echten Kartenleser bleibt auf Benutzerwunsch ausgesetzt.

Zusätzlich wird die gebaute macOS-Anwendung geprüft: Tresor entsperren, Umsatzansicht mit vorgemerktem Umsatz, Kategoriechips, Kategorieauswahl und kompakte Umsatzzeilen. Status- und Kategoriechip müssen ihre Mitte auf derselben horizontalen Linie haben; Beschriftungen müssen innerhalb des Chips vertikal zentriert sein. Die tatsächliche Bankkommunikation der Postbank-Kreditkarte wurde separat lesend geprüft.

Am 13.11.2026 wurde das Erscheinungsbild in der installierten macOS-App geprüft:
ohne Einstellungsdatei das vorhandene dunkle Systemdesign übernommen, zwischen
Hell und Dunkel gewechselt, Hell trotz dunklem Systemdesign nach vollständigem
Beenden wiederhergestellt (einschließlich Entsperrbildschirm und Titelleiste) und
anschließend auf Systemeinstellung zurückgestellt. Finanzübersicht, Umsatzliste
mit vorgemerkten Umsätzen und Kategoriechips, Überweisungsformular sowie
Einstellungen wurden im dunklen Design visuell geprüft. Die Systemeinstellung
von macOS selbst wurde für diese Prüfung nicht geändert.

Am 13.11.2026 wurde die Lokalisierung in der neu gebauten und installierten
macOS-App geprüft: Ohne Sprachdatei erschien die deutsche Systemsprache.
Der Wechsel auf Französisch, Spanisch und Englisch wurde sofort angewendet.
Einstellungen und französisches Überweisungsformular wurden visuell auf
abgeschnittene Beschriftungen geprüft; die spanische Umsatzliste zeigte
übersetzte Kategorien, Datumsüberschriften und Vormerkungsstatus bei unveränderten
Banktexten. Nach vollständigem Beenden und Neustart blieb Englisch bereits auf
dem Entsperrbildschirm und anschließend in der Finanzübersicht erhalten.
Abschließend wurde die Sprache wieder auf Systemeinstellung (Deutsch) gesetzt.
Diese Sichtprüfung löste weder einen Bankabruf noch eine Zahlung aus.
