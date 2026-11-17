# Lokale BankingCraft-Builds

Vorgabe des Nutzers vom 17.11.2026:
- Für lokale Entwicklung und das Ersetzen der installierten App standardmäßig einfache Debug-Builds verwenden (`cargo build`, ohne `--release`).
- Keine Developer-ID-Signierung, keine Apple-Notarisierung und keine Uploads an Apple für lokale Builds. Nur technisch notwendige lokale Ad-hoc-Signaturen zum Laden von Mach-O-Dateien sind zulässig.
- Release-Builds oder Apple-Distributionssignierung nur erstellen, wenn der Nutzer sie ausdrücklich anfordert.
