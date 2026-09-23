# M2 — Status und Abnahme

**Stand:** 23.09.2026.

## Umgesetzt

- **Storage-Crate** (`crates/storage`): SQLite (WAL, FK an, busy_timeout), versionierte
  Migrationen mit Schutz gegen neuere Schema-Versionen, Tabellen gemäß Spezifikation
  §9.1 (accounts, groups, feeds, feed_groups, articles, article_contents,
  feed_fetch_state, account_state, FTS5 `article_fts` mit
  `unicode61 remove_diacritics 2`).
  - Re-Import ersetzt Inhalt, nie Gelesen-/Speicherstatus (getestet).
  - Keyset-Paginierung über `(sort_ms DESC, id DESC)`; zukünftige Zeitstempel werden
    nur für die Sortierung begrenzt (`sort_ms`), Original bleibt erhalten.
  - FTS-Suche: Benutzereingabe wird tokenisiert und als Präfix-Match escaped —
    keine rohe FTS-Syntax (getestet inkl. Umlaute/Diakritika).
  - Batch-Upsert pro Feed-Abruf in einer Transaktion (getestet: 50 Artikel,
    Status bleibt erhalten).
- **Provider-local** (`crates/provider-local`): reqwest (rustls, gzip) mit
  10 s Connect-/30 s Gesamt-Timeout, max. 5 Redirects, 10-MiB-Limit;
  ETag/`If-None-Match` + `If-Modified-Since` (304-Pfad getestet gegen Mock-Server);
  Discovery über `link rel=alternate` plus kleine wohlbehaltene Pfad-Heuristik;
  Parsing via feed-rs (RSS 2.0 + Atom getestet); Identität: GUID/Atom-ID →
  normalisierte URL (Fragment entfernt, Default-Ports entfernt, Query/Case erhalten)
  → deterministischer Hash.
- **App-Architektur**: Dedizierter DB-Worker-Thread mit serialisierten Jobs
  (`crates/app/src/dbworker.rs`), Tokio-Runtime mit 2 Workern für Netz
  (`net.rs`), UI-Drain alle 120 ms (keine blockierenden Calls im UI-Thread,
  kein `block_on`).
  - Scheduler: 30-min-Intervall pro Feed, Backoff bei Fehlern (30 min ≪ 24 h,
    Jitter), Parallelität global 6 / je Host 2 (Semaphoren).
  - Netz-Events (FetchDone/Failed/Discovery) laufen über denselben Drain;
    neue Artikel erscheinen sofort, wenn die Liste oben steht, sonst Zähler + Toast
    (Scroll-Anker-Regel aus §5.2).
- **UI auf DB-Basis**: Quellen/Counts/Listen/Reader-Inhalt kommen aus SQLite;
  Statusänderungen optimistisch lokal + Job an den Worker; Undo-Stack über Batches;
  Suche (Strg+L) mit 150 ms Debounce gegen FTS; „Bereich als gelesen"-Dialog;
  Feed-hinzufügen mit Discovery-Vorschau-Dialog; Neustart lädt Bestand +
  `last_sync` aus `account_state`.
- Datenpfad: `$XDG_DATA_HOME/lesefluss/library.db` (XDG-respektierend).

## Tests

- `cargo test --workspace`: storage 5 Tests (Migration/Status-Erhalt, Keyset,
  FTS-Unicode, Gruppen-Dedup, Batch-Transaktion), provider-local 6 Tests
  (RSS/Atom-Felder, Identitäts-Fallback, URL-Normalisierung, Discovery-Links,
  ETag/304 gegen lokalen Mock-Server).
- Live-Verifikation: heise-Atom + Lobsters-RSS abonniert (LF_SUBSCRIBE),
  175 Artikel geladen, Reader rendert echte Inhalte, Auto-gelesen senkt Zähler,
  Zeile bleibt bis Auswahlwechsel stehen und graut aus.

## Bekannt/offen (bewusst verschoben)

- OPML, Aufbewahrung/Backup, Mediencache: M3.
- Feedly: M4/M5.
- „N neue Artikel"-Banner statt sofortigem Listen-Reload bei Scrollposition > oben:
  aktuell Toast + Reload beim nächsten Quellenwechsel/Scroll-top.
- Listen-Refresh nach FetchDone baut die Seite neu, wenn oben gescrollt —
  Scroll-Anker für Einfügungen oberhalb des Sichtbereichs folgt mit dem Banner (M3).
- 100k-Datensatz-Lasttest: folgt in M2-F/manuell (Generatorskript geplant).
