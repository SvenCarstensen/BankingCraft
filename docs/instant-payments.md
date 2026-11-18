# Echtzeit- und Terminüberweisungen über AqBanking

Verfügbar ab BankingCraft 0.2.1. Offline geprüft am 18. November 2026.

BankingCraft erweitert das fest eingebundene AqBanking 6.9.2 reproduzierbar mit
`scripts/patch-instant.py`. Die Änderungen liegen in `native/aqbanking/instant/`
und fließen in den Hash des nativen Build-Rezepts ein. Eine Systeminstallation
von AqBanking reicht für diese Auftragsarten nicht aus.

## Echtzeitüberweisung

Der Schalter verwendet einen eigenen Backend-Befehl (`SepaInstantTransfer`),
der ausschließlich HKIPZ 1 oder 2 erzeugt. Der bisherige Subtyp `EuASAP` auf
einem normalen SEPA-Befehl war unzureichend: Er erzeugte weiterhin HKCCS/HKCCM.
Die Version stammt aus den BPD. Kontoberechtigungen (UPD), PIN/TAN-Parameter,
HKIPS, dessen Mindestwartezeit und die erforderliche Empfängerprüfung müssen
vorhanden sein. Fehlende Fähigkeiten blockieren den Auftrag, ohne Rückfall auf
Standard-SEPA.

Die Exportprofile pain.001.001.09, pain.001.001.03 und pain.001.003.03 werden mit
der bankseitigen Formatliste abgeglichen. Die Liste in HIIPZS/HIIPSS hat Vorrang
vor HISPAS. Das Zahlungs-XML enthält `PmtTpInf/LclInstrm/Cd=INST` und
`ReqdExctnDt=1999-01-01`. HKIPZ 2 enthält die Umwandlungsentscheidung `N` nur,
wenn die BPD dieses Feld erlauben. Standard- und Terminüberweisungen erhalten
kein `INST`. Die für VoP vorbereiteten Zahlungsbytes bleiben bei der Freigabe
unverändert. BestSign einschließlich seiner getrennten Freigabeabfragen bleibt
im vorhandenen AqBanking-Dialogablauf.

HIIPZ/HIIPS werden anhand der Auftragsreferenz ausgewertet; HIIPS zusätzlich
anhand der Auftraggeber-IBAN. Status 7 oder ein auftragsbezogenes 0020 ohne
abweichenden Status bestätigen die Ausführung. Annahme, laufende Verarbeitung,
Ablehnung, fehlende Antworten und widersprüchliche Antworten werden getrennt
behandelt. Ein Fehler beim Statusabruf belegt keine Ablehnung der Zahlung.
3046 oder eine unerwartete Umwandlung führen zu einem unklaren Ergebnis.

Die Bankreferenz, das Ergebnis und der früheste nächste Abfragezeitpunkt werden
im verschlüsselten Zahlungsauftrag gespeichert, bevor eine weitere Abfrage
startet. Ein Speicherfehler stoppt die Abfragen. Bis zu drei automatische HKIPS-
Abfragen folgen einem noch laufenden Auftrag und beachten die Bankwartezeit.
Danach bleibt der Auftrag prüfbar. „Status prüfen“ setzt nach einem Neustart mit
HKIPS fort; dieser Weg kann keinen neuen Zahlungsauftrag erzeugen. Wie bei allen
Zahlungen verhindert der vor dem Versand gespeicherte Submitting-Marker eine
unbeabsichtigte Wiederholung nach Absturz oder Verbindungsabbruch.

## Terminüberweisung

`SepaCreateDatedTransfer` war im FinTS-Provider bisher nicht implementiert.
BankingCraft ergänzt HKCSE/HICSE/HICSES 1. Das Datum muss in der Zukunft und
innerhalb der von der Bank angegebenen minimalen und maximalen Vorlaufzeit
(Kalendertage) liegen. Es wird unverändert in die pain.001-Datei übernommen.
HKCSE verwendet Challenge-Klasse 23 und den vorhandenen VoP/TAN-Ablauf.

Auftragsbezogene 0010/0020 bestätigen die Annahme zur späteren Ausführung;
eine bloße positive Nachrichtenrückmeldung reicht nicht. Fehlermeldungen
verhindern die Erfolgsmeldung. Hinweise der Bank, etwa 3030 zur Verschiebung
auf einen Bankgeschäftstag, werden in der Annahmebestätigung aufbewahrt.
Die Einreichung ist keine Bestätigung der späteren Kontobelastung. Änderungen
oder Löschungen eines bereits eingereichten Terminauftrags sind nicht Teil
dieses Überweisungsformulars.

Die Oberfläche bietet Echtzeit nur mit HKIPZ und HKIPS sowie Termin nur mit
HKCSE an. Bestehende Zugänge müssen ihre Kontofähigkeiten einmal synchronisieren.
Vorlagen wechseln bei fehlender Berechtigung nicht unbemerkt die Zahlungsart.

## Prüfung ohne Bankverbindung

- `python3 scripts/test-native-payments.py`: echte Provider-Factory,
  FinTS-Parser und -Serializer, Zahlungs-XML für alle drei Formate,
  BPD/UPD/VoP-Abhängigkeiten, Versionswahl, Konvertierungsverbot,
  Datumsgrenzen und Bankrückmeldungen. Das Testprogramm bricht Netzwerkverbindungen
  über `connect` sofort ab und läuft mit ASan/UBSan. Unter Linux bleiben lokale
  Unix-Sockets für die PC/SC-Initialisierung erlaubt.
- `python3 scripts/test-native-adapter.py`: vorhandene VoP-/BestSign-Dialoge,
  Freigabeintervalle, Freigabeablehnung und Abbruch mit synthetischem Transport.
- `cargo test --workspace`: zusätzlich IPC-Folgen, maximale Statusabfragen,
  Wartezeiten, Neustart, Fehler, verschlüsselte Speicherung, Schutz gegen
  erneutes Senden und Kontofähigkeiten im Formular.
- `cargo clippy --workspace --all-targets -- -D warnings` und `cargo build`.

Es werden weder echte Bankzugänge noch PIN/TAN oder echte Zahlungen verwendet.
Eine Annahme durch das produktive Postbank-System lässt sich damit nicht belegen.

## Protokollgrundlagen

Die [offizielle FinTS-Spezifikation](https://www.fints.org/de/spezifikation)
und deren [Änderungen](https://www.fints.org/de/spezifikation/aenderungen):
CR0482 (HKIPZ/HKIPS), CR0514 (HKIPZ 2), Messages 3.0 Release 2022,
C.10.2.2.1 und Data Dictionary (HKCSE und Vorlaufzeiten), sowie VoP 1.02
vom 19.10.2026. Die [Postbank-FinTS-Informationen](https://www.postbank.de/privatkunden/services/fints.html)
beschreiben das Bankangebot; maßgeblich für ein konkretes Konto sind dessen
aktuelle BPD und UPD.
