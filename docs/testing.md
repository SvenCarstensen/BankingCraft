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
| Überweisung | Betrags-, IBAN- und Textvalidierung, normale und Echtzeitüberweisung, kein Rückfall auf normale Überweisung, keine automatische Wiederholung bei Bankfehler/Antwortverlust | `client::business_flow_tests`, `business_data_tests`, `regression_tests` |
| Terminüberweisung | Fehlendes/heutiges Datum abweisen, zukünftiges Datum im Zahlungsdokument, Auftrags-ID auswerten, nicht unterstützte Bankfunktion | `client::business_flow_tests` |
| Dauerauftrag anlegen | Monatlich/wöchentlich, Rhythmus, Enddatum, Ultimo, Grenzen, Platzhalterdatum im Zahlungsdokument, TAN | `client::business_flow_tests`, `business_data_tests` |
| Dauerauftrag abrufen | HKCDB 1/2, Formataushandlung, mehrere Seiten, Ausführungszeit und letztes Datum, ungültige Versionen/Formate, unvollständige Seiten | `client::standing_regressions`, `client::business_flow_tests` |
| Dauerauftrag löschen | Originaldaten der Bank und Auftrags-ID erhalten, TAN, fehlende Bankunterstützung, fehlender lokaler Auftrag | `client::business_flow_tests`, `jobs::business_tests` |
| Nächste Ausführung | Schaltjahr, kurze Monate, Quartal, Zweiwochenrhythmus, inklusives Enddatum, Ultimo minus 1/2 Tage | `business_tests` |
| TAN und Freigabe | Zulässige Verfahrensauswahl, TAN-Folgeauftrag ohne Zahlungswiederholung, Abbruch, App-Freigabe mit Statusabfrage, fehlerhafte Challenge, SCA bei lesenden Aufträgen, PIN-Verwerfung | `client::business_flow_tests` |
| TAN-Darstellung und Kartenprotokoll | Matrix-Nutzdaten und Längen, fehlerhafte Flickercodes, USB-BCD-TAN, Kartenstatus, APDU/TLV, DDV-Verschlüsselung und Signaturzähler | `business_data_tests`, `regression_tests`, `cardreader` |
| RDH | Simulierter kompletter RDH-7-Dialog, Bankschlüssel, Hashvergleich, Signatur/Ver- und Entschlüsselung, manipulierte/unsignierte Antworten | `rdh_flow_test`, `rdh::tests`, `regression_tests` |
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

## Grenzen und Sichtprüfung

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
