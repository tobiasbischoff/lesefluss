# Nachweise zu Lizenz, Icon und Fremdmitteln

**Stand:** 24.09.2026

## Lizenz

Das Repository ist noch ohne veröffentlichte Lizenzdatei; die geplante Doppel-Lizenzierung
`MIT OR Apache-2.0` ist in `data/io.github.tobiasbischoff.Lesefluss.appdata.xml` als
`project_license` eingetragen. Vor der ersten Veröffentlichung müssen `LICENSE-MIT` und
`LICENSE-APACHE` im Wurzelverzeichnis liegen; beide Texte sind Standard und ohne Änderungen zu
übernehmen.

## Icon

`data/io.github.tobiasbischoff.Lesefluss.svg` ist ein eigens für dieses Projekt gezeichnetes Vektor-
Icon (RSS-Signal über dunkler Verlaufsfläche), keine Kopie oder Bearbeitung fremder Icons. Es ist
als skalierbares Anwendungssymbol unter `hicolor/scalable/apps` installiert.

## Schriften und Bilder

Keine gebündelten Schriften, keine eingebetteten Bilder, keine kopierten UI-Assets. Typografie und
Symbole kommen vollständig aus dem System (GTK/libadwaita, Icon-Theme des Benutzers), Reader-
Inhalte verwenden die Systemschriften des WebView.

## Fremdabhängigkeiten (Rust, Runtime)

| Crate | Zweck | Lizenz |
|---|---|---|
| `gtk4`, `gdk4`, `glib`, `gio`, `gsk4`, `pango` | Oberfläche, Windowing | MIT |
| `libadwaita` | GNOME-Design, Settings-Seiten, Dialoge | LGPL-3.0 |
| `webkitgtk6` (System-Bibliothek) | Reader-Ansicht | LGPL-2.1 |
| `reqwest`, `url` | HTTP/Netzwerk, URL-Normalisierung | MIT / Apache-2.0 |
| `feed-rs` | Feed-Parsing (RSS/Atom) | MIT |
| `rusqlite` (mit `bundled`) | SQLite, FTS5 | MIT |
| `ammonia`, `html5ever` | HTML-Bereinigung | MIT/Apache-2.0 |
| `tokio` | Netzwerk- und Worker-Async | MIT |
| `quick-xml` | OPML-Import/-Export | MIT |
| `serde`, `serde_json` | Serialisierung, Feedly-Payloads | MIT/Apache-2.0 |
| `base64`, `scraper`, `markup5ever_rcdom` | Medien-Encoding, Extraktion, DOM für die Bereinigung | MIT/Apache-2.0 |
| `secret-tool` (Systemwerkzeug) | Secret Service für das Feedly-Token | LGPL-2.1 |

Alle Abhängigkeiten werden dynamisch gegen die System-Bibliotheken gelinkt; es wird keine Qt-, Electron-
oder Web-Runtime gebündelt. Die vollständige, maschinenlesbare Liste steht in `Cargo.lock`
(`cargo license`/`cargo deny` sind in dieser Umgebung nicht installiert und daher nicht Teil des
Nachweises — siehe `docs/known-limitations.md`).

## Verträge

Feedly wird ausschließlich über die in `docs/feedly-api-vertrag.md` dokumentierte, live geprüfte
Marker-/Stream-Schnittstelle angesprochen. Es findet kein Scraping, keine Umgehung von Zugriffs-
beschränkungen und keine Nachnutzung fremder Oberflächen statt.
