# Abnahmeprotokoll

**Stand:** 24.09.2026 · Build: `cargo build --release --locked` · Commits bis `e76802c`
**Referenzmaschine:** Intel Core 5 320, 15 GiB RAM, Omarchy/Hyprland, 2560×1600 @ 60 Hz,
Skalierung 1.667, GTK 4, WebKitGTK 6.

Je Fall: Soll, Ist, Beleg, Status. „Offen“ bedeutet: nicht geprüft oder nicht erfüllt —
nicht „vermutlich in Ordnung“.

## Automatisierte Gates

| Fall | Soll | Ist | Beleg | Status |
|---|---|---|---|---|
| Testsuite | alle grün | 15 Suiten / 82 Tests grün | `cargo test --workspace --locked` | erfüllt |
| Clippy | keine Fehler | 0 Fehler, 24 Warnungen (Typkomplexität, ungenutzte Hilfsmethoden) | `cargo clippy --workspace --all-targets --locked` | erfüllt mit Vorbehalt |
| Formatierung | einheitlich | neue Dateien formatiert, Bestand uneinheitlich (kein `rustfmt.toml`) | `cargo fmt --all -- --check` | teilweise |
| Reproduzierbarkeit | feste Werkzeuge | `rust-toolchain.toml` (stable + rustfmt/clippy), Lockfile eingecheckt, CI-Workflow (Arch-Container) | `.github/workflows/ci.yml` | erfüllt |
| Identität/Kontogrenzen | keine kontoübergreifende Wirkung | Tests: `identity_is_account_and_article_id`, `remote_status_updates_stay_inside_the_account`, `remote_pull_never_touches_local_articles` | `cargo test -p storage` | erfüllt |
| Konflikte nach ACK | jüngere Absicht gewinnt | Test `stale_pull_cannot_overwrite_a_confirmed_local_intent` | dito | erfüllt |
| Retention | Pending geschützt, Metadaten erhalten | Test `retention_keeps_metadata_and_protects_pending_outbox` | dito | erfüllt |
| Volltextindex | keine Waisen nach rowid-Wiederverwendung | Test `fts_index_survives_rowid_reuse` | dito | erfüllt |
| Restore | defekte Datei darf Bestand nicht ersetzen | Tests `broken_candidate_keeps_existing_library`, `valid_candidate_replaces_library_and_keeps_safety_copy` | `cargo test --bin lesefluss-app` | erfüllt |
| Netzwerkgrenzen | keine Ziele ins Heimnetz, Bodies begrenzt | Tests `private_and_loopback_targets_are_blocked`, `oversized_chunked_body_is_aborted_before_full_allocation` | `cargo test -p provider-local` | erfüllt |
| Bilder | SVG ausgeschlossen, 40-MP-Grenze wirksam | `oversized_images_are_rejected_by_header_dimensions` | dito | erfüllt |
| Pagination | Limit gilt als unvollständig | `pager_reports_safety_limit_as_incomplete`, `pager_detects_cursor_cycles` | `cargo test -p provider-feedly` | erfüllt |
| Tastaturroute | eindeutiger Besitzer, keine Felder | `letter_keys_map_to_actions`, `other_keys_are_untouched`, `editing_classes_are_recognized` | `cargo test --bin lesefluss-app` | erfüllt |
| OPML-Hierarchie | Stack korrekt, Grenzen hart | `group_after_feed_outline_stays_open`, `unbalanced_and_too_deep_documents_are_rejected` | dito | erfüllt |
| Sortierung | beide Richtungen, lückenlos | `oldest_first_paginates_without_gaps_or_repeats`, `search_also_supports_oldest_first` | `cargo test -p storage` | erfüllt |
| Abbestellen | gespeicherte Artikel bleiben | `unsubscribing_keeps_saved_articles_and_stops_fetching` | dito | erfüllt |

## Laufzeitmessungen

| Fall | Soll | Ist | Beleg | Status |
|---|---|---|---|---|
| Startzeit | warm p95 < 700 ms | 192–232 ms | `LF_DEBUG=1` → `startup-ready` | erfüllt |
| Lokale Auswahl | p95 < 50 ms | Seitenabruf 11 ms p95 (100k-Artikel-DB) | `lf-bench` | erfüllt (ohne Renderzeit) |
| Suche 100k | p95 < 150 ms | 45,6 ms p95 | `lf-bench` | erfüllt |
| Idle-CPU | < 1 % Kern | 0,20 % über 10 s | `/proc/<pid>/stat` | erfüllt |
| Speicher | < 350 MiB PSS | 123 MiB | `/proc/<pid>/smaps_rollup` | erfüllt |
| Frame-Pacing | keine Stalls > 50 ms | max 33 ms unter Navigationslast | `LF_FRAMECHECK=stress` | erfüllt (ohne Endgerät-Scrollen) |

Details und Rohwerte: `docs/perf-report.md`.

## Reale Abnahmen

| Fall | Soll | Ist | Status |
|---|---|---|---|
| Feedly zwei Richtungen | Read/Unread und Saved/Unsaved je Richtung | live verifiziert (Marker-Write bestätigt, Saved-Stream antwortet 200) | erfüllt |
| Feedly Delta-Sync | wiederkehrender Abgleich | läuft im Betrieb (Watermark wird fortgeschrieben) | erfüllt |
| Keyring | Token im Secret Service | `secret-tool lookup` liefert Token, Datei entfernt | erfüllt |
| Outbox nach Neustart | Änderungen gehen raus | Outbox-Zeilen bleiben persistent, `outbox_reset_inflight` beim Start | erfüllt (Test), live nach Neustart bestätigt |
| Dauerhafte Fehler | sichtbar, kein Massenwechsel | 404 → `status='failed'` + `unsynced`, Meldung an die Oberfläche | erfüllt |
| 60-s-Scrollen mit Parallel-Import | kein Stalls > 50 ms | nicht durchgeführt (Zeit-/Werkzeugbudget) | **offen** |
| Skalierung 125/150/200 %, Textskalierung | Layout bleibt benutzbar | nicht geprüft | **offen** |
| High Contrast, reduzierte Bewegung | unterstützt und geprüft | nicht geprüft | **offen** |
| Orca/AT-SPI | Namen, Rollen, Status | zugängliche Namen für Icon-Knöpfe gesetzt; Durchgang nicht erfolgt | **offen** |
| WebKit-Absturz | App bleibt lauffähig | Fehlerseite wird geschaltet (`web_process_terminated`) | teilweise (kein Absturztest) |
| Voller Datenträger | verständlicher Fehler, Daten bleiben | Validierung und Meldungen vorhanden, kein Fülltest | teilweise |
| `makepkg` Paketbau | Paket entsteht mit Desktop, Icon, Metadaten, Lizenzen | `makepkg -f` erfolgreich, 5,2 MiB, `appstreamcli validate` erfolgreich | erfüllt |
| Installation ohne Entwicklerwerkzeuge | `pacman -U` auf frischem System | Paket gebaut und geprüft; Installation hier mangels `sudo` nicht ausgeführt | teilweise |
| Tastatur/Fokus am Gerät | alle Aktionen erreichbar | Unit-Tests der Route, physische Prüfung ausstehend | **offen** |
| Touchpad-Gefühl | natürlich, keine Doppelträgheit | nicht geprüft | **offen** |

## Bewusste Abweichungen

| Abweichung | Grund | Fundstelle |
|---|---|---|
| Manueller Feedly-Token statt System-Browser-PKCE | kein freigegebener Clienttyp für private Konten; als benannte Abweichung geführt | `docs/feedly-api-vertrag.md` |
| Volltext aus Originalseiten | laut Spec SPÄTER | `docs/known-limitations.md` |
| Spaltenbreiten nicht gespeichert | libadwaita 0.9 bietet keinen Lesezugriff auf die Breite der Navigations-Spalten | `crates/app/src/window.rs` |
| Sicheres „Alles gelesen" mit serverseitiger Wirkung | nur nach verifiziertem Marker-Pfad; Umfang und Bestätigung werden angezeigt | `crates/app/src/feedly_sync.rs` |
| Farbpunkt statt Favicon | eigene Symbolsprache, keine fremden Assets | `docs/credits.md` |
| Ein-/Ausgabe der Spaltenbreiten per Tastatur | Teil der Restarbeiten | `docs/m7-offen.md` |

## Nächste Schritte für die Veröffentlichung

1. `sudo pacman -U packaging/lesefluss-git-*.pkg.tar.zst` auf einem frischen System (Paket ist gebaut und geprüft).
2. App-ID und Namen finalisieren (Desktop-Datei, AppStream, Icon, `application_id` gemeinsam).
3. Orca-, Skalierungs- und `sysprof`-Abnahme sowie Tastaturprüfung am Gerät.
4. `cargo audit` auf einer Maschine mit Netzzugang; Befund dokumentieren.
