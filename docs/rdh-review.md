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

## PIN-Korrektur am 13.11.2026

Eine PIN-freie Diagnose bestätigte am angeschlossenen cyberJack ecom_a eine
ZKA/SECCOS-Karte mit globaler PIN-Referenz 3 und ASCII-Format. Der bisherige
VERIFY-Puffer enthielt unabhängig von der eingegebenen Länge acht Leerzeichen.
Der generische libchipcard-PC/SC-Pfad verwendet dagegen bei ASCII nur den
vier Byte langen Befehlskopf; der Leser ergänzt Lc und die tatsächlich eingegebene
PIN. Die bisherige Vorlage entspricht dem besonderen SCM-X32-Pfad, nicht dem
generischen Pfad. Diese Abweichung ist eine plausible Erklärung für die gemeldete
Ablehnung einer korrekten PIN; ein erfolgreicher PIN-Versuch ist damit noch nicht bewiesen.

BankingCraft verwendet jetzt die generische ASCII-Vorlage. Auch Software-PINs
werden mit ihrer tatsächlichen Länge übertragen; FPIN2 bleibt separat codiert.
Die Zugriffs-PIN muss eindeutig aus EF_PWDD hervorgehen. Die App probiert weder
andere PIN-Referenzen noch andere Codierungen aus und wiederholt einen gescheiterten,
abgebrochenen oder unklaren Versuch innerhalb der Kartenverbindung nicht.
EF_FBZ liefert den Fehlversuchszähler rein lesend; die Diagnose zeigt ihn an.
Bei null verbleibenden Versuchen wird kein VERIFY ausgelöst.

Referenz: [libchipcard generic_pcsc/card.xml](https://github.com/aqbanking/libchipcard/blob/c0d0f19d36674baf06032731ea6b1830aa31b30f/data/drivers/posix/generic_pcsc/card.xml).
Zusätzliche Regressionstests prüfen die Befehlsbytes und Fehlerabläufe mit
synthetischen PINs, ohne Hardware-PIN-Versuche.

Validierung: Alle 140 Workspace-Tests und Clippy mit `-D warnings` bestehen.
Der Release-Build wurde installiert und gestartet. Bei der abschließenden
PIN-freien Diagnose meldete die App jedoch keinen verfügbaren Kartenleser;
der tatsächliche Fehlversuchszähler konnte deshalb noch nicht gelesen werden.
Es wurde keine PIN-Prüfung ausgelöst. Die Bestätigung mit der echten Karte
und ein erfolgreicher VR-Bank-Abruf stehen weiterhin aus.

## Schlüsselerkennung am 13.11.2026

Beim VR-Bank-Abruf erschien „Schlüssel #2 nicht auf der Chipkarte“.
Eine rein lesende EF_KEYD-Diagnose zeigte, dass der Signaturschlüssel #2 und
der Entschlüsselungsschlüssel #3 vorhanden sind, jeweils mit 248 Byte
(1984 Bit). Nur der Authentisierungsschlüssel #4 hat 256 Byte (2048 Bit).
Der Parser verwarf bisher alle Schlüssel unter 256 Byte und meldete die
beiden benötigten Schlüssel deshalb fälschlich als fehlend.

Die Erkennung berücksichtigt jetzt auch 248-Byte-Schlüssel; private
Schlüsselreferenzen und gültige Längenbeschreibungen bleiben erforderlich.
Ein Regressionstest mit derselben Metadatenstruktur und synthetischem Zähler
reproduzierte den Fehler vor der Korrektur. Tests prüfen außerdem die Grenzen,
öffentliche Referenzen und die exakten APDU-Längen. Ein zusätzlicher simulierter
RDH-7-Dialog prüft Signatur und Entschlüsselung mit 1984-Bit-Kartenschlüsseln.

Der korrigierte produktive Kartenlesercode erkannte anschließend auf der
angeschlossenen Karte beide Schlüssel mit jeweils 1984 Bit. Diese Prüfung
erfolgte ohne PIN-Prüfung, Signatur oder Bankverbindung. Ein vollständiger
realer VR-Bank-Abruf ist damit noch nicht bestätigt.

Offline-Verifikation: 186 Workspace-Tests erfolgreich, zwei manuelle
Renderingtests ausgelassen; Clippy mit `-D warnings` ohne Befund.
Der Release-Build wurde in die vorhandene macOS-App übernommen, die
Ad-hoc-Signatur geprüft und die App bis zum Tresor-Entsperrbildschirm gestartet.

## Anonymer Bankschlüsselabruf am 13.11.2026

Nach der Schlüsselerkennung scheiterte der Abruf mit 9050/9800/9110.
Die Ursache war die fehlende FinTS-Produktregistrierung: Mit dem Standardwert
`BANKINGCRAFT` weist Atruvia die kombinierte V/S-Schlüsselanfrage irreführend
als „Falsche Segmentzusammenstellung“ zurück. Ein anonymer BPD-Abruf mit
demselben Produktnamen liefert den eindeutigen Fehler 9078 „Software nicht
als FinTS-Produkt registriert“.

Auf ausdrücklichen Nutzerwunsch wurde vorübergehend eine fremde Produkt-ID für
den lokalen Test verwendet. Ihr Wert wird nicht im Repository dokumentiert.
Die ursprüngliche Übergabe über `BANKINGCRAFT_DEFAULT_PRODUCT_ID` wurde später
entfernt: Nur ein expliziter Build mit `local-fints-test` liest die Test-ID zur
Laufzeit aus einer privaten Datei außerhalb des Projekts. Normale Builds
ignorieren diese Datei. Eine im Tresor hinterlegte eigene ID hat Vorrang;
die lokale Test-ID wird nicht in den Tresor übernommen.

Mit dieser Testkonfiguration lieferte der produktive FinTS-Client vom Server
`fints1.atruvia.de:3000` für die öffentliche BLZ 21763542 beide öffentlichen
Bankschlüssel: V und S, jeweils 2048 Bit, Nummer 7, Version 2; Rückmeldungen
0020 und 0010. Beide HKISA-Segmente müssen in derselben Anfrage stehen, wie
auch in den AqBanking-/HBCI4Java-Vorlagen. Eine einzelne HKISA-Anfrage liefert
nur BPD und keinen Schlüssel. Die kombinierte Anfrage bleibt deshalb erhalten.

Der Test verwendete ausschließlich synthetische Kartenmetadaten und die
anonyme Kundenkennung. Die anschließende Fingerabdruckbestätigung wurde
abgelehnt; es wurden keine Schlüssel gespeichert, keine persönlichen
Anmeldungen, PIN-Prüfungen oder Signaturen ausgelöst. Ein vollständiger
VR-Bank-Kontenabruf mit der echten Karte steht noch aus.

Die ursprüngliche HKISA-Segmentübersicht bleibt nun erhalten, wenn der Server
beim anschließenden anonymen Dialogende einen anderen Fehler meldet (im
Live-Test 9800). Abruffehler nennen außerdem den betroffenen Bankzugang.
Regressionstests prüfen die kombinierte Anfrage, RDH-7 mit 2048/1984 Bit,
fehlende Schlüssel, erhaltene bestehende Schlüssel und Fehlerprotokolle sowie
die Bankzuordnung nach einem teilweise erfolgreichen Kontenrundruf.

Validierung mit gesetzter Test-ID: 188 Workspace-Tests erfolgreich, zwei
manuelle Renderingtests ausgelassen; Clippy mit `-D warnings` ohne Befund.
Der Release-Build wurde in die vorhandene macOS-App übernommen. Test-ID im
Binary, übereinstimmende Build-UUID und Ad-hoc-Signatur wurden geprüft; die
App wurde bis zum Tresor-Entsperrbildschirm neu gestartet.

## Persönlicher RDH-Abruf am 13.11.2026

Der vom Nutzer freigegebene VR-Bank-Abruf wurde bei leerer Auftragsliste
gestartet. Nach der PIN-Eingabe am Kartenleser brach die App mit
„RDH-Antwort ist nicht vollständig verschlüsselt“ ab. Da diese Sicherheitsfehler
bisher keine Segmentübersicht speicherten, war die eigentliche Bankablehnung
nicht sichtbar.

Beim Quellvergleich wurde ein weiterer Protokollfehler festgestellt:
Die übermittelte Signaturschlüsselversion stammte aus EF_KEYD, obwohl die
bankseitigen Versionen im ausgewählten EF_NOTEPAD-Zugang maßgeblich sind.
Auch libchipcard übernimmt deshalb die Versionen aus dem Kontext
(`zkacard.c`, `LC_Crypt_TokenZka_GetContext`, Zeilen 772–788 der oben verlinkten
Revision). BankingCraft übernimmt jetzt Signatur-/Entschlüsselungsversion aus
dem Bankzugang, während die physischen Slots und Schlüssellängen erhalten
bleiben. Die RDH-7-Tests mit 2048 und 1984 Bit reproduzierten vorher die falsche
Version und bestehen nach der Korrektur.

Bei reinen unverschlüsselten Fehlerantworten werden nun ausschließlich
9xxx-Codes als nicht authentifizierte Diagnose an den Sicherheitsfehler
angehängt, mit Mengen-/Längenbegrenzung und ohne Steuerzeichen. Die Antwort
wird weiterhin verworfen; weder Erfolg noch Geschäftsdaten, System-ID oder
Bankparameter werden übernommen. Gemischte Antworten werden nicht ausgewertet.
Auch Sicherheitsfehler erhalten eine Segmentübersicht ohne Nachrichteninhalte;
bei verschlüsselten Anfragen sind zusätzlich nur die angefragten Segmentnamen
und -nummern enthalten.

Offline-Verifikation: 190 Workspace-Tests erfolgreich, zwei manuelle
Renderingtests ausgelassen; Clippy mit `-D warnings` ohne Befund. Die
Bestätigung dieser Korrektur im persönlichen Live-Abruf steht noch aus.

Nach Installation und erneuter PIN-Freigabe zeigte der nächste Abruf eine
verschlüsselte Antwort auf HKIDN/HKVVB. Der folgende Abbruch betraf jedoch die
Nachrichtenhülle; die bisherige Protokollierung zeigte dabei noch die vorherige
Antwort. Die Übersicht wird deshalb jetzt vor der Hüllenprüfung aktualisiert.
Sie enthält zusätzlich ausschließlich numerische Länge/Version/Zähler und das
boolesche Ergebnis des Dialog-ID-Vergleichs, keine Dialog-IDs. Reine
Fehlerantworten mit abweichenden Hüllendaten können ihre 9xxx-Codes als
nicht authentifizierte Diagnose anzeigen; alle bisherigen Hüllenprüfungen
bleiben verbindlich. Ein Regressionstest prüft die aktualisierte, datenarme
Übersicht bei einer abgewiesenen Antwort (Nachrichtennummer 9999).
Validierung dieses Diagnoseschritts: 191 Tests und Clippy erfolgreich.

## Erneuter Quellvergleich am 13.11.2026: Kartenzugriff und TCP-Dialog

Der nächste reale Abruf kam durch die persönliche Dialoginitialisierung. Erst
HKSPA, Nachricht 2, erhielt eine unverschlüsselte Ablehnung 9800 „FGW Die
Kundennachricht enthaelt ungueltige Daten“. Die Fehlerantwort hatte im Kopf die
Nachrichtennummer 2, im Abschluss dagegen 1. Die strikte Hüllenprüfung ist
deshalb korrekt; ihre Lockerung würde die Ursache nicht beheben.

Der erneute Vergleich umfasst die oben genannten unveränderten Referenzrevisionen.
Für den Kartenzugriff ist libchipcard maßgeblich, für FinTS-Nachrichten und
Verbindungen dessen Aufrufer AqBanking/AqHBCI.

| Bereich | Referenzverhalten und BankingCraft-Befund |
|---|---|
| Kartendateien | MF/EF_ID `0003`, EF_PWDD `0015`, EF_FBZ `0016`, DF_SIG `d27600006601` mit EF_KEYD `0013`, DF_NOTEPAD `d2760000254e500100` mit EF_NOTEPAD `a611` stimmen mit `zkacard-app.xml` überein. |
| PIN-Beschreibung und Leser | PWDD-Pseudo-OID bestimmt ASCII bzw. FPIN2 und Mindestlänge. Der generische PC/SC-ASCII-Pfad übergibt nur den vier Byte langen VERIFY-Kopf; FPIN2 verwendet den separaten Acht-Byte-Block. Die frühere ASCII-Korrektur entspricht `generic_pcsc/card.xml`. BankingCraft verwirft unbekannte Formate, während libchipcard teils ASCII annimmt. Keine automatische PIN-Wiederholung. |
| Private Kartenschlüssel | Private Referenzen `84`/`94`, D0-Längenbeschreibung und Auswahl der höchsten physischen Version entsprechen dem verwendeten ZKA-Pfad. Die früher ausgeschlossenen 248-Byte-Schlüssel werden erkannt. Der Record, nicht die FinTS-Schlüsselnummer, adressiert den Kartenbefehl. |
| Kontext und Versionen | Gleiche Signatur-/Chiffrierschlüsselnummern im Notepad kennzeichnen das RDH-Profil. Physische Schlüssel sind #2/#3; die bankseitigen Versionen kommen aus EF_NOTEPAD. Die bereits korrigierte Übernahme entspricht ausdrücklich `LC_Crypt_TokenZka_GetContext`. |
| Kartenkennung | Das vollständige rohe EF_ID wird als CID verwendet, wie in `LC_Crypt_TokenZka__ReadContextList`. Bei fehlender Kundenkennung gilt die Benutzerkennung. |
| Signaturzähler | Der 16-Bit-Komplementwert aus KEYD-Tag `91` wird verwendet und vor einer Signatur frisch gelesen. Ein Lesefehler führt nicht zum Wiederverwenden eines alten Zählers. |
| Signierbefehl | MSE `00 22 41 A4` mit `84 01 (Record\|80)` und `80 01 46`, danach INTERNAL AUTHENTICATE `00 88 00 00`: entspricht libchipcard `LC_ZkaCard_Sign` und `_isoManageSe`. Der Host hasht einmal SHA-256, die Karte übernimmt den zweiten Hash und RSA-PSS. |
| Entschlüsseln | MSE `00 22 41 B8`, Algorithmus `1A`; PSO DECIPHER `00 2A 80 86` mit vorangestelltem `00`: entspricht libchipcard. BankingCraft fordert die bekannte Moduluslänge als Le an, statt der maximalen Antwortlänge. Tests decken 248/256/384 Byte ab. |
| FinTS-Signatur | HNSHK 4, RDH-Profil, Funktion 2, Rolle/Bereich 1, Hash `1:6:1`, Signatur `6:10:19`, Schlüsseltyp S mit Profilnummer und Kontextversion entsprechen AqBanking für die hier verwendete Sicherheitsklasse. |
| FinTS-Verschlüsselung | HNVSK 3, Funktion 4, 2-Key-3DES-CBC; RSA-PKCS#1-v1.5-Schlüsseltransport (Modus 18) für RDH-7/9. HNVSK nutzt den öffentlichen V-Schlüssel der Bank. RDH-10 verwendet separat den rohen RSA-Modus 2. |
| Anonyme Schlüsselanfrage | HKIDN/HKVVB mit beiden HKISA-Segmenten V und S, Benutzer/Nummer/Version `999` in den Schlüsselanfragen, entsprechen `jobgetkeys.xml`. Beide Schlüssel wurden vom realen Server geliefert. |
| System-ID | Signierte, verschlüsselte HKIDN/HKVVB/HKSYN-Anfrage mit System-ID 0 entspricht `jobgetsysid.xml`. |
| TCP-Verbindung | **Neu gefundener Fehler:** AqBanking hält `dlg->ioLayer` vom Dialogbeginn bis zum Ende offen. BankingCraft erzeugte bisher für jedes `send()` einen neuen Socket und verlor damit die Bindung des Dialogs an die Verbindung. |

### Ursache, Korrektur und Nachweis

[AqBanking `dialog_hbci.c`](https://github.com/aqbanking/aqbanking/blob/33504e378993d9cdb6702bd90c7039ef378fe31f/src/libs/plugins/backends/aqhbci/msglayer/dialog_hbci.c)
legt den Socket nur an, wenn `dlg->ioLayer` noch nicht existiert. Alle folgenden
Send-/Receive-Aufrufe verwenden diese Verbindung, bis der Dialog sie schließt.

Ein PIN-freier Test mit ausschließlich öffentlichen/anonymen Angaben bestätigte
die Bedeutung für den Atruvia-Server: HKISA und nachfolgendes HKEND auf derselben
Verbindung ergaben 0010 „Nachricht entgegengenommen“ und 0100 „Dialog beendet“.
Das vorherige Dialogende über eine neue Verbindung hatte 9800 ergeben.

`TcpTransport` hält jetzt seinen Stream über alle Nachrichten eines Dialogs.
`Client::reset_dialog()` schließt ihn; der nächste Dialog eröffnet eine neue
Verbindung. Auch ein Ende ohne zugewiesene Dialog-ID räumt den Transport auf.
Nach einem Transportfehler ist die Verbindung gesperrt, bis explizit ein neuer
Dialog beginnt. Es gibt weder automatisches Wiederverbinden innerhalb des
Dialogs noch ein erneutes Senden einer Anfrage mit unklarem Ausführungsstatus.
HTTPS-Verhalten und sämtliche Signatur-, Verschlüsselungs- und Hüllenprüfungen
bleiben erhalten.

Drei lokale Socket-Regressionen prüfen mehrere Nachrichten über einen Socket
(einschließlich fragmentierter Antworten), das Schließen/Neuverbinden an
Dialoggrenzen und das Unterbinden einer Wiederholung nach abgebrochener Antwort.
Der Test für die zweite Nachricht schlug vor der Korrektur fehl und besteht
danach. Zusammen bestehen 194 Workspace-Tests und Clippy mit `-D warnings`;
zwei manuelle Renderingtests sind weiterhin ausgelassen.

### Verbleibende Unterschiede und Prüfumfang

Dieser Vergleich bestätigt keine vollständige Gleichwertigkeit mit libchipcard.
BankingCraft beschränkt sich auf den bekannten ZKA-Pfad: keine alternativen
KEYD-Datenlayouts D1/D2/D8, Karteninitialisierung oder Zertifikatsverfahren;
keine vollständige Abdeckung herstellerspezifischer Leser-Vorlagen. EF_GD0/EF_SSD
werden nicht wie im allgemeinen libchipcard-Reopen-Pfad ausgewertet. PWDD wird
derzeit nur über die ersten acht Records gelesen (libchipcard: bis 31); die
angeschlossene Karte liefert ihre eindeutige Beschreibung innerhalb dieses
Bereichs. Der allgemeine KEYD-/Notepad-Record-Leser beendet auch bei anderen
Lesefehlern als 6A83 die Aufzählung; eine unvollständige Liste kann daher als
fehlender Schlüssel/Zugang erscheinen. Der gewünschte Kontext und die
benötigten Schlüssel müssen dennoch vorhanden sein; es gibt keinen Fallback
auf einen anderen Bankzugang.

Der reale RDH-7-Kartendialog ist der konkrete Prüfgegenstand. RDH-9/10 und andere
Karten-/Leserkombinationen sind dadurch nicht live bestätigt.

Der Release-Build mit der vom Nutzer autorisierten temporären Test-Produkt-ID
wurde installiert. Ad-hoc-Bundlesignatur und übereinstimmende Build-UUID
`E7E7D670-56DD-3384-98D8-58F1F69BB39D` wurden geprüft; der vorherige App-Stand
liegt als lokale Sicherung vor. Auch nach der abschließenden Dialogende-Korrektur
bestehen alle 194 Tests und Clippy. Die App wurde bis zum Entsperrbildschirm
gestartet. Der Nutzer bestätigte anschließend den erfolgreichen persönlichen
VR-Bank-Abruf mit der TCP-Korrektur.

## Lokale Test-Produkt-ID vom Repository getrennt

Auf Nutzerwunsch wurde der konkrete Wert aus diesem Bericht entfernt. Er war
in keinem bisherigen Commit enthalten. Die bisherige Compile-time-Variable
`BANKINGCRAFT_DEFAULT_PRODUCT_ID` wird nicht mehr ausgewertet.

Nur das ausdrücklich aktivierte App-Feature `local-fints-test` liest einen
Fallback aus `BankingCraft/local-tests/fints-product-id` im Konfigurationsordner
des lokalen Benutzers. Unter macOS liegt die Datei außerhalb des Projekts in
`~/Library/Application Support`; Ordnerrechte sind 0700, Dateirechte 0600.
Der Inhalt wird weder in das Binary eingebaut noch in den Tresor gespeichert.
Ein vorhandener eigener Wert im Tresor bleibt vorrangig. Normale Builds lesen
die Datei nicht; eine weitergegebene Test-App enthält die Test-ID nicht.

In diesem lokalen Checkout sind `pre-commit`, `commit-msg` und `pre-push`
eingerichtet. Eine separate private Sperrliste außerhalb des Projekts erlaubt
die Prüfung, ohne den Wert in Prüfscripts oder versionierten Dateien abzulegen.
Die Hooks prüfen gestagte Dateiinhalte/Namen, Commit-Nachrichten sowie sämtliche
erreichbaren Objekte der zu pushenden Historie einschließlich früher gelöschter
Vorkommen und Binärdaten. Sie und die Sperrliste sind bewusst maschinenlokal;
die Schutzprüfung ist kein serverseitiger GitHub-Hook. Es wurde nichts gepusht.

Die Hook-Prüfungen wurden in einem Wegwerf-Repository mit synthetischen Werten
gegen saubere Inhalte, gestagte Treffer, Commit-Texte, später entfernte Treffer
und UTF-16-Binärdaten über Chunk-Grenzen getestet. Die bestehende Historie und
der bestehende Index bestehen die lokale Prüfung.

Verifikation: Je 197 Workspace-Tests und Clippy mit `-D warnings` bestehen mit
und ohne `local-fints-test`; zwei manuelle Renderingtests bleiben ausgelassen.
Ein kurzzeitiger lokaler Prüflauf ohne Bankkontakt bestätigte das tatsächliche
Laden der privaten Datei nur im Test-Build und den Vorrang eines konfigurierten
eigenen Werts. Der normale Build ignorierte auch die frühere Build-Variable.
Die Prüfhilfe wurde danach entfernt. Die Test-ID wurde weder in den aktuellen
versionierten/nicht ignorierten Dateien noch im neuen Release-Binary gefunden.
Die lokale Test-App wurde installiert, ihre Ad-hoc-Signatur und Build-UUID
`E380DB8B-C372-35B6-961E-FA6A0E0F256C` geprüft und der Start bis zum
Entsperrbildschirm bestätigt. Ein erneuter Bankabruf war dafür nicht nötig.
