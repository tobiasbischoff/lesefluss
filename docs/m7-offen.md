# Restarbeiten nach dem technischen Review

**Stand:** 24.09.2026 · Basis: `docs/review-fertigstellungsplan.md` (Maßstab) und
`docs/abnahme-protokoll.md` (Beleglage).

Die Liste `m7-offen.md` vom Vortag ist durch dieses Dokument ersetzt: Die dort
offenen Punkte zu Feedverwaltung, Sortierung, Lokalisierung, Bildvorschauen, Vorrats-
begrenzung, Aufbewahrung, Restore-Sicherheit, Kontozuständen und Netzwerkgrenzen sind inzwischen
umgesetzt und durch Regressionstests abgesichert (siehe Abnahmeprotokoll).

## Erledigt in der Review-Umsetzung

| Paket | Umsetzung | Commit |
|---|---|---|
| A1 Restore/Migration | Kandidatenprüfung, Sicherung, Staging + `fsync` + `rename`, Fehlermeldung | `f5a478d` |
| B1 Netzwerk-/Mediengrenzen | streamende Limits, Netzwerkpolicy, SVG/40-MP-Prüfung, 4er-Semaphore | `127cb77` |
| A2 Identität | `(Konto, Artikel-ID)`, kontobezogene Lookups, Counts-Regel | `55bd198` |
| A3 Atomarität | Status+Outbox in einer Transaktion, dauerhafte Revisionen, ACK-Bestätigung | `55bd198`, `ca33a74` |
| A4 Retention/FTS | Pending geschützt, Metadaten erhalten, FTS-Trigger, ehrliches `has_content` | `55bd198` |
| C1 Routing | `due_feeds` nur lokal, Refresh nach Kontotyp, ein Zyklus, Kontostate nach Login | `55bd198`, `60636ef` |
| C2 Pagination | Zustandsmaschine, leere Seiten mit Cursor, Zyklen/Limit = unvollständig | `bc5d64a` |
| C3 Konflikte | Generationsprüfung im selben Job | `55bd198` |
| C4 Zustände | Kontozustände persistent, Fehlerklassifikation, Backoff mit Jitter | `1cb2e2b` |
| B2 Secret/URI | 0600 beim Anlegen, Kontobindung, `http(s)`-Allowlist | `27d1b15` |
| D1 Lesen | Tastenrouter, Auto-Read-Bindung, Zählerlogik | `55bd198` |
| D2 Liste | Cursor aus `sort_ms`, Suche mit Scope/Filter/Pagination, Fenster 2 000, „N neue Artikel" | `03d1ed2`, `fe2cd17` |
| D3 OPML/Discovery | Stack-Disziplin, Transaktion, HTML-Parser statt String-Scanner | `1edffd1` |
| D4 Reader | sofortiges Rendern, Medienbrücke, Generationsprüfung | `ed179d6` |
| D5 Hauptthread | kein `recv()`/`secret-tool` im UI-Thread, BG-Kanal | `e4e486e` |
| E1 Feedverwaltung | Kontextmenü, Umbenennen, Gruppen, Abbestellen, Nutzertitel | `2630382` |
| E2 Sortierung | beide Richtungen, Einstellung, `Strg+Shift+P` | `6386832` |
| E3 Sprache/AT | DE/EN-Umschaltung, zugängliche Namen | `1fda591` |
| E4 Medien/Ansicht | echte Vorschauen, Nur-Lesen (F9), Fenstergröße | `ce7ef15` |
| F/G | Module, CI, README, Lizenzen, Abnahmeprotokoll, lauffähiges Arch-Paket | `e76802c`, `0473be9`, `b915a1f` |

## Offen — nur Nutzer oder Testumgebung

Diese Punkte kann kein Automatisierungslauf ersetzen; sie stehen im Abnahmeprotokoll
mit demselben Status.

- [ ] Physische Tastaturprüfung: alle Aktionen aus Spec §6, Buchstabenkürzel in Liste und
      Reader, keine Eingabefelder-Abfangung, `F6` aus dem WebView, `Esc`-Kette.
- [ ] Touchpad- und Mausgefühl, Fenster im gekachelten, schwebenden, maximierten und
      sehr schmalen Zustand; `sysprof`-Messung über 60 s beim Scrollen mit Parallel-Import.
- [ ] Orca/AT-SPI-Durchgang, Textskalierung 200 %, High Contrast, reduzierte Bewegung,
      Skalierungen 125/150/200 %, Monitormix.
- [ ] WebKit-Absturz und voller Datenträger im isolierten Testprofil.
- [ ] `sudo pacman -U packaging/lesefluss-git-*.pkg.tar.zst` auf einem frischen System.
- [ ] Feedly-Zwei-Client-Test mit autorisiertem Testkonto: Read/Unread und Saved/Unsaved je
      Richtung, offline mit Umweg, Tokenablauf, alter Saved-Artikel, entferntes Abo.
- [ ] `cargo audit` auf einer Maschine mit Netzzugang; Befund mit Datum und Versionen
      in `docs/credits.md` ergänzen.

## Offen — bewusste Entscheidung

- [ ] Öffentlicher Feedly-Login (Authorization Code + PKCE im Systembrowser) ist weiterhin
      nicht freigegeben; manueller Token bleibt benannte Abweichung.
- [ ] Abo-/Gruppen-Schreiben bei Feedly erst nach Live-Verifikation des Endpunkts.
- [ ] Spaltenbreiten persistieren: in libadwaita 0.9 nicht lesbar; Fenstergröße wird gespeichert.
- [ ] Volltext aus Originalseiten, Flatpak, Favicons, Hintergrunddienst: SPÄTER laut Spec.

## Optional für die nächste Runde

- [ ] `window.rs` weiter zerlegen (Lesersteuerung, Kontoverwaltung, Ereignisrouter).
- [ ] Typisierte Nachrichten statt `Any`/Downcasts zwischen DB-Worker und Oberfläche.
- [ ] `cargo fmt` repo-weit einführen (der Bestand ist bewusst uneinheitlich).
- [ ] Testabdeckung nach der Testmatrix §17.1 vervollständigen (Fault-Injection an
      Commit-Grenzen, Property-/Fuzz-Tests für OPML und URL-Verarbeitung).
