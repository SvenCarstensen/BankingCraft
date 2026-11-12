# Sicherheitsprüfung vom 12. November 2026

Geprüft wurden Tresor und PIN-Speicherung, Export/Diagnose, Transport, FinTS-Parser, TAN-Freigabe, Zahlungsaufträge, RDH/DDV sowie die aufgelösten Rust-Abhängigkeiten. Die Änderungen werden durch synthetische Regressionstests abgesichert; die Tests verwenden keine echten Zugangsdaten und lösen keine Zahlungen aus.

## Behobene Befunde

| Befund | Änderung und Prüfung |
|---|---|
| Veraltete RSA-Abhängigkeit mit bekanntem Timing-Angriff | `rsa` von 0.9.10 auf 0.10.0 aktualisiert. RDH-Signaturprüfung, kompletter simulierter RDH-7-Dialog und unabhängiger RDH-10-RSA-Vektor prüfen die Migration. Die produktiven privaten Benutzerschlüssel bleiben auf der Chipkarte. |
| Nicht abgeschlossene TAN-Freigabe konnte als Erfolg behandelt werden | Offene Freigabecodes, fehlende Auftragsreferenz und fehlende abschließende Erfolgsmeldung führen zum Fehler. Zahlung wird nicht automatisch wiederholt. |
| Zahlungsdokument konnte ein anderes Auftraggeberkonto enthalten | IBAN des ausgewählten Kontos muss bei Überweisung, Terminüberweisung und Dauerauftrag mit dem Zahlungsdokument übereinstimmen. |
| CSV-Formeln aus Banktexten | Potenzielle Formelpräfixe einschließlich führender Leerzeichen/BOM werden in Textfeldern entschärft; reguläre Zahlenbeträge bleiben numerisch. |
| Zu offene Dateirechte und vorhersehbare Exportziele | CSV-/CAMT-Dateien werden exklusiv mit zufälligem Namensanteil und unter Unix mit 0600 angelegt. Diagnoseberichte werden atomar privat ersetzt. Vorhandene Dateien/Symlinks werden nicht als Schreibziel geöffnet. |
| Banktexte in dauerhaften Diagnoseausgaben | Fehlerbericht enthält nur die technische Segmentübersicht. Laufzeitprotokoll beschränkt Bankrückmeldungen auf geprüfte numerische Codes. Fachliche Fehlermeldungen erscheinen weiterhin in der Oberfläche. |
| Unnötig lange Lebensdauer von Geheimnissen | PIN-Zwischenspeicher, Nachrichtenpuffer und Transportkodierung verwenden `Zeroizing`; abgelehnte PIN wird auch aus dem Arbeitsspeicher-Cache entfernt. Passwortfelder werden nach der Eingabe geleert. |
| Verwirrende HTTPS-Zieladressen | HTTPS-Adressen mit Zugangsdaten, Fragmenten, Rückwärtsschrägstrichen oder Steuerzeichen werden vor der PIN-Übermittlung abgelehnt. Zertifikatsprüfung und deaktivierte Weiterleitungen bleiben aktiv. |
| Übermäßige Speicherbelegung durch Eingabedaten | Grenzen für Nachrichtengröße, Segment-/Feldanzahl, XML-Knoten, TAN-Bildabmessungen/-speicher, Bankschlüssel und Tresordatei. XML-DTDs bleiben verboten. |
| Unzureichende Prüfung verschlüsselter Kartenantworten | DDV verlangt einen passenden verschlüsselten Umschlag ohne zusätzlich eingeschleuste Klartextsegmente. RDH/DDV prüfen Blocklängen und Padding; Bankschlüssel werden vor Übernahme validiert. |

Die RSA-Aktualisierung folgt [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html). Die CSV-Behandlung orientiert sich an [OWASP CSV Injection](https://community.owasp.org/attacks/CSV_Injection). CSV bietet keine universelle Sicherheitsgarantie für alle Tabellenprogramme und spätere Speicher-/Importvorgänge; die implementierte Behandlung adressiert die ursprüngliche Interpretation exportierter Textfelder als Formel.

## Verifikation und verbleibende Grenzen

- `cargo test --workspace --locked`: 132 Tests erfolgreich, einschließlich 22 zusätzlicher Sicherheits-/Fenstertests gegenüber dem vorherigen Stand.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- `cargo audit` 0.23.0: nach dem RSA-Update keine bekannte Sicherheitslücke. Sechs separate Hinweise auf nicht mehr gepflegte indirekte Pakete bleiben sichtbar: `bincode`, `pin-utils`, `ttf-parser`, `zbus_names`, `zvariant`, `zvariant_derive`. Es wurden keine Warnungen unterdrückt. `pin-utils` wird auf macOS über die Slint-Bauwerkzeuge verwendet; die übrigen sind für andere/optionale Plattformpfade im Lockfile enthalten. Eine Umstellung dieser indirekten APIs erfordert entsprechende Änderungen bei Slint/Winit/AccessKit; sie sind kein durch eine einfache Versionsanhebung lösbarer direkter App-Befund.
- Native macOS-Prüfung erfolgreich: aktualisierte Anwendung gestartet, vorhandenen Tresor entsperrt, Überweisungsformular über volle Breite geprüft. Fensterzustand über macOS-Zoom verändert, mit Cmd-Q beendet und über den Schließen-Button erneut beendet; Position und Größe nach Neustart exakt gleich. Ursprüngliche Fenstermaße anschließend wiederhergestellt.
- Kein Test mit realem Kartenleser, da auf Benutzerwunsch ausgesetzt. Die DDV-Verbesserung ist eine Umschlag-/Paddingprüfung, keine Aussage über vollständige kryptografische Authentifizierung aller DDV-Bankantworten. RDH-Antwortsignaturen werden separat geprüft.
- `Zeroizing` begrenzt die Lebensdauer der kontrollierten Puffer; temporäre Kopien in GUI-, Betriebssystem- oder Drittbibliotheken lassen sich damit nicht vollständig ausschließen.

Diese Quellcodeprüfung und die Tests ersetzen keine unabhängige vollständige Sicherheitszertifizierung.
