# BankingCraft – Online-Banking für Windows, Linux und macOS

Desktop-Banking in Rust mit Slint-Oberfläche. Aufbau angelehnt an klassische deutsche
Finanzsoftware: Navigationsleiste links, Kachel-Übersicht, Umsatzliste mit Tagessalden,
Zahlungsverkehr-Formulare, Auswertungen.

Die App hieß zuvor Kontor. Bestehende Tresore werden weiterhin am bisherigen Speicherort
gefunden; Dateiformat, Dateiname `vault.kontor` und Schlüsselableitung bleiben kompatibel.

## Funktionen

| Bereich | Umsetzung |
|---|---|
| Kontenrundruf Girokonten | UPD-Kontenliste, IBAN/BIC (HKSPA), Saldo inkl. verfügbar (HKSAL v5–7), Umsätze camt.052 (HKCAZ) mit Fallback MT940/MT942 (HKKAZ v5–7), vorgemerkte Umsätze, Aufsetzpunkte, Duplikaterkennung |
| Kreditkarten | Umsatzabruf HKKKU v1 / DKKKU v2, Saldo HKKKS v1 / DKKKS v3 bzw. aus der Umsatzantwort; Auswahl anhand der Kontoberechtigungen, mehrseitige Antworten, Soll/Haben und Vormerkungen |
| PIN/TAN | FinTS 3.0, PSD2-SCA (HKTAN v6/v7, Prozess 4/2/S), Systemsynchronisation, TAN-Verfahren-/Medienwahl, Decoupled (App-Freigabe) |
| TAN-Darstellung | chipTAN optisch (animierter Flickercode, Tempo/Größe einstellbar), photoTAN/QR-TAN (Bild aus HHDuc), SMS/Text |
| chipTAN USB | HHDuc an den Leser per `FEATURE_MCT_READER_DIRECT` (SCardControl) oder `FF 91 06 00`-APDU, TAN wird übernommen |
| HBCI-Chipkarte (DDV) | DDV-0/DDV-1, Bankdaten/Signaturzähler von der Karte, RIPEMD-160 + DES-Retail-MAC durch die Karte, 2-Key-3DES-CBC-Verschlüsselung, HBCI über TCP-Port 3000 |
| RDH-Chipkarte | ZKA/SECCOS-Signaturkarte (z. B. VR-NetWorld-Card), Profile RDH-7/9/10: Bankzugänge aus dem Notizblock der Karte, Bankschlüssel per HKISA mit Prüfung gegen den Hashwert auf der Karte, RSA-PSS-Signatur durch die Karte, Signaturzähler der Karte, 2-Key-3DES + RSA-Schlüsseltransport (PKCS#1 v1.5 bei RDH-7/9, Modus 2 bei RDH-10), Prüfung der Banksignatur |
| Sichere PIN-Eingabe | `FEATURE_VERIFY_PIN_DIRECT` (Klasse 2/3) – PIN verlässt den Leser nicht; Soft-PIN nur bei Lesern ohne Pinpad |
| Zahlungsverkehr | SEPA-Überweisung (HKCCS), Echtzeit (HKIPZ), Terminüberweisung (HKCSE), Daueraufträge anlegen/abrufen/löschen (HKCDE/HKCDB/HKCDL), pain.001.001.03/.001.003.03/.001.001.09 |
| Auswertungen | Automatische Kategorisierung mit lernenden Regeln, Monats- und 12-Monats-Auswertung |
| Export | CSV (Excel-tauglich, `;`, deutsches Zahlenformat) und camt.052.001.08 in den Download-Ordner |
| Bankverzeichnis | 2.787 Institute mit PIN/TAN-URL bzw. HBCI-Host, Suche nach Name, Ort, BLZ, BIC (umlauttolerant), eingebettet in `app/data/banks.tsv` |
| Datenschutz | Lokaler Tresor: AES-256-GCM, Schlüssel per Argon2id aus dem Tresor-Passwort; PIN/TAN-PINs werden nur auf ausdrücklichen Wunsch zusätzlich verschlüsselt gespeichert; Karten-PINs nicht |

## REINER SCT cyberJack (secoder)

Leser mit „cyberJack“/„REINER SCT“ im PC/SC-Namen werden erkannt und bevorzugt.
Die Seite **Kartenleser → Leser & Karte prüfen** zeigt, welche PC/SC-Part-10-Features
das Gerät meldet (sichere PIN-Eingabe, MCT/chipTAN USB, Display), ob eine HBCI-Chipkarte
steckt und welche Bankzugänge darauf gespeichert sind.

Die proprietäre Secoder-Visualisierung (Anzeige von Auftragsdaten bei HBCI-Signaturen) ist
nicht öffentlich spezifiziert und daher nicht umgesetzt. Bei chipTAN USB zeigt der Leser die
Auftragsdaten über das standardisierte HHDuc-Verfahren an.

Treiber: macOS und Windows über REINER SCT, Linux über `pcscd` + `ifd-cyberjack`.

## Bauen

Voraussetzungen: aktuelles stabiles Rust (getestet mit 1.99.1), plus

- **Linux:** `libpcsclite-dev`, `pcscd`, `libfontconfig1-dev`, `libxkbcommon-dev`
- **macOS:** Xcode Command Line Tools (PC/SC über `PCSC.framework`)
- **Windows:** keine (WinSCard ist Teil von Windows)

```bash
cargo build --release
./target/release/bankingcraft          # Windows: target\release\bankingcraft.exe
```

**macOS-App:** `macos/build-macos.command` per Doppelklick ausführen – baut `BankingCraft.app` neben dem Projektordner (mit Icon, ad-hoc signiert) und startet sie.

Ohne GPU/OpenGL (z. B. VMs): `SLINT_BACKEND=winit-software ./target/release/bankingcraft`.
Protokoll-Logging: `RUST_LOG=debug`. Nachrichteninhalte und PIN/TAN werden nicht ausgegeben;
Fehlerprotokolle enthalten nur technische Segmentübersichten, keine freien Banktexte.

## Erste Schritte

1. Tresor-Passwort festlegen.
2. **Einstellungen → FinTS-Produktregistrierung**: Banken akzeptieren FinTS-Zugriffe nur mit
   einer bei der Deutschen Kreditwirtschaft registrierten Produkt-ID (kostenlos, fints.org).
3. **Bankzugänge → Neuer Bankzugang**: Bank über die Suche (Name, Ort, BLZ oder BIC) wählen –
   BLZ und FinTS-URL bzw. HBCI-Host werden übernommen. Dann PIN/TAN-Kennung eintragen, oder
   HBCI-Chipkarte: Leser wählen, „Daten von Karte lesen“.
4. **Umsätze abrufen** auf der Übersicht.

Ohne Bankzugang: „Demo-Daten laden“ legt einen lokalen Demo-Zugang an; Überweisungen
zeigen dort einen Beispiel-Flickercode und werden nur lokal simuliert.

Fensterposition und -größe werden beim Schließen und Beenden gespeichert und beim nächsten
Start wiederhergestellt, auch bei gesperrtem Tresor. Die Datei `window-state.json` neben dem
Tresor enthält nur Fenstereinstellungen. Ist ein zuvor verwendeter Bildschirm nicht mehr
angeschlossen, wird das Fenster auf einen erreichbaren Bildschirm verschoben. Unter Wayland
kann der Fenstermanager das Wiederherstellen einer absoluten Position einschränken.

## Kontengruppen

Unter **Konten** können beliebige Gruppen angelegt und umbenannt werden. Jedes Konto
wird über seine Gruppenauswahl zugeordnet. „Ohne Gruppe“ hebt die Zuordnung auf. Beim
Entfernen einer Gruppe bleiben Konten, Bankzugänge und Umsätze erhalten. Die Gruppen
werden zusammen mit dem Tresor verschlüsselt gespeichert.

Die Übersicht zeigt je Gruppe die Summe der bekannten gebuchten Salden, getrennt nach
Währung. Fehlende Salden werden gekennzeichnet und nicht als null gewertet. In der
Kontenliste der Übersicht können Konten mit der Maus an die gewünschte Position gezogen
werden. Die Einfügelinie zeigt die Zielposition; die Reihenfolge bleibt nach Neustart und
Bankabruf erhalten. Die Gruppenzuordnung bleibt beim Sortieren bestehen.

Der Dauerauftragsabruf handelt HKCDB-Version 1 oder 2 mit der Bank aus und berücksichtigt
die dafür angebotenen pain.001-Formate. Version 2 liefert zusätzlich Ausführungszeiten.
Fehlgeschlagene Abrufe überschreiben keine vorhandenen Daueraufträge und werden als
Fehler bzw. Teilerfolg angezeigt.

## Aufbau

```
crates/fints       FinTS 3.0: Syntax, Nachrichten, Sicherheit (PIN/TAN, DDV, RDH), BPD/UPD,
                   Geschäftsvorfälle, MT940, camt.052, pain.001, Flickercode
crates/cardreader  PC/SC: DDV- und RDH-Chipkarte (ZKA/SECCOS), chipTAN USB, Pinpad, Diagnose
app                Slint-Oberfläche, Tresor, Kategorisierung, Hintergrund-Jobs
```

## Bankverzeichnis

`app/data/banks.tsv` stammt aus `blz.properties` von [hbci4java](https://github.com/hbci4j/hbci4java)
(LGPL-2.1, Stand Oktober 2026). Veraltete Adressen wurden umgeschrieben: `hbci-pintan.gad.de` →
`fints1.atruvia.de`, `hbci11.fiducia.de` → `fints2.atruvia.de`, ehemalige Dresdner-Bank-Adressen →
Commerzbank. Maßgeblich bleibt die Angabe der jeweiligen Bank; die URL ist im Formular editierbar.

## Kompatibilität (Abgleich mit AqBanking/GnuCash)

Nachrichtenaufbau und Dialogablauf folgen AqBanking, der FinTS-Bibliothek von GnuCash
(Verhalten nachgebaut, kein Code übernommen):

- System-ID per Einschritt-Dialog (Verfahren 999, Sicherheitsprofil `PIN:1`, ohne HKTAN),
  Zwei-Schritt-Dialoge mit `PIN:2` und HKTAN Prozess 4 → 2/S
- TAN-Medium (z. B. BestSign-Gerät) in HKTAN Feld 11 in allen Prozessschritten
- HKVVB Dialogsprache 1, SEPA-Aufträge mit vollständiger KTI, HKCDB/HKCDL-Feldfolge wie AqBanking
- Decoupled-Freigaben (BestSign, pushTAN-App) per HKTAN Prozess S, erkannt über HITANS oder Code 3955

### RDH (Chipkarte) im Vergleich zu AqBanking/GnuCash

**Prüfergebnis vom 12.11.2026: keine bestätigte Gleichwertigkeit.** GnuCash verwendet
AqBanking/AqHBCI und für ZKA-Karten libchipcard. BankingCraft bildet einen Teil dieses
Funktionsumfangs selbst in Rust nach.

| Bereich | BankingCraft nach der Prüfung |
|---|---|
| RDH-7/9/10-Kryptografie | SHA-256/SHA-256, RSA-PSS, 2-Key-3DES und profilabhängiger RSA-Schlüsseltransport; lokal mit RSA-2048 getestet |
| Antwortprüfung | Persönliche RDH-Antworten müssen vollständig verschlüsselt und gültig signiert sein; fehlende Schlüssel/Signaturen, widersprüchliche Kontrollreferenzen und ungeschützte Zusatzsegmente werden abgelehnt |
| Kartenzugriff | ZKA/SECCOS, Notizblock, private Schlüssel, frischer Signaturzähler, PIN-Pad; EF_KEYD-Deskriptoren und erweiterte APDU-Längen korrigiert |
| Vertrauensprüfung | Bankschlüssel gegen Kartenhash prüfen bzw. vom Nutzer prüfen lassen; keine ungeprüfte automatische Übernahme |
| Umfang | Bereits initialisierte RDH-Chipkarten; keine Software-Schlüsseldateien, INI-Erstinitialisierung, Schlüsseländerung/-sperre oder Zertifikatsverfahren |
| Praxisnachweis | Noch kein bestätigter Ende-zu-Ende-Abruf mit echter Bank und Chipkarte |

Befunde, Quellen, Korrekturen und Testgrenzen: [RDH-Codeprüfung](docs/rdh-review.md).

### Übernahme aus Bank X

BankingCraft kann vorbereitete RDH-Kontoeinstellungen beim nächsten Entsperren aus
`bankx-import.json` neben dem bestehenden Tresor übernehmen. Dies ist ein eigenes,
versioniertes Austauschformat, kein Parser für Bank-X-Kontenmappen oder Umzugsarchive.
Die Datei enthält nur Bankzugang und Kontoverbindungen; keine PINs, privaten Schlüssel,
Bankschlüssel, Salden oder Buchungen. Sie muss mit privaten Dateirechten angelegt werden.

Der Import validiert IBAN/BLZ/Kontonummer, ergänzt fehlende Konten ohne bestehende
Zugänge zu überschreiben, sichert den verschlüsselten Tresor als
`vault.before-bankx-import` und entfernt die Importdatei erst nach erfolgreichem Speichern.
Die Karte wird anhand BLZ und Benutzerkennung zugeordnet; der Leser wird bei leerer
Auswahl automatisch erkannt. Berechtigungen, Kontotypen und aktuelle Salden werden beim
Abruf von der Bank aktualisiert. Das Format ist in `app/src/bankx_import.rs` dokumentiert.

## Stand der Verifikation

Die fachlichen Regressionstests lassen sich mit `./scripts/test-business.sh` ausführen.
Eine Übersicht der Fälle und Grenzen steht in [docs/testing.md](docs/testing.md).
Die zusätzliche [Sicherheitsprüfung](docs/security-review.md) dokumentiert Korrekturen,
Abhängigkeitsscan und verbleibende Grenzen. Aktuell bestehen 132 automatisierte Tests.

- Flickercode-Erzeugung gegen die sechs Referenzvektoren der hbci4java-Testsuite geprüft (identisch).
- Nachrichtenaufbau, Escaping, Binärdaten, MT940-/SEPA-Feldzerlegung, pain.001-Roundtrip und IBAN-Prüfung geprüft.
- DDV- und chipTAN-USB-APDU-Folgen 1:1 aus hbci4java (`smartcardio`) übernommen.
- RDH-7: kompletter Dialog (Schlüsselabruf, Hashprüfung, Synchronisation, signierter und verschlüsselter
  Dialog, Prüfung der Banksignatur) gegen eine simulierte Bank mit echten RSA-2048-Schlüsseln getestet;
  Kartenformate (Notizblock, EF_KEYD, PIN-Format) mit Testdatensätzen.
- **Postbank PIN/TAN:** realer Konten-/Saldo-/Umsatzabruf am 12.11.2026 nach der camt-Korrektur erfolgreich abgeschlossen; Kreditkartensaldo und Kreditkartenumsätze ebenfalls erfolgreich abgerufen.
- **RDH:** noch nicht gegen eine echte Bank und Chipkarte getestet; der Leser war nicht angeschlossen.
- Echte Zahlungsaufträge wurden nicht gesendet oder getestet.
- Lastschriftrückgabe ist kein FinTS-Geschäftsvorfall und daher nicht enthalten; Daueraufträge
  ändern = löschen + neu anlegen.
