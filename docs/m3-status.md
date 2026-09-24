# M3 — Status und Abnahme

**Stand:** 24.09.2026.

## Umgesetzt

- **Reader-Pipeline** (`crates/reader/src/sanitize.rs`): html5ever-Parse →
  URL-Auflösung gegen Feed-/Artikel-Base → ammonia-Clean mit expliziter
  Allowlist (semantische Tags, keine Styles/Scripts/Handler/Frames/Objects),
  `rel=noopener noreferrer nofollow` an Links, `url_schemes` nur http/https.
  Tests: Script/onerror/iframe/javascript:/meta-refresh/style/object/embed werden
  entfernt, Struktur (Tabellen/Listen/Quotes/Code) bleibt, relative URLs werden
  aufgelöst, Bildliste wird extrahiert.
- **Mediencache** (`crates/provider-local/src/media.rs`): kontrollierter Download
  (12 MiB Limit, Magic-Byte-MIME-Check, Tracking-Pixel-Heuristik ≤128 Bytes),
  Cache unter `$XDG_CACHE_HOME/lesefluss/media`, LRU-Prune auf 512 MiB beim Start,
  Pins für Medien gespeicherter Artikel (werden nicht gepruned). Bilder werden
  beim Öffnen des Artikels asynchron geladen und als `data:`-URI inline gesetzt —
  die WebView bleibt ohne eigene Netzwerkfreiheiten (CSP `default-src 'none'`).
  Inhalte werden bereits beim Abruf sanitisiert gespeichert
  (`article_contents.html` = bereinigtes HTML + Content-Hash).
- **OPML** (`crates/app/src/opml.rs`): Parse OPML 1.x/2.0 (UTF-8, Limits:
  20 MiB / 20.000 Outlines / Tiefe 32), Gruppenhierarchie, Fehlerliste;
  Import-Dialog mit Vorschau (neu/bestehend/ungültig), Merge ohne Ersetzen,
  Gruppen-Zusammenführung bestehender Feeds; Export als OPML 2.0 (atomar via
  temp+rename). Roundtrip-Test grün. Menü: Zahnrad in der Quellen-Headerbar.
- **Aufbewahrung:** beim Start werden gelesene, nicht gespeicherte Artikel älter
  als 90 Tage bereinigt; Tombstones verhindern Wiederauftauchen aus Feeds;
  Inhalte/Media/Positionen werden mitgelöscht.
- **Backup/Restore:** `VACUUM INTO`-Backup über Dateidialog; Restore merkt Datei
  als `restore.pending` vor und spielt sie beim nächsten Start ein (WAL-Shms
  werden vorher entfernt).
- **Lesepositionen:** `read_positions` (Absatzindex + Pixel-Offset + Content-Hash);
  Capture beim Artikelwechsel per App-JS, Restore nach Load-Finished nur bei
  passendem Hash; Theme-/Zoom-Reloads behalten zusätzlich den Scroll-Offset.

## Tests

`cargo test --workspace`: storage 5, provider-local 6, reader 4 (Sanitizer),
app/opml 3 — alle grün.

## Vereinfachungen (dokumentiert)

- Bilder werden als data:-URI inline eingebettet statt über eigenen URI-Scheme-
  Handler der NetworkSession; Wirkung gleich (kein WebView-Netz), Speicherbedarf
  pro geöffnetem Artikel höher — durch 12-MiB-Limit und 25 Bilder/Artikel begrenzt.
- Restore erfordert Neustart (pending-Datei), kein Hot-Swap bei laufender App.
- Aufbewahrungsdauer fest 90 Tage; Einstell-UI folgt mit dem Einstellungsdialog (M6).
