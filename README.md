# Lesefluss

Ein nativer RSS-Reader für Omarchy/Arch unter Wayland: GTK 4, libadwaita, WebKitGTK 6,
SQLite. Zwei Betriebsarten, ein Fenster: lokale Bibliothek (eigene Feeds, OPML, Volltextsuche)
und Feedly (zwei Wege für Gelesen/Gemerkt, offline-fähig).

## Bauen

Voraussetzungen auf Arch:

```sh
sudo pacman -S --needed base-devel rust gtk4 libadwaita webkitgtk-6.0 sqlite
```

```sh
cargo build --release --locked      # Binary: target/release/lesefluss-app
cargo test --workspace --locked     # 15 Test-Suiten, 163 Tests
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

`rust-toolchain.toml` wählt **stable** (rustfmt + clippy) — ein bewegliches Ziel,
keine gepinnte Version. Für reproduzierbare Builds das Lockfile verwenden und die
Rust-Version beim Bauen protokollieren (`rustc --version`); CI (.github/workflows/ci.yml)
baut und prüft denselben Stand in einem Arch-Container.

Installation über das Arch-Paket in `packaging/` (Desktop-Eintrag, Icon, AppStream-Metadaten):

```sh
cd packaging && makepkg -si
```

## Bedienung

| Taste | Wirkung |
|---|---|
| `j` / `k` | nächsten/vorigen Artikel öffnen |
| `n` / `p` | nächsten/vorigen ungelesenen Artikel |
| `m` / `s` | gelesen umschalten / merken umschalten |
| `o` | Artikel im Standardbrowser öffnen |
| `Ctrl+L` / `Ctrl+F` | Artikelsuche / Suche im geöffneten Artikel |
| `Ctrl+Shift+P` | Reihenfolge umkehren (neueste/älteste zuerst) |
| `F9` | Nur-Lesen-Ansicht |
| `F6` / `Shift+F6` | Hauptbereiche wechseln |
| `Alt+←` / `Esc` | zurück / eine Ebene zurück |
| `Ctrl+,` | Einstellungen (auch über das Menü oben links) |
| `Ctrl+Z` / `Ctrl+Y` | rückgängig / wiederherstellen |
| `Ctrl+Shift+M` | Bereich als gelesen markieren |

Quellen lassen sich per Rechtsklick, Menütaste oder `Shift+F10` umbenennen, in Gruppen
einordnen, als gelesen markieren und abbestellen (gespeicherte Artikel bleiben erhalten).

## Datenorte

| Zweck | Ort |
|---|---|
| Bibliothek | `$XDG_DATA_HOME/lesefluss/library.db` |
| Bildcache | `$XDG_CACHE_HOME/lesefluss/media` |
| Einstellungen/Entwürfe | `$XDG_CONFIG_HOME/lesefluss/` |
| Feedly-Token | Secret Service (Eintrag `lesefluss` / `feedly-token`) |

Wiederherstellung: Menü → „Aus Backup wiederherstellen". Die Datei wird geprüft
(SQLite-Integrität, erwartete Tabellen, Schema-Version), der alte Bestand gesichert und
erst dann atomar aktiviert.

## Sprachen und Themes

Oberflächensprache Deutsch (Standard) oder Englisch (`LF_LANG=en` bzw. `LANG=en_*`).
Theme: System, Dunkel, Hell oder Omarchy (liest `colors.toml` des aktiven Omarchy-Themes
und folgt Änderungen automatisch).

## Sicherheit

* Artikel-HTML wird gegen eine Allowlist bereinigt; der WebView nutzt `default-src 'none'`.
* Bodies werden streamend in Größe begrenzt (Feed 10 MiB, Bild 12 MiB, JSON 4 MiB).
* Automatisch entdeckte Ziele dürfen keine Loopback-, Link-Local- oder privaten Netze sein.
* Keine Telemetrie, keine Analytik, kein Hintergrunddienst.

## Dokumentation

* `docs/review-fertigstellungsplan.md` — verbindlicher Fertigstellungsplan
* `docs/abnahme-protokoll.md` — Abnahmen mit Belegen und offenen Punkten
* `docs/feedly-api-vertrag.md` — geprüfter Feedly-Vertrag
* `docs/perf-report.md`, `docs/privacy.md`, `docs/known-limitations.md`
* `docs/m7-offen.md` — Restarbeitenliste

## Lizenz

Doppel-Lizenzierung `MIT OR Apache-2.0` (siehe `LICENSE-MIT`, `LICENSE-APACHE`).
Icon und Oberflächengestaltung sind eigenständig; keine kopierten Reeder-Assets.
