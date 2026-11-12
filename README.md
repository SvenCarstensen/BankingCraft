# Kontor – Online-Banking für Windows, Linux und macOS

Desktop-Banking in Rust mit Slint-Oberfläche. Aufbau angelehnt an klassische deutsche
Finanzsoftware: Navigationsleiste links, Kachel-Übersicht, Umsatzliste mit Tagessalden,
Zahlungsverkehr-Formulare, Auswertungen.

## Funktionen

| Bereich | Umsetzung |
|---|---|
| Kontenrundruf Girokonten | UPD-Kontenliste, IBAN/BIC (HKSPA), Saldo inkl. verfügbar (HKSAL v5–7), Umsätze camt.052 (HKCAZ) mit Fallback MT940/MT942 (HKKAZ v5–7), vorgemerkte Umsätze, Aufsetzpunkte, Duplikaterkennung |
| PIN/TAN | FinTS 3.0, PSD2-SCA (HKTAN v6/v7, Prozess 4/2/S), Systemsynchronisation, TAN-Verfahren-/Medienwahl, Decoupled (App-Freigabe) |
| TAN-Darstellung | chipTAN optisch (animierter Flickercode, Tempo/Größe einstellbar), photoTAN/QR-TAN (Bild aus HHDuc), SMS/Text |
| chipTAN USB | HHDuc an den Leser per `FEATURE_MCT_READER_DIRECT` (SCardControl) oder `FF 91 06 00`-APDU, TAN wird übernommen |
| HBCI-Chipkarte (DDV) | DDV-0/DDV-1, Bankdaten/Signaturzähler von der Karte, RIPEMD-160 + DES-Retail-MAC durch die Karte, 2-Key-3DES-CBC-Verschlüsselung, HBCI über TCP-Port 3000 |
| Sichere PIN-Eingabe | `FEATURE_VERIFY_PIN_DIRECT` (Klasse 2/3) – PIN verlässt den Leser nicht; Soft-PIN nur bei Lesern ohne Pinpad |
| Zahlungsverkehr | SEPA-Überweisung (HKCCS), Echtzeit (HKIPZ), Terminüberweisung (HKCSE), Daueraufträge anlegen/abrufen/löschen (HKCDE/HKCDB/HKCDL), pain.001.001.03/.001.003.03/.001.001.09 |
| Auswertungen | Automatische Kategorisierung mit lernenden Regeln, Monats- und 12-Monats-Auswertung |
| Export | CSV (Excel-tauglich, `;`, deutsches Zahlenformat) und camt.052.001.08 in den Download-Ordner |
| Datenschutz | Lokaler Tresor: AES-256-GCM, Schlüssel per Argon2id aus dem Tresor-Passwort; PINs werden nie gespeichert |

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

Voraussetzungen: Rust ≥ 1.80, plus

- **Linux:** `libpcsclite-dev`, `pcscd`, `libfontconfig1-dev`, `libxkbcommon-dev`
- **macOS:** Xcode Command Line Tools (PC/SC über `PCSC.framework`)
- **Windows:** keine (WinSCard ist Teil von Windows)

```bash
cargo build --release
./target/release/kontor          # Windows: target\release\kontor.exe
```

**macOS-App:** `macos/build-macos.command` per Doppelklick ausführen – baut `Kontor.app` neben dem Projektordner (mit Icon, ad-hoc signiert) und startet sie.

Ohne GPU/OpenGL (z. B. VMs): `SLINT_BACKEND=winit-software ./target/release/kontor`.
Protokoll-Logging: `RUST_LOG=debug` (zeigt FinTS-Nachrichten inkl. PIN im Klartext – nur zum Debuggen).

## Erste Schritte

1. Tresor-Passwort festlegen.
2. **Einstellungen → FinTS-Produktregistrierung**: Banken akzeptieren FinTS-Zugriffe nur mit
   einer bei der Deutschen Kreditwirtschaft registrierten Produkt-ID (kostenlos, fints.org).
3. **Bankzugänge → Neuer Bankzugang**: PIN/TAN mit BLZ, Kennung und FinTS-URL, oder
   HBCI-Chipkarte: Leser wählen, „Daten von Karte lesen“.
4. **Kontenrundruf** auf der Übersicht.

Ohne Bankzugang: „Demo-Daten laden“ legt einen lokalen Demo-Zugang an; Überweisungen
zeigen dort einen Beispiel-Flickercode und werden nur lokal simuliert.

## Aufbau

```
crates/fints       FinTS 3.0: Syntax, Nachrichten, Sicherheit (PIN/TAN, DDV), BPD/UPD,
                   Geschäftsvorfälle, MT940, camt.052, pain.001, Flickercode
crates/cardreader  PC/SC: DDV-Chipkarte, chipTAN USB, Pinpad, Diagnose
app                Slint-Oberfläche, Tresor, Kategorisierung, Hintergrund-Jobs
```

## Stand der Verifikation

- Flickercode-Erzeugung gegen die sechs Referenzvektoren der hbci4java-Testsuite geprüft (identisch).
- Nachrichtenaufbau, Escaping, Binärdaten, MT940-/SEPA-Feldzerlegung, pain.001-Roundtrip und IBAN-Prüfung geprüft.
- DDV- und chipTAN-USB-APDU-Folgen 1:1 aus hbci4java (`smartcardio`) übernommen.
- **Nicht** gegen einen echten Bankserver oder echte Chipkarten getestet – erste Läufe bitte mit `RUST_LOG=debug`.
- Lastschriftrückgabe ist kein FinTS-Geschäftsvorfall und daher nicht enthalten; Daueraufträge
  ändern = löschen + neu anlegen.
