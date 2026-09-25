# M6 — Status und Abnahme

**Stand:** 24.09.2026.

## Umgesetzt

- **Einstellungen** (Strg+,): AdwPreferencesWindow mit fünf Seiten
  (Lesen, Darstellung, Aktualisierung, Speicher & Datenschutz, Konten).
  Persistenz in `prefs`-Tabelle (Migration 5), Live-Anwendung:
  Auto-gelesen, Kompaktheit, Bildvorschauen, Reader-Typografie (Größe/Breite/
  Zeilenhöhe), Theme-Modus, Buchstabenkürzel an/aus (Accel-Registrierung),
  Abrufintervall, Aufbewahrungstage, Bildcache-Cap (AtomicU64 im MediaCache).
- **Secret Service statt Token-Datei:** `secret-tool` (libsecret) als Backend;
  bestehende Token-Datei wird beim ersten Zugriff migriert und gelöscht
  (live verifiziert: `secret-tool lookup lesefluss feedly-token` liefert Token,
  `~/.config/lesefluss/` ist leer). Fallback Datei nur, wenn kein Keyring
  erreichbar (chmod 600).
- **Omarchy-Theme-Adapter:** liest `~/.local/state/omarchy/current/theme/colors.toml`
  (Verzeichnis-Form) bzw. `~/.config/omarchy/current/theme` + `/usr/share/omarchy/themes/<name>/colors.toml`;
  mappt mode/background/foreground/accent/selection/muted/lighter/dark_background
  auf die semantischen Tokens; Modus „omarchy" in den Einstellungen;
  gio-Directory-Monitor auf dem Theme-Parent mit 250 ms Debounce;
  es werden nur app-eigene CSS-Klassen überschrieben.
- **Distribution:** `packaging/PKGBUILD` (cargo --release --locked), Desktop-Entry,
  skalierbares Icon, AppStream-Metainfo unter `data/`; Fenster-Iconname gesetzt.
  App-ID seit der Namensfestlegung: `io.github.tobiasbischoff.Lesefluss`.

## Abnahme offen (manuell, frische Omarchy-Umgebung)

- `makepkg -si` im packaging-Kontext (Quellpfad des PKGBUILDs auf Repo-Layout
  anpassen bzw. via `source=("git+...")` des öffentlichen Repos nach Release).
- Orca/AT-SPI-Durchgang, Fractional Scaling 125/150 %, Zweitmonitor-Mix.
