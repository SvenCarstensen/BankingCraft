# BankingCraft: RDH-Prüfung und Korrekturen

Stand: 12.11.2026. Geprüft wurde der vorhandene, teilweise noch nicht eingecheckte
Quellstand. Diese Änderungen bleiben erhalten. Das Projekt wurde einschließlich Git-Verlauf
und lokaler Änderungen aus `~/Claude/kontor` nach
`/Users/svencarstensen/Documents/Entwicklung/Source/Eigene Projekte/BankingCraft` verschoben.
46 Quelldateien wurden vor/nach dem Verschieben per SHA-256 verglichen.

## Ergebnis

**BankingCraft ist nicht als gleichwertig zu GnuCash/AqBanking bestätigt.** Der Kern für
bereits initialisierte RDH-7/9/10-Chipkarten ist vorhanden. Der Ausgangsstand hatte jedoch
Sicherheitslücken bei der Antwortprüfung sowie Fehler im Kartenprotokoll. Diese wurden
korrigiert. Ein erfolgreicher realer Bankabruf mit dieser Implementierung steht aus.

GnuCash implementiert RDH nicht selbst: Es bindet AqBanking ein. AqHBCI implementiert
Dialog und Nachrichtenkryptografie, Gwenhywfar stellt Kryptografie-/Token-Abstraktionen,
libchipcard den ZKA-Kartenzugriff bereit.

## Referenzen und Vergleich

- [GnuCash: AqBanking](https://code.gnucash.org/wiki/AqBanking).
- AqBanking, Commit `33504e378993d9cdb6702bd90c7039ef378fe31f` (24.09.2026):
  [job_crypt.c](https://github.com/aqbanking/aqbanking/blob/33504e378993d9cdb6702bd90c7039ef378fe31f/src/libs/plugins/backends/aqhbci/joblayer/job_crypt.c),
  [msgcrypt_rxh_common.c](https://github.com/aqbanking/aqbanking/blob/33504e378993d9cdb6702bd90c7039ef378fe31f/src/libs/plugins/backends/aqhbci/msglayer/msgcrypt_rxh_common.c),
  [msgcrypt_rxh_verify.c](https://github.com/aqbanking/aqbanking/blob/33504e378993d9cdb6702bd90c7039ef378fe31f/src/libs/plugins/backends/aqhbci/msglayer/msgcrypt_rxh_verify.c).
- libchipcard, Commit `c0d0f19d36674baf06032731ea6b1830aa31b30f` (11.04.2025):
  [zkacard.c](https://github.com/aqbanking/libchipcard/blob/c0d0f19d36674baf06032731ea6b1830aa31b30f/src/ct/zkacard/zkacard.c),
  [ZKA-Kartenformat](https://github.com/aqbanking/libchipcard/blob/c0d0f19d36674baf06032731ea6b1830aa31b30f/src/libchipcard/cards/zkacard/zkacard-app.xml).

Gemeinsame Grundverfahren: doppelte SHA-256-Hashbildung, RSA-PSS, 2-Key-3DES-CBC,
RSA-PKCS#1-v1.5-Schlüsseltransport bei RDH-7/9 und Modus 2 bei RDH-10. ZKA:
MSE-Algorithmus 46 zum Signieren, 1A zur Entschlüsselung, EF_KEYD/EF_NOTEPAD,
PIN-Eingabe am Leser. Diese Übereinstimmung im Code beweist keine Interoperabilität.
AqBanking prüft erwartete Verschlüsselung und Signaturen; bei bestimmten unsignierten
Antworten bietet es eine ausdrückliche Nutzerentscheidung. BankingCraft lehnt solche
persönlichen RDH-Antworten jetzt strikt ab. Anonyme Schlüsselabfragen sind separat.

## Behobene Befunde

| Befund im Ausgangsstand | Korrektur und Nachweis |
|---|---|
| Unsignierte Klartext-/verschlüsselte RDH-Antworten und Antworten ohne Banksignaturschlüssel wurden akzeptiert | Vollständige Verschlüsselung und gültige Banksignatur zwingend; fünf ursprüngliche Fehlverhalten offline reproduziert und als Regressionen abgedeckt |
| Klartextsignatur wurde nicht geprüft; HNSHK/HNSHA-Kontrollreferenzen und ungeschützte Zusatzsegmente nicht ausreichend geprüft | Signaturstruktur, Profil, Algorithmen und Kontrollreferenzen prüfen; Klartext im persönlichen RDH-Dialog ablehnen |
| Bankantwort konnte falschem Dialog/Nachrichtenzähler zugeordnet werden | Länge, Header/Trailer, Dialog-ID und Nachrichtennummer prüfen |
| RDH-Kartenrecord wurde bei Fehlschlag durch den ersten Zugang ersetzt | Exakte Zuordnung nach gewähltem Record, BLZ und Benutzerkennung; keine stillschweigende Ausweichzuordnung |
| EF_KEYD wurde als tatsächlicher Modulus statt Längenbeschreibung gelesen; öffentliche Schlüsselslots konnten gewählt werden | Private Slots wählen; D0-Deskriptoren auswerten; fehlende/ungültige Schlüssellänge ablehnen; 2048-/3072-Bit-Fixtures |
| Erweitertes APDU-Le 256 wurde als 0000 (=65536) codiert | 0100 für 256, erweiterte Le auch bei kurzen Nutzdaten; Signaturlänge gegen Kartenschlüssel prüfen |
| Veralteter Signaturzähler und zu kurze Software-PIN konnten verwendet werden | Zähler frisch lesen und bei Lesefehler abbrechen; Mindestlänge beachten |
| Unbeschränkt große TCP/HTTP-Antworten, rohe Nachrichten im Debug-Log, ungesichertes PIN/TAN-HTTP möglich | Größenlimits, Verbindungszeitlimit, HTTPS-Pflicht ohne Redirects, Protokoll nur mit Segmentkennungen |
| Manipulierte Binärlängen/Unicode-Datumsfelder konnten Panics auslösen | Begrenzte/prüfende Indizierung; Regressionen für Parser und Flicker-Challenge |
| Ungültige camt-Werte wurden als Null/Standarddatum interpretiert, Parserfehler oder Seitenlimits verschwiegen | Fehler weitergeben; unvollständige Abrufe nicht als vollständigen Erfolg zurückgeben |
| Zahlungsbeträge wurden still gerundet; Feld-/Terminlimits fehlten | Maximal zwei Nachkommastellen, positive Beträge, SEPA-Feldlängen und Dauerauftragstermine validieren |
| SEPA-IDs konnten nach Umbenennung zu lang werden | Kurze, eindeutige BC-IDs innerhalb der 35-Zeichen-Grenze |
| Passwortwechsel konnte in Speicher und Datei auseinanderlaufen | Alle gespeicherten PINs umschlüsseln; Kandidaten zuerst atomar speichern, danach Speicherzustand wechseln |
| Temporäre Tresordatei hatte keine exklusiven/privaten Rechte | Zufälliger temporärer Name, create_new, Unix 0600, sync_all, atomare Umbenennung |
| Doppelklick konnte parallele Bankjobs auslösen; erneute Dauerauftragserstellung nach Folgefehler denkbar | Synchroner Jobschutz; erfolgreiche Erstellung und fehlgeschlagene Listenaktualisierung getrennt melden |
| Konten mit gleicher Nummer und unterschiedlichen Unterkonten konnten zusammenfallen | Unterkontomerkmal und BLZ beim Nummernabgleich berücksichtigen |

## Grenzen

- Keine Software-Schlüsseldateien, INI-Erstinitialisierung, Schlüsseländerung/-sperre oder
  vollständigen Zertifikatsverfahren. AqBanking unterstützt eine breitere Kombination von
  Profilen und Medien; nicht jede denkbare Sicherheitsklasse wird dort unterstützt.
- Die neuen RSA-Tests verwenden einen simulierten Karten-/Bankpartner. Teile der Nachrichten
  werden durch denselben Builder auf beiden Seiten erzeugt. Das kann gemeinsame Fehler verdecken.
- RDH-7 ist als simulierter kompletter Dialog getestet; RDH-9/10, echte ZKA-Karten,
  PIN-Pads und unterschiedliche Banken benötigen weiterhin Integrationstests.
- DDV wurde nicht auf eine gleichwertige Prüfung der Banksignatur erweitert.
- camt-Sammelbuchungen mit mehreren TxDtls werden weiterhin nicht vollständig in einzelne
  Umsätze aufgeteilt. Das ist keine Bestätigung einer vollständigen camt-Unterstützung.
- Kein vollständiges unabhängiges Sicherheitsaudit; keine Zusage der Fehlerfreiheit.
- Für einen realen Abruf sind die vorhandene Chipkarte, ihre PIN-Freigabe und eine gültige
  FinTS-Produktregistrierung von BankingCraft notwendig. Bank Xs Produkt-ID wird nicht kopiert.

## Bank-X-Übernahme

Aus den sichtbaren Kontenmappeneinstellungen wurde ein RDH-7-Zugang mit acht Kontoverbindungen
vorbereitet. Kontodaten liegen ausschließlich in einer privaten lokalen Importdatei außerhalb
von Git. Der neue Importpfad validiert Kontoverbindungen, ergänzt sie nach Tresorentsperrung,
sichert den bisherigen verschlüsselten Tresor und speichert atomar. Wiederholung erzeugt
keine Duplikate. Bestehende Bankschlüssel und Zugangseinstellungen werden nicht ersetzt.
PINs, private Schlüssel, historische Buchungen und alte Salden werden nicht übernommen.
Die Übernahme wurde in der App nach dem Entsperren bestätigt: acht zusätzliche Konten,
insgesamt 13. Der reale Kartentest wurde auf Nutzerwunsch ausgelassen, da der Leser
nicht angeschlossen ist.

## Umbenennung und Icon

Produkt, Cargo-Binary, Oberfläche, Bundle und Exportnamen heißen BankingCraft.
Tresor-Dateiname, Speicherort, Dateimagic und Schlüsselableitung bleiben kompatibel.
Das B-Icon wurde mit dem eingebauten ImageGen bearbeitet; Quelldatei:
`macos/bankingcraft-icon.png`. Das ursprüngliche K-Icon bleibt als Ausgangsdatei erhalten.

Finaler ImageGen-Prompt (Edit mit Bildreferenz und transparentem Hintergrund):

> Use case: precise-object-edit. Edit target: BankingCraft B app icon. Clean the alpha edge only: remove ALL stray blue, cyan, purple pixels and artifacts OUTSIDE the rounded blue square. Exterior must be perfectly transparent. Make edge antialiasing clean and smooth; preserve centered white B, existing gradient and rounded square proportions. Flat crisp vector-like finish with no grain or texture. Deliver a square 1024x1024 macOS icon with transparent margins.

Die erzeugte Rasterdatei hat 1280×1280 Pixel; beim Bundlebau wird sie mit sips auf
macOS-Icongrößen skaliert.

## Verifikation

`cargo test --workspace --locked` prüft lokale Parser, Kryptografie, simulierten RDH-Dialog,
Import-Wiederholung/Validierung/verschlüsseltes Speichern sowie Passwortwechsel und Rollback.
Der Release-Build und die macOS-Bundleprüfung werden zusätzlich durchgeführt. Ein lokaler
Build oder erfolgreicher Import ist noch kein bestätigter Bankabruf.

## Nachbesserung nach Postbank-Abruf

Ein realer Postbank-Abruf meldete, dass camt-Buchungs-/Valutadaten fehlen. Beide
Datumsfelder sind optional; ihr Fehlen darf den Umsatzabruf nicht abbrechen. Fehlende
Daten werden jetzt ausdrücklich als unbekannt gespeichert und in der Oberfläche als
„nicht geliefert“ dargestellt; CSV/XML exportieren dafür kein erfundenes Datum.
Ein internes Bericht-/Abrufdatum dient nur der Sortierung. Nicht leere ungültige
Datumswerte werden weiterhin abgelehnt. HICAZ-Vormerkungen bleiben auch ohne eigenen
XML-Status vorgemerkt. Pending-/undatierte Umsätze fließen nicht in Monatsauswertungen ein.
Die Änderung ist mit synthetischen gemischten BOOK-/PDNG-Beispielen und Datum/Uhrzeit
getestet. Der reale Postbank-Abruf wurde anschließend erfolgreich wiederholt;
die Abschlussmeldung lautete „6 Konten geprüft, 0 neue Umsätze“, ohne Datumsfehler.

Zusätzlich wurden frei benennbare Kontengruppen umgesetzt. Zuordnung, Umbenennung
und Entfernen werden atomar im bestehenden Tresor gespeichert; Entfernen löscht keine
Konten. Serialisierung, alte Tresore ohne Gruppen und eindeutige Zuordnung sind getestet.

## Abschließender Stand

- 33 Tests erfolgreich (8 App, 4 Kartenprotokoll, 21 FinTS), zusätzlich Doc-Tests.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: erfolgreich, keine Warnungen.
- `cargo build --release --locked`: erfolgreich; macOS-Plist und Ad-hoc-Bundlesignatur geprüft.
- BankingCraft.app geöffnet und bestehender Tresor erfolgreich weiterverwendet.
- Bank-X-Import und individuelle Gruppenzuordnung in der Oberfläche bestätigt:
  „Privat“ mit fünf Postbank-Konten, „VR Bank“ mit acht VR-Bank-Konten.
- Die Bank liefert beim Postbank-Abruf sechs Kontodatensätze; die bestehende lokale
  Zuordnung umfasst fünf Konten. Dies ist von der Gruppenzählung zu unterscheiden.
- Reale Postbank-Abfrage nach camt-Korrektur ohne Datumsfehler abgeschlossen.
- Kein realer Kartenabruf auf ausdrücklichen Nutzerwunsch; keine Zahlung gesendet.

## Ergänzung: Übersicht, Kontenreihenfolge und Dauerauftragsabruf

Die Übersicht zeigt Gruppensalden aus den gebuchten Kontosalden. Verschiedene Währungen
bleiben getrennt, unvollständige Gruppen sind als Teilsaldo gekennzeichnet, vollständig
fehlende Salden als unbekannt. Der Abrufknopf heißt „Umsätze abrufen“.

Die Kontenliste lässt sich per Drag-and-drop sortieren. Die bestehende Reihenfolge im
verschlüsselten Tresor wird atomar gespeichert; Bankabrufe und Neustarts erhalten sie.
Gruppenzuordnungen und ausgewählte Quellkonten in den Umsatz-/Zahlungsformularen werden
anhand stabiler Konto-IDs erhalten.

Der Dauerauftragsabruf verwendete bislang fest HKCDB-Version 1. Er handelt jetzt die
höchste gemeinsam unterstützte Version (1 oder 2) aus und wählt ein unterstütztes
pain.001-Format aus den Dauerauftragsparametern. Bei HICDB-Version 2 steht vor dem
Enddatum die Ausführungszeit; beide Felder werden getrennt gelesen. Bankfehler ergeben
keine irreführende Erfolgsmeldung mit null Aufträgen, und betroffene lokale Bestände
bleiben erhalten. Referenz für die Feldfolge:
[hbci4java hbci-300.xml, Stand 7482dcdb](https://github.com/hbci4j/hbci4java/blob/7482dcdb85b1e6e879501f87e23c3264adc393c3/src/main/resources/hbci-300.xml).

Offline-Verifikation: 38 Tests erfolgreich (11 App, 4 Kartenprotokoll, 23 FinTS),
einschließlich Saldenberechnung, Sortierung/Speicherung, Version 1/2 mit mehrseitigen
Dauerauftragsantworten, Ausführungszeit und Ablehnung inkompatibler Formate/Versionen.
Clippy mit `-D warnings` ist erfolgreich.

Der reale Postbank-Bestandsabruf nach der Korrektur meldete „2 Daueraufträge geladen“;
beide Einträge werden in der App angezeigt. Gruppensalden und die neue Buttonbeschriftung
wurden in der aktualisierten App geprüft. Das Umordnen wurde per Maus ausgeführt und
nach Neustart bestätigt; anschließend wurde die vorherige Kontenreihenfolge wiederhergestellt.
Der Kartenlesertest bleibt auf Nutzerwunsch ausgelassen.
