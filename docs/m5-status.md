# M5 — Status und Abnahme

**Stand:** 24.09.2026.

## Umgesetzt

- **Outbox** (Migration 4): `outbox(account_id, entity_id, field, desired, revision,
  attempts, next_try_ms, status)`, UNIQUE pro (Konto, Entität, Feld).
  - Jede Status-Mutation schreibt lokalen Zustand **und** Outbox im selben DB-Job.
  - Gewünschter Endzustand statt Toggle → Wiederholungen idempotent.
  - Coalescing: aufeinanderfolgende ungesendete Änderungen desselben Feldes
    verdichten zur jüngsten Absicht (Revision+1), Test `outbox_coalesce_and_ack`.
  - Verspätetes ACK entfernt niemals eine neuere Mutation (Revision-Vergleich),
    getestet.
  - Crash-Recovery: `inflight` → `pending` beim Start.
- **Prozessor** (10-s-Tick + sofortige Wirkung über nächsten Tick): batcht pro
  (Feld, Zustand) → `POST /v3/markers` mit `markAsRead`/`keepUnread`/
  `markAsSaved`/`markAsUnsaved` (in M0 verifiziertes Verhalten).
  Fehlerpfade: 429 → 5 min Pause; 401/403 → 15 min + Toast; 404 → quittieren
  (Entität remote nicht mehr änderbar); sonstige → Backoff 60 s.
- **Konfliktregeln (§13.5):** Remote-Deltas (Reads) und Saved-Inventar überspringen
  Entitäten mit ausstehender lokaler Absicht (`outbox_has_pending`); lokale
  Absicht gewinnt bis zum ACK, danach wieder Server.
- **Undo** läuft jetzt über `apply_status` → erzeugt kompensierende Outbox-
  Mutationen statt an der Outbox vorbei zu schreiben.
- **Live verifiziert:** Outbox-Zeile für einen Feedly-Eintrag → Prozessor sendet
  `markAsRead` → `entries/.mget` zeigt remote `unread:false` → Outbox-Zeile
  quittiert und entfernt.

## Offen

- Zwei-Client-Abnahme mit Feedly-Web in echter Offline-Phase: manueller
  Abnahmeschritt (Spec §17.3), vom Nutzer durchzuführen.
- Subscription-/Kategorie-Schreiboperationen remote: nicht in V1-Kernumfang
  laut Endpoint-Matrix offen; lokal bleiben sie möglich.
