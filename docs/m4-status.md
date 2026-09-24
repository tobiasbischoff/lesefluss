# M4 — Status und Abnahme

**Stand:** 24.09.2026.

## Umgesetzt

- **provider-feedly**: Client für `cloud.feedly.com/v3` (Profile, Subscriptions,
  Categories, `streams/contents` mit `continuation`/`newerThan`/`unreadOnly`,
  `streams/ids`, `entries/.mget`, `markers/reads`, `markers/counts`),
  serde-DTOs mit fehlertoleranten Defaults; Mock-Server-Test für
  Paginierung/Continuation.
- **Erst-Sync** (`feedly_sync::initial_sync`): Profil → Account-Zeile
  (`accounts.kind='feedly'`), Subscriptions/Kategorien → Feeds/Gruppen mit
  `remote_id` (Migration 3), Inhalte über 30-Tage-Fenster paginiert,
  Ungelesen-/Saved-Status aus Entry-Feldern übernommen.
- **Delta-Sync** alle 15 min (Scheduler im UI-Thread-getriggert, Ausführung auf
  Tokio): `markers/reads` mit 5-min-Überlappung → gelesen markieren; neue
  Entries via `newerThan`; **Saved-Reconciliation** über vollständiges
  `streams/ids`-Inventar des Saved-Streams (bidirektional für bekannte Artikel).
  `account_state.last_sync_ms` je Phase.
- **UI**: Konto-Zeile in der Quellenliste (Cloud-Icon, Ungelesen-Zahl),
  `Scope::Account` (eigene Ansicht pro Konto, vermischt nichts),
  „Feedly verbinden …" im Bibliotheksmenü (Token-Dialog, Speicherung chmod 600
  als gekennzeichneter privater Testpfad; Secret-Service-Anbindung offen → M6),
  Toasts für Sync-Ergebnisse.
- Live verifiziert: Erst-Sync lud 1524 Artikel des Testkontos, Konten/Feeds/
  Gruppen korrekt in der DB, Sidebar zeigt Konto + Feedly-Feeds getrennt von
  lokalen Feeds (OPML-Import des Nutzers parallel sichtbar).

## Offen (bewusst)

- **Schreibpfad (Outbox, markAsRead/keepUnread/markAsSaved remote, Konflikte,
  Undo über Konten hinweg)** = M5.
- Secret-Service/Keyring statt Token-Datei = M6.
- Feedly-OPML-Bulk = nicht in V1 (Spec §11.3).
