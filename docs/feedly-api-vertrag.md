# Feedly-API-Vertrag (M0-Protokoll)

**Stand:** 23.09.2026. Live getestet mit persönlichem Testkonto (Plan: `standard`),
Basis `https://cloud.feedly.com`. Alle Beispiele anonymisiert; Entry-/User-IDs sind
konto-spezifische opake Strings (Spezifikation §9.1).

## 1. Zugangsweg (Auth)

Getestet und funktionierend — **Legacy-OAuth Authorization-Code-Flow mit Feedlys
eigenem Public-Client** (`client_id=feedly`, ohne Secret, mit PKCE S256):

1. `GET https://feedly.com/v3/auth/auth?response_type=code&client_id=feedly&redirect_uri=https://feedly.com/i/login&scope=https://cloud.feedly.com/subscriptions&state=<zufällig>&code_challenge=<S256>&code_challenge_method=S256`
2. Login im Systembrowser; Redirect auf `https://feedly.com/i/login?code=fac_...&state=...`
   (Die Seite selbst zeigt u. U. „Token exchange error", weil Feedlys Web-App den
   Code ebenfalls konsumieren will — der Code aus der Adressleiste/History war trotzdem gültig.)
3. `POST https://feedly.com/v3/auth/token` mit
   `grant_type=authorization_code, client_id=feedly, redirect_uri=https://feedly.com/i/login, code=..., code_verifier=...`
4. Ergebnis: `access_token` (Bearer), `expires_in: 604800` (7 Tage), `plan`, `id` (User-UUID).
   **Refresh-Token: nicht beobachtet/unbestätigt.** Ablauf → erneuter Login-Flow.

### Befundlage zur Freigabe (Spezifikation §12.1/§12.3)

- `https://feedly.com/v3/auth/dev/tokens` (alte Dev-Token-Seite): **tot**, liefert
  `400 {"errorMessage":"unsupported key format"}` (auch per curl ohne Auth).
- Self-Service-Tokens laut aktueller Doku: **nur Enterprise** (`https://feedly.com/i/team/api`,
  Admin-Rechte nötig; sonst `sales@feedly.com`).
- NewsFlash (Referenz-Implementierung, `patchedsoul/news-flash` + Crate `feedly_api 0.3`):
  nutzt **von Feedly staff ausgestellte eigene `client_id`/`client_secret`**
  (zur Buildzeit eingebettetes `feedly_secrets.json`, nicht im Repo),
  Flow gegen `https://cloud.feedly.com/v3/auth/auth`, Redirect `http://localhost`,
  Token-Tausch mit Secret. Beweist: Feedly registriert weiterhin Third-Party-Reader-Clients
  auf Anfrage.
- **Konsequenz für den Release:** Für die verteilbare App eigene Client-Registrierung bei
  Feedly beantragen (Kontakt sales@feedly.com / Entwicklerprogramm). Der hier getestete
  `client_id=feedly`-PKCE-Weg ist Feedlys eigener First-Party-Client und gilt nur als
  **klar gekennzeichneter privater Testbetrieb**, nicht als Distributionsweg.
  Fremde Reader-Client-IDs (z. B. NewsFlashs) werden nicht verwendet.

## 2. Endpoint-Matrix: bestätigt

| Zweck | Request | Befund |
|---|---|---|
| Profil | `GET /v3/profile` | 200. `id` = **nackte UUID** (nicht `user/...`!), `plan`, `email`, `login` |
| Abonnements lesen | `GET /v3/subscriptions` | 200, Liste mit `id` (`feed/<URL>`), `title`, `categories`, `velocity`, `website` |
| Abonnement anlegen | `POST /v3/subscriptions`, Body `{"id":"feed/<URL>","title":...}` | 200, Response = Subscription-Objekt; sofort in Liste sichtbar |
| Abonnement löschen | `DELETE /v3/subscriptions/<urlenc(feed/URL)>` | 200, Body `[]`; nur Ziel-Abo entfernt |
| Kategorien lesen | `GET /v3/categories` | 200, `[{id,label,created}]` |
| OPML | `GET /v3/opml` | 200, **XML** (kein JSON) |
| Stream-Inhalte | `GET /v3/streams/contents?streamId=<enc>&count=N[&unreadOnly=true][&continuation=...]` | 200: `{id,updated,continuation?,items[]}`; Item-Felder u. a. `id,origin,title,summary,content,alternate,author,published,crawled,updated,unread,categories,tags?,snippet,visual` |
| Stream-IDs | `GET /v3/streams/ids?streamId=<enc>&count=N` | 200: `{ids[],continuation?}` |
| Entries batchen | `POST /v3/entries/.mget`, Body `{"ids":[...]}` | 200, JSON-**Array** der Entries (mit `tags`) |
| Ungelesen-Zähler | `GET /v3/markers/counts` | 200: `{unreadcounts[],updated}` |
| Read/Unread schreiben | `POST /v3/markers`, Body `{"action":"markAsRead"|"keepUnread","type":"entries","entryIds":[...]}` | 200; **Wirkung per `.mget` verifiziert** (`unread` true↔false, Roundtrip ok) |
| Reads-Delta lesen | `GET /v3/markers/reads?newerThan=<ms>` | 200: `{entries[],feeds[],updated}` — Delta nach markAsRead beobachtet |
| Unreads-Delta lesen | `GET /v3/markers/unreads` | **404 — Endpoint existiert nicht (mehr)** |
| Gespeichert setzen | `PUT /v3/tags/<enc(user/UUID/tag/global.saved)>`, Body `{"entryId":"<id>"}` | 200; Entry bekommt Tag `global.saved` („Saved For Later"), erscheint im Saved-Stream |
| Gespeichert entfernen | `DELETE /v3/tags/<enc(tagId)>/<enc(entryId)>` | 200; Entry-Tag wechselt auf `global.unsaved`, verschwindet aus Saved-Stream |
| Tag-Liste | `GET /v3/tags` | 200: `[{id,label,actionTimestamp}]` |

## 3. Bestätigte Semantik-Details und Fallen

- **StreamID-Format:** `user/<UUID>/category/global.all`, `user/<UUID>/tag/global.saved`,
  `user/<UUID>/tag/global.must`. Die Kurzform `user/-/...` liefert
  `400 invalid stream id` — Profil-`id` ist eine nackte UUID und muss zu `user/<UUID>/...`
  zusammengesetzt werden. IDs/Cursor immer URL-encodieren.
- **Saved-Falle:** `POST /v3/tags/{tagId}/{entryId}` (auch mit `{"label":...}`) liefert
  **200, wirkt aber nicht** (Entry bleibt `global.unsaved`, Saved-Stream leer).
  Verbindlich ist `PUT /v3/tags/{tagId}` mit Body `{"entryId":...}`. Erfolg immer per
  `.mget`/`tags` verifizieren, nicht per Statuscode allein. → Outbox-Design (§13.5):
  ACK ≠ bestätigt.
- **Entries tragen explizite Status-Tags:** `global.unsaved` als Default-Tag; Zustände
  sind über `tags` maschinenlesbar.
- **`markers/unreads` fehlt:** Kein Unread-Delta-Journal. Konsequenz für §13.4:
  Unread-Abgleich über `markers/reads` (Delta) + periodischen vollständigen
  `streams/ids?unreadOnly=true`-Inventarabgleich; „abwesend = gelesen" nur bei
  nachweislich vollständigem Inventar.
- **Timestamps:** Millisekunden-EPOCH (`published`, `crawled`, `updated`, `actionTimestamp`),
  konsistent zur Doku.
- **Rate-Limit-Header:** In Stichproben **keine** `X-Ratelimit-*`-Header gesehen
  (nur `x-feedly-processing-time`, `x-feedly-server`). 429-Verhalten ist damit nicht
  verifiziert; Budgetierung (§13.7) konservativ fahren.
- **Plan-Grenzen:** Testkonto `standard` (Free). `streams/contents` lieferte Artikel;
  maximales Sync-Fenster/Archivtiefe für Free vs. Pro **nicht vermessen** — bleibt offen.

## 4. Offen / nicht verifiziert (M4/M5)

- [ ] Refresh-Verfahren bzw. Token-Verlängerung (7-Tage-Ablauf beobachtet; Refresh-Token nicht gesehen)
- [ ] 401/403/429-Verhalten unter Last; Rate-Limit-Signale
- [ ] Kategorien-Schreiboperationen (`POST/DELETE /v3/categories`)
- [ ] `continuation`-Semantik über Seitengrenzen, leere Seiten mit Continuation
- [ ] Vollständigkeit `streams/ids?unreadOnly=true` als Inventar (Alters-/Mengengrenzen je Plan)
- [ ] Saved-Vollständigkeit über lange Historie
- [ ] `newerThan`-Fenster-Grenzen von `markers/reads`
- [ ] Registrierung eigener Client-Zugangsdaten für die Distribution (externer Schritt, sales@feedly.com)

## 5. Reproduktion

- `tools/feedly-probe.sh` (Leseproben; Schreibproben nur mit `FEEDLY_PROBE_WRITE=1`)
- Token-Ablage: **nur** lokal, chmod 600, nie im Repo (`.gitignore`: `*.token`, `feedly-token`).
  Getestetes Token liegt derzeit in `/tmp/feedly-token-tmp` (temporär!).
