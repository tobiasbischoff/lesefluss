# M7 — Releaseprüfung

**Stand:** 24.09.2026 · Lesefluss 0.1.0 · Commit-Basis: `e30b7dc` (M6) + dieser Stand

## Inhalt

> **Stand nach dem Review vom 24.09.2026:** Die ursprüngliche Einschätzung dieses Dokuments
> („Kern und Sicherheitsrahmen erreicht“) war zu weitgehend. Das technische Review
> (`docs/review-fertigstellungsplan.md`) fand P0-Fehler in Datenintegrität, Restore und
> Netzwerkgrenzen; diese sind inzwischen behoben und durch Regressionstests abgesichert.
> Verbindlich sind heute `docs/review-fertigstellungsplan.md` und `docs/abnahme-protokoll.md`.
> Die untenstehende DoD-Tabelle ist eine Momentaufnahme vor diesem Review.

- **Performancebericht** (`docs/perf-report.md`): Startzeit, DB-Latenzen bei 100 000 Artikeln,
  Idle-CPU, Speicher, Frame-Pacing — mit Methode, Werkzeug und Rohwerten.
- **Datenschutzhinweise** (`docs/privacy.md`): Datenorte, Netzwerkzugriffe, Feedly-Datenfluss,
  Portal-/Schlüsselbundbedarf, Löschwege.
- **Lizenz-/Iconnachweise** (`docs/credits.md`): eigene Lizenz, eigenes Icon, keine gebündelten
  Assets, Fremdabhängigkeiten mit Lizenz, API-Vertrag.
- **Bekannte Grenzen** (`docs/known-limitations.md`): Funktional, Datenmodell, Sicherheitsrahmen,
  Distribution.

## Beim Releasecheck gefundene und behobene Fehler

| Fund | Wirkung | Behebung |
|---|---|---|
| `saved_ids_for_account` band den SQL-Parameter `account_id` nicht (`query_map([])`) | Saved-Reconciliation mit Feedly scheiterte immer (`Invalid parameter number`) und brach den Sync ab; der Marker-Pfad blieb unberührt | `query_map(params![account_id], …)`, neuer Test `saved_ids_scoped_to_account` (Storage: 6 → 7 Tests) |
| Feedly-Scheduler ignorierte `refresh_min` (fest 15 min) | Einstellung „Aktualisierung" wirkte nur kosmetisch | Intervall comes from `prefs.refresh_min` (5–1440 min), Kontokasten-Text folgt der Einstellung |
| Aufbewahrung war fest auf 90 Tage verdrahtet | Einstellung „Aufbewahrung" wirkte nur kosmetisch, erst nach Neustart | `run_retention()` nutzt `prefs.retention_days` und wird bei Änderung sofort ausgeführt |
| Clippy `never_loop` im ETag-Test | Lint-Fehler blockierte `--all-targets` | Testserver auf einen Request pro Verbindung umgestellt |

## Messwerkzeuge (bleiben im Repo)

- `crates/app/src/bin/lesefluss-bench.rs` — synthetische Datenbank (100 000 Artikel) und Latenzmessung
  für Liste, Keyset, Suche, Zähler, Statuswechsel; schreibt nur in die über `--db` angegebene Datei.
- `LF_DEBUG=1` — Startzeit-Log (`startup-ready <ms>`) und Statusmeldungen.
- `LF_FRAMECHECK=1` — 3600 Frame-Ticks am `gtk::FrameClock` mit p50/p95/p99/max und
  Budgetüberschreitungen; `LF_FRAMECHECK=stress` startet zusätzlich `win.next-article` alle 350 ms
  (Datenbank + Reader-Render pro Schritt).

## Verifikation

- `cargo test --workspace`: 15 Test-Suiten, 0 Fehler.
- `cargo clippy --workspace --all-targets`: keine Fehler; verbleibende Warnungen sind
  vorbestehend (Typkomplexität in Callback-Signaturen, ungenutzte Hilfsmethoden). `rustfmt` ist im
  Projekt nicht erzwungen (keine CI, keine `rustfmt.toml`); neue Dateien sind formatiert, eine
  repo-weite Formatierung würde jeden Bestandspfad anfassen.
- Feedly live: Delta-Sync nach 60 s aktualisiert `account_state.last_sync` (Erfolg, keine
  Fehlermeldung); Saved-Inventar `streams/ids?streamId=user/<id>/tag/global.saved` liefert 200
  (Kontenbestand leer) — die Reconciliation läuft damit auf einem geprüften Pfad.
- Secret Service: Token ausschließlich im Schlüsselbund, kein Treffer in der Datenbank, kein
  Treffer in der Git-Historie (`.gitignore` zusätzlich auf `feedly-token`).

## Definition of Done (§19) — Stand

| Kriterium | Status |
|---|---|
| Drei Spalten, Ruhe/Dichte, keine macOS-Dekorationen | erledigt (M1, Screenshot-Abnahme beim Nutzer) |
| Schmale Hyprland-Kachel ohne horizontales App-Scrolling | erledigt (M1/M6, Collapse 1119/779 px) |
| Native Wayland-Nutzung ohne XWayland | erledigt (M0-Probe, `GDK_BACKEND=wayland`) |
| Lokaler Modus vollständig ohne Feedly/Backenddienst | erledigt |
| OPML Import/Export mit Vorprüfung, Merge, Roundtrip | erledigt (3 Tests) |
| Feedly in beide Richtungen mit dem persönlichen Konto | erledigt (Marker live geprüft, Read/Saved) |
| Offline-Änderungen überstehen Neustart/401/erneute Anmeldung | erledigt (Outbox-Tests, Crash-Recovery) |
| Zähler, lokale Bestände, Remote-Vollständigkeit nicht verwechselt | erledigt |
| Reconciliation ohne Überschreiben bei unvollständigen Seiten | erledigt (Overlappungsfenster, `outbox_has_pending`) |
| Alle wesentlichen Aktionen per Tastatur, keine Fokusfalle im WebView | erledigt (M1, Accel-Gating M6) |
| Listen- und Reader-Scrollen erfüllen Messziele | teilweise (Frame-Pacing belegt, Endgerät-Scrollen offen) |
| Sync/Bilder/Themewechsel ohne Positionssprünge | erledigt (M2/M3/M6) |
| Dunkel/Hell, High-Contrast, reduzierte Bewegung, Textskalierung | teilweise (Theme geprüft, Textskalierung/Skalierung offen) |
| Artikel-HTML ohne Skripte/lokale Ressourcen | erledigt (ammonia-Tests, CSP `default-src 'none'`) |
| Secrets weder in DB, OPML, Backup, Logs noch Repo | erledigt (geprüft) |
| Migration, Backup/Restore, voller Datenträger, WebKit-Absturz | teilweise (getestet; Absturz-Simulation offen) |
| Arch-Paket installiert Desktop-Datei, Icon, Metadaten, Abhängigkeiten | teilweise (Dateien vollständig, `makepkg` nicht durchlaufen) |
| Bekannte Einschränkungen und API-Rechte dokumentiert | erledigt (`known-limitations.md`, `feedly-api-vertrag.md`) |

## Offen für die Veröffentlichung

1. `makepkg -si` einmal vollständig durchlaufen (Quellpfad des PKGBUILDs auf ein echtes Repository
   umstellen).
2. Name festgelegt: Lesefluss; Desktop-Datei, AppStream, Icon und `application_id`
   verwenden `io.github.tobiasbischoff.Lesefluss`. Projektadresse:
   `https://github.com/tobiasbischoff/lesefluss`.
3. `LICENSE-MIT` und `LICENSE-APACHE` ins Wurzelverzeichnis.
4. `cargo audit`/`cargo deny` auf einer Maschine mit Netzzugang.
5. Manuelle Abnahmen: Orca/AT-SPI, Skalierung 125 %/150 %, Zweitmonitor-Mix, `sysprof`-Scrollmessung,
   WebKit-Absturzverhalten.
6. Optional Flatpak (Portal-/Schlüsselbund-/Theme-Zugriffe separat prüfen).
