# Technisches Review und verbindlicher Fertigstellungsplan

Stand: 24.09.2026. Geprüfte Basis: `58e48d38dc4cf8cf11401994d8e1d309e253693d`.
Adressat: der weiter implementierende Agent.

## Ergebnis und Arbeitsauftrag

Die App hat eine brauchbare Grundlage (GTK/Adwaita, separater DB-Worker, parametrisierte
SQL-Abfragen, transaktionale Batch-Upserts, HTML-Allowlist und restriktive CSP). Sie ist
aber **noch nicht releasefertig**. Es fehlen nicht nur Bedienungsdetails: Kontogrenzen,
Statusänderungen, Sync-Vollständigkeit, Restore und Netzwerk-/Mediengrenzen enthalten
konkrete Fehler. Die Aussage in `m7-offen.md`, Kern und Sicherheitsrahmen seien bereits
erreicht, ist durch den aktuellen Code nicht gedeckt.

Arbeite die folgenden Pakete in der angegebenen Reihenfolge ab. Der ursprüngliche
[Implementierungsplan](../Implementationsplan-RSS-Reader-Omarchy.md) bleibt der
Produktvertrag. [m7-offen.md](m7-offen.md) ist eine ergänzende Checkliste, aber weder
Vollständigkeitsnachweis noch Erlaubnis, die unten genannten Probleme zu verschieben.
Eine vereinfachte Architektur ist erlaubt, wenn sie dieselben Invarianten erfüllt.
Eine reduzierte Produktzusage muss ausdrücklich als Abweichung dokumentiert werden;
sie zählt nicht automatisch als Erfüllung des ursprünglichen Plans.

Für jeden Fix: reproduzierbaren Regressionstest ergänzen, Implementierung korrigieren,
Test und betroffene Integration prüfen, erst dann abhaken. Tests dürfen einen bisherigen
Fehler nicht als Sollverhalten festschreiben. Keine echten Nutzerdaten für destruktive
Tests verwenden; separate XDG-Verzeichnisse und Wegwerf-Datenbanken benutzen.

## Prüfungsumfang und belastbare Nachweise

Gelesen wurden die relevante Implementierung aller sieben Crates, die Spezifikation
und die Status-, API-, Datenschutz-, Performance- und Restarbeitsdokumente in `docs`.
Dieses Review verändert keinen Anwendungscode und führt keine Feedly-Schreibaufrufe,
keine Restore-Aktion und keine Installation in der Benutzerumgebung aus.

- `cargo test --workspace --locked`: erfolgreich, **29 Tests**, verteilt auf fünf
  Testprogramme mit Tests; weitere Programme und Doc-Tests enthalten null Tests.
  Der erste Versuch scheiterte ausschließlich am Socket-Verbot der Sandbox;
  die Wiederholung mit erlaubten lokalen Testservern bestand vollständig.
- `cargo clippy --workspace --all-targets --locked`: Exit 0, aber Warnungen.
- Zusätzliche isolierte Aufrufe der aktuellen Storage-Implementierung und des
  unveränderten OPML-Parsers bestätigten folgende Fehler:

| Probe | Beobachtetes falsches Ergebnis | Soll |
|---|---|---|
| Zwei Artikel mit `id="same"`, einer lokal, einer im Feedly-Konto | `counts().total == 1` | 2 unabhängige Artikel |
| `set_read_by_article_id("same", true)` für den Remote-Abgleich | Auch der lokale Artikel wird gelesen | Nur Zielkonto verändern |
| `due_feeds()` mit lokalem und Feedly-Feed | Beide Feed-URLs werden geliefert | Nur aktive lokale Feeds |
| Alten gelesenen Remote-Artikel mit ausstehender Outbox bereinigen | Artikel wird gelöscht | Bis zur Bestätigung erhalten |
| Artikel mit Body `oldsecretword` bereinigen, danach neuen Artikel ohne Body einfügen | Suche nach `oldsecretword` liefert den neuen Artikel | Kein Treffer |
| Zukünftiges Datum `published_ms=9999`, begrenzt auf `sort_ms=100`; nächste Seite mit UI-Cursor | Derselbe Artikel erscheint erneut | Cursor schreitet eindeutig voran |
| Gruppe G mit `<outline xmlUrl="…"></outline>` und folgendem Geschwisterfeed | Gruppen der Feeds: `[G]`, `[]` | Beide `[G]` |

Die übrigen Befunde unten sind aus konkreten Codepfaden abgeleitet; ihre jeweiligen
Tests sind Arbeitsaufträge. Keine vollständige GUI-, Live-Feedly-, Hardware- oder
Abhängigkeits-Sicherheitsabnahme wurde in diesem Review durchgeführt. Die grüne
bestehende Suite deckt diese Fehler bislang nicht ausreichend ab.

## A — Datenintegrität und gefährliche Fehler zuerst

### A1 — Restore und Migrationen ausfallsicher machen (P0)

**Fundstellen:** `crates/app/src/main.rs:26–36`, `window.rs::restore_dialog`,
`storage::Database::open/migrate`, `dbworker.rs::DbWorker::start`.

`restore_dialog` kopiert jede gewählte Datei ungeprüft nach `restore.pending`.
Beim nächsten Prozessstart werden WAL/SHM gelöscht und die aktive Datenbank
überschrieben; Fehler werden ignoriert und die Pending-Datei anschließend entfernt.
Dies geschieht sogar vor der Registrierung als einzelne GTK-Anwendungsinstanz.
Eine zweite gestartete Instanz könnte somit eine noch benutzte Bibliothek ersetzen.
Eine beschädigte Datei oder voller Datenträger kann den vorhandenen Bestand zerstören.
Vor Migrationen wird zudem kein automatisches konsistentes Backup angelegt.

- [ ] Restore erst nach exklusiver Instanz-/Bibliothekssperre und vor Start aller
  DB-/Netzworker ausführen. Keine Sidecars einer noch offenen Datenbank entfernen.
- [ ] Kandidat begrenzt einlesen, in Staging-Datei prüfen: SQLite-Integrität,
  erwartete Tabellen, Fremdschlüssel, unterstützte Schema-Version. Validierung darf
  die produktive Datenbank nicht verändern.
- [ ] Vorherigen konsistenten Bestand sichern; Kandidaten auf demselben Dateisystem
  atomar aktivieren. Fehler in Kopie, Flush, Migration oder Rename erhalten den
  alten Bestand und eine verständliche Fehlermeldung. Pending erst nach Erfolg löschen.
- [ ] Vor Schemaänderungen konsistentes Backup erstellen; neuere Versionen vor
  schreibenden Initialisierungsoperationen erkennen. DB-Startfehler an die UI melden,
  statt den Worker still zu beenden und eine leere App zurückzulassen.
- [ ] Regressionen: Nicht-SQLite-Datei, korrupte DB, neuere Version, Copy-/Disk-full-
  Fehler, Crash an jeder Austauschgrenze, Zweitstart bei laufendem Fenster,
  fehlgeschlagene Migration, erfolgreiche Wiederherstellung inklusive Outbox/Prefs.

**Abnahme:** Bei jedem Fehlschlag bleibt entweder die alte oder eine vollständig
validierte neue Bibliothek nutzbar; niemals ein ungeprüfter oder halber Ersatz.

### A2 — Artikelidentität und Kontogrenzen durchgängig korrigieren (P0)

**Fundstellen:** `storage/src/lib.rs::set_read_by_article_id`,
`set_saved_by_article_id`, `account_id_for_article`, `feed_id_by_url`, `counts`;
`window.rs::dedupe_by_article_id`; `model.rs::article/row_pos/set_article`;
Reader- und Timer-Schlüssel in `window.rs` und `reader.rs`.

Remote-Statusupdates verwenden `WHERE id=?` ohne Konto. Die Kontoermittlung nimmt
`LIMIT 1`. UI-Auswahl, Timer und Reader-Ergebnisse werden häufig nur über die String-ID
zugeordnet. Die UI dedupliziert auch lokale Feeds anhand `(account,id)`, obwohl lokale
GUIDs nur innerhalb ihres Feeds eindeutig sind. Globale Counts deduplizieren sogar
kontoübergreifend. Ein Teil der vorhandenen Dedup-Tests zementiert diese Vereinfachung.

- [ ] Typisierten Artikel-Schlüssel einführen und durch Storage, UI, Undo, Timer,
  Reader, Medienjobs und Sync transportieren: lokal `(feed_id, local_id)`, Feedly
  `(account_id, remote_entry_id)` mit separaten Feed-Zuordnungen oder gleichwertiger
  zentraler Identitätsauflösung.
- [ ] Alle Status-/Inhalts-/Kontolookups explizit scopen. Lokale Gruppen und Feeds
  dürfen nie über reine URL-/Namensgleichheit Remote-Konten zugeordnet werden.
- [ ] Counts und Listen müssen dieselbe Identitätsregel verwenden; lokale Artikel
  aus unterschiedlichen Feeds bleiben unabhängig. Feedly-Mehrfachzuordnungen teilen
  einen effektiven Status innerhalb desselben Kontos.
- [ ] Migration vorhandener Daten planen: durch direkten RSS-Abruf entstandene
  Remote-Dubletten nicht blind anhand Titel oder URL löschen. Unklare Zuordnungen
  erhalten und diagnostizieren; bestehende Outbox/Lesepositionen erhalten.
- [ ] Tests mit gleicher GUID in zwei lokalen Feeds, gleicher Entry-ID in zwei
  Konten und einer Feedly-Entry in mehreren Zuordnungen; alle Aktionen einschließlich
  Undo, Reader-Wechsel und verspätetem Medienergebnis prüfen.

**Abnahme:** Keine Aktion, Zählung oder verspätete Antwort verändert/verwechselt einen
Artikel außerhalb ihres logischen Schlüssels.

### A3 — Lokalen Zustand und Outbox atomar und revisionsfest behandeln (P0)

**Fundstellen:** `window.rs::apply_status/undo/mark_scope_server`,
`feedly_sync.rs::process_outbox`, Storage-Outbox-Methoden.

Ein DB-Job ist **keine Transaktion**: `set_status` und `enqueue_outbox` committen derzeit
getrennt. DB-Fehler aus `apply_status` werden nicht ausgewertet. Die serverweite Aktion
sendet außerhalb der Outbox und meldet Erfolg, bevor die Antwort vorliegt. HTTP 404
quittiert derzeit den gesamten Marker-Batch als Erfolg. Outbox-Claim und Leseabfrage
sind getrennt; Fehlerrückmeldungen kennen keine Revision. Nach Löschen einer quittierten
Zeile beginnt `revision` wieder bei 1, also ist sie keine dauerhafte Feldgeneration.

- [ ] Eine Storage-Domänenoperation implementieren: effektiven Zustand, monotone
  Feldrevision und Mutation gemeinsam committen. Auch Auto-read, Undo und Sammelaktionen
  benutzen diese Operation. Bei DB-Fehler UI zurücksetzen bzw. sichtbar als ungespeichert
  kennzeichnen; niemals erfolgreiche Persistenz vortäuschen.
- [ ] Pro Konto/Feld laufende Schreibvorgänge koordinieren. Claim in einer Transaktion;
  ACK und Fehler an tatsächlich gesendete Revision/Request-ID binden. Neue Absicht darf
  während eines alten Requests nicht durch dessen späte Antwort bestätigt oder verzögert werden.
- [ ] 404 als differenzierten Entitäts-/Endpointfehler behandeln; nicht still quittieren.
  Permanente Fehler sichtbar erhalten. Partielle Batches nur in bestätigtem Umfang abschließen.
- [ ] Serverweite „gelesen“-Aktion in dieselbe robuste Job-/Outbox-Infrastruktur integrieren
  oder bis dahin deaktivieren. Umfang, lokale Wirkung, Bestätigung und Undo ehrlich darstellen.
- [ ] Undo muss auch für inzwischen aus der Liste entfernte Artikel funktionieren.
  Nicht zusätzlich an der Outbox vorbei `set_status` ausführen. Echten Undo-/Redo-Verlauf
  führen: aktuell legt `undo` das Redo wieder auf denselben Undo-Stack.
- [ ] Tests: Crash zwischen Status und Enqueue, voller Datenträger, verlorenes ACK,
  read→unread während Request, verspätetes ACK/Fehler, parallele Ticks, 404/Teilbatch,
  Undo nach Quellenwechsel und nach bestätigtem Upload, mehrere aufeinanderfolgende Undos.

### A4 — Retention, FTS und Inhaltslebenszyklus reparieren (P0)

**Fundstellen:** `storage/src/lib.rs::prune_old_read/upsert_article/upsert_articles/search`,
`window.rs::run_retention`; Anforderungen §9.3–9.4.

Retention löscht Artikelmetadaten und ignoriert ausstehende Mutationen. FTS-Zeilen
bleiben zurück. Da Artikel-rowids wiederverwendet werden können, treffen alte Texte
später auf einen anderen Artikel zu (oben reproduziert). Artikel ohne HTML werden
überhaupt nicht indexiert; Titeländerungen ohne HTML und Feed-Umbenennungen aktualisieren
FTS nicht zuverlässig. `search` behauptet stets `has_content=true`.

- [ ] Aufbewahrung gemäß Vertrag trennen: alte gelesene, ungespeicherte Inhalte
  bereinigen; notwendige Metadaten erhalten. Pending/inflight und ungeklärte lokale
  Mutationen ausschließen. Wiederimport-, Tombstone- und Wiederherstellungssemantik festlegen.
- [ ] FTS mit stabiler Schlüsselzuordnung transaktional pflegen; bestehende verwaiste
  Indexeinträge per Migration/Rebuild reparieren. Titel/Autor/Quelle auch ohne HTML indexieren.
- [ ] Inhaltsstatus tatsächlich abfragen; für fehlenden Inhalt verständlichen Platzhalter
  mit Original-Link anbieten statt „Web-Prozess wurde beendet“.
- [ ] Tests für Löschung/rowid-Wiederverwendung, Metadaten-only-Artikel, Rename,
  geschützte Outbox, Saved/Unread-Erhalt und Reimport. Speicher-/Zähleranzeige nach
  Retention aktualisieren.

## B — Netzwerk-, HTML- und Secret-Grenzen durchsetzen

### B1 — Downloader gegen interne Ziele und Ressourcenüberlastung absichern (P0)

**Fundstellen:** `provider-local/src/lib.rs::HttpClient`, `media.rs::get_or_fetch`,
`provider-feedly/src/lib.rs::json`; Anforderungen §10.2, §14.3.

Alle Bodies werden mit `resp.bytes()` vollständig allokiert, bevor Größenlimits geprüft
werden. Feedly hat kein entsprechendes Body-Limit. Bilder können beliebige HTTP(S)-Ziele
inklusive Loopback/Heimnetz erreichen; Redirect-/DNS-Zielprüfung fehlt. Die CSP schützt
hier nicht: den Request macht Rust. `MAX_DECODED_MEGAPIXEL` ist nur eine unbenutzte
Konstante. Externe SVG/XML-Dateien werden anhand weniger Bytes als SVG akzeptiert.

- [ ] Bodies streamend begrenzen, inklusive dekomprimierter Bytes und fehlendem/falschem
  Content-Length. Getrennte Limits für Feed, Discovery, Feedly-JSON und Bild anwenden;
  aktuell erbt ein Bild versehentlich das 10-MiB-Feedlimit trotz 12-MiB-Zusage.
- [ ] Zentrale Netzwerkpolicy für automatisch entdeckte Ressourcen: nur erlaubte Schemes,
  Zielprüfung bei jedem Redirect, IPv4/IPv6 sowie aufgelöste/verbundene IPs prüfen,
  Loopback/Link-local/private Netze abweisen; DNS-Rebinding verhindern. Explizite
  Intranet-Feedfreigabe nur eng an dessen Origin binden (§14.3).
- [ ] Bilder vor Nutzung auf Format und Abmessungen prüfen, Dekodierung/Animationen
  budgetieren; 40 MP und maximal vier parallele Bildabrufe global erzwingen.
  Externe SVG zunächst ablehnen oder über nachweislich sichere Bereinigung/Rasterung führen.
- [ ] Parsing/Sanitizing ebenfalls gegen extreme Tiefe und Größe begrenzen; rekursive
  DOM-Traversierung in `sanitize.rs` mit adversarial tiefem HTML testen.
- [ ] Mocktests: Redirect ins Heimnetz, IPv6/IPv4-mapped, DNS-Wechsel, riesiger chunked/gzip-
  Body, langsamer Stream, Pixelbombe, XML statt Bild, defektes Bild, viele schnelle Artikelwechsel.

**Abnahme:** Abbruch vor unbeschränkter Speicherallokation; keine automatisch ausgelösten
Requests an verbotene Ziele. Dies ist eine Code-Lücke, kein Nachweis eines ausgeführten Angriffs.

### B2 — Secret Service, URLs und Fehlerausgaben härten (P1)

**Fundstellen:** `feedly_sync.rs::save_token/token_from_disk/keyring_*`,
`window.rs::open_external/connect_feedly_dialog`, `reader.rs` Policy-Handler,
`tools/feedly-probe.sh`; Anforderungen §12.3, §14.2/14.4.

Die Fallback-Datei wird erst geschrieben, dann auf 0600 gesetzt. Bis dahin gelten die
Erstellungsrechte der umask. Der Schlüsselbund hat einen globalen Token-Key statt einer
Kontobindung. Bei vorhandenem, aber abgelaufenem Token öffnet „Verbinden“ keinen Dialog
zum Ersetzen, sondern startet erneut denselben Erst-Sync. Fehler enthalten teils rohe
URLs/Serverantworten. `open_external` übernimmt Artikel-URLs ohne HTTP(S)-Allowlist.

- [ ] Token-Fallback nur ausdrücklich transparent anbieten; bereits beim atomaren
  Anlegen 0600, privates Verzeichnis, keine Symlink-Verfolgung/unsichere Ersetzung.
  Secret-Service-Zugriff zeitlich begrenzen und außerhalb des GTK-Threads durchführen.
- [ ] Token an die nach Profilprüfung bestätigte Konto-ID binden; bei erneutem Login
  abweichendes Profil nicht an die Outbox des alten Kontos koppeln. Ersetzen, Logout
  und Ablaufzustände implementieren; Outbox bei Authfehler behalten.
- [ ] Alle externen URI-Einstiegspunkte auf HTTP(S) beschränken, einschließlich Feed-
  Metadaten und Browser-Button. WebKit-Popups, Downloads und Berechtigungsanfragen explizit
  ablehnen und testen; app-eigenes JS mit Dokumentgeneration und isolierter Welt versehen.
- [ ] Fehler/Logs systematisch redigieren (Token, URL-Userinfo, private Queryparameter,
  Serverantworten). Auch Debugpfade und Probe-Script einbeziehen. Keine echten Tokens lesen
  oder in Prüfarbeitsdateien kopieren.
- [ ] Tests mit unmaskierter umask, gesperrtem/fehlendem Keyring, falschem Profil,
  abgelaufenem Token, gefährlichen URL-Schemes und sensitiven Fehlertexten.

## C — Feedly als koordinierten, vollständigkeitsbewussten Sync fertigstellen

### C1 — Provider-Routing und Kontolebenszyklus korrigieren (P1)

**Fundstellen:** `storage::due_feeds`, `net.rs::spawn_scheduler/fetch_and_store`,
`window.rs::do_refresh/bootstrap/reload_meta_keep/start_feedly_scheduler`.

Der lokale Scheduler und manuelle Refresh behandeln aktuell alle Feedly-Feeds als lokale
RSS-Feeds. Dadurch entstehen Artikel mit RSS-GUIDs im Feedly-Konto; spätere Marker können
ungültige Feedly-Entry-IDs enthalten. Der Refresh-Button startet keinen Feedly-Delta-Sync.
Nach erstmaligem Verbinden lädt `reload_meta_keep` die Kontenliste nicht nach; Scheduler
und Outbox prüfen weiterhin den alten `state.accounts`-Stand bis zum Neustart.

- [ ] Nach Kontotyp routen. `due_feeds` nur für aktive lokale Feeds; manueller Refresh
  betrifft das aktuelle Konto und geht bei Feedly ausschließlich durch dessen Coordinator.
- [ ] Konten, Fähigkeiten und Synczustand nach Login atomar ins UI übernehmen;
  Scheduler/Outbox ohne Neustart aktivieren, doppelte Timer verhindern.
- [ ] Pro Konto höchstens einen koordinierten Synczyklus; Refresh priorisiert/merkt
  Folgearbeit vor. Logout und Kontowechsel invalidieren laufende Jobs und Antworten.
- [ ] Lokaler Scheduler verwendet `refresh_min` statt festem `BASE_INTERVAL_MS`, verhindert
  doppelte gleichzeitige Feedabrufe und berücksichtigt Fehler/Retry-After.
  Dauerhafte Redirects als Aliasse speichern (`final_url` wird derzeit verworfen).
- [ ] Tests: Konto frisch verbinden, sofort offline markieren und wieder online gehen;
  Refresh je Kontotyp; null direkte Feed-Requests für Feedly; keine parallelen Zyklen.

### C2 — Pagination, Watermarks und Inventare korrigieren (P0)

**Fundstellen:** `feedly_sync.rs::initial_sync/delta_sync/ingest_entries`, Feedly-DTOs;
Anforderungen §13.2–13.4 und `feedly-api-vertrag.md` §3–4.

Erst-Sync endet nach maximal 100, Delta/Saved nach maximal 50 Seiten und setzt trotzdem
den Abschlussmarker. Saved beendet zusätzlich bei leerer Seite trotz Continuation und
entspeichert anschließend anhand dieser unvollständigen Menge. Cursorzyklen fehlen.
`#[serde(default)]` akzeptiert `{}` als leeres Inventar/Profil. Fehler in Reads-Delta und
DB-Ingestion werden teilweise geschluckt. Es gibt nur einen Abschlusszeitpunkt, keine
separaten bestätigten Phasen/Checkpoints.

Außerdem fehlen ein vollständiger Unread-Abgleich, alte ungelesene/gespeicherte Inhalte
außerhalb der ersten 30 Tage, Nachladen unbekannter Saved-IDs und regelmäßiger Metadaten-
Snapshot. `ingest_entries` überspringt unbekannte Feeds und setzt Read nur auf gelesen,
nie zurück auf ungelesen. Remote-Abos/Gruppenänderungen erscheinen im Delta nicht.

- [ ] Pagination als wiederverwendbare Zustandsmaschine implementieren: leere Seiten
  mit Cursor weiterverarbeiten, wiederkehrende Cursor erkennen, Sicherheitslimit als
  **unvollständig/Fehler** behandeln. Vollständigkeit samt Geltungsbereich speichern.
- [ ] DTO-Pflichtfelder validieren. Fehlendes `ids/items/id` darf nicht zu „leere, vollständig
  erfolgreiche Antwort“ werden. Fehlender Status bleibt unbekannt.
- [ ] Phase/Page und zugehörige DB-Änderungen atomar persistieren; DB-Fehler propagieren.
  Watermark nur nach vollständigem Erfolg auf einen sicheren Start-/Servercheckpoint
  setzen, nicht pauschal auf die spätere lokale Endzeit. Lange Läufe > Überlappungsfenster testen.
- [ ] Unread- und Saved-Inventare nach bestätigter API-Fähigkeit implementieren; fehlende
  Inhalte priorisiert über `.mget` laden, auch für nicht mehr abonnierte Ursprungsfeeds.
  Entfernte Abos erst nach vollständigem Snapshot deaktivieren, gespeicherte Artikel behalten.
- [ ] Anbietergrenzen aus dem API-Vertrag bleiben offen, bis echte Vollständigkeit belegt
  ist. Wo nur Teilinventare erreichbar sind, Status als unbekannt/teilweise anzeigen;
  aus Abwesenheit niemals unberechtigt gelesen/ungespeichert ableiten.
- [ ] Tests mit konfigurierbar kleinem Seitenlimit: Restcursor am Limit, A→B→A,
  leere Seite mit Folgeseite, `{}`, fehlerhafte Seite 2, Disk-full bei Persistenz,
  Saved älter als 30 Tage, Web keepUnread für alten Artikel, entfernte Abos,
  unbekannte Origins und langer Offline-Zeitraum.

### C3 — Pull/Push-Konflikte auch nach ACK korrekt behandeln (P0)

**Fundstellen:** `feedly_sync.rs::ingest_entries/delta_sync`, Outbox-Revisionen.

`ingest_entries` setzt gelesenen/gespeicherten Status ohne Pending-Prüfung. Die anderen
Pfade prüfen Pending und ändern erst in einem späteren DB-Job: dazwischen kann eine
Nutzeraktion liegen. Selbst eine atomare Pending-Prüfung genügt nicht, wenn ein alter
Pull erst nach dem ACK einer neueren Aktion eintrifft (§13.4).

- [ ] Bestätigten Remote-Status, effektive lokale Absicht und dauerhafte Feldrevision
  unterscheiden. Jeder Pull trägt seine Startgeneration; Anwenden und Revisionsprüfung
  erfolgen in derselben Storage-Transaktion, für **alle** Import-/Reconciliation-Pfade.
- [ ] Nach Upload wenigstens einen danach begonnenen Statusabruf zur Bestätigung nutzen;
  verspätete Konsistenz als „noch zu bestätigen“ behandeln, begrenzt erneut prüfen.
- [ ] Deterministische Tests: Pull A startet → Nutzer setzt unread → Upload/ACK → A
  meldet gelesen; analog Saved/Unsaved. Jüngere Absicht bleibt erhalten, späterer frischer
  Remote-Stand kann wieder gewinnen. Zusätzlich Pending-Check/Update-Race testen.

### C4 — Fehler, Quoten und sichtbare Zustände vervollständigen (P1)

- [ ] Zentraler Requesthaushalt pro Konto, Priorität für Nutzeraktionen/Outbox,
  Retry-After (Sekunden/HTTP-Datum), Jitter und begrenzter exponentieller Backoff.
  401/403 pausieren passend statt alle 15 Minuten denselben Fehler zu wiederholen.
- [ ] Kontozustände gemäß §13.1, letzte **vollständig erfolgreiche Phase**, ausstehende
  und dauerhaft fehlgeschlagene Änderungen anzeigen. `FetchFailed` darf nicht über
  `touch_last_sync` eine erfolgreiche Aktualisierung vortäuschen.
- [ ] Authentifizierungsweg für Distribution verbindlich klären. Manueller persönlicher
  Token bleibt bis dahin eine benannte Planabweichung; fremde Client-IDs nicht verwenden.
  Nicht verifizierte Remote-Abo-/Gruppenschreibaktionen über Fähigkeiten deaktivieren.
- [ ] Mocktests für 401/403/404/429/5xx, Teilbatch, verlorene Antwort und Login-Erneuerung;
  anschließend reale Zwei-Client-Matrix aus Paket G.

## D — Lesefluss, Suche und Import korrekt machen

### D1 — Read-Toggle, Auto-read und Zähler reparieren (P1)

**Fundstellen:** `window.rs::toggle_read/start_read_timer/apply_status/open_article`.

`was_unread` wird mit `Some(!was_unread)` an den Parameter **read** übergeben. Das setzt
für beide Ausgangszustände denselben bisherigen Zustand erneut. Trotzdem werden Zähler
verändert. Der Auto-read-Timer prüft `prefs.auto_read` nie und startet vor erfolgreichem
Laden; er kann einen noch unsichtbaren oder fehlerhaften Artikel als gelesen markieren.
Fokus wird nur am Ende geprüft, nicht auf durchgängige Sichtbarkeit.

- [ ] Toggle mit eindeutig benannter read/unread-Semantik korrigieren; keine Seiteneffekte
  bei idempotentem Setzen. Counts aus tatsächlichem Vorher/Nachher ableiten, inklusive
  Gruppen-/Kontozähler und deduplizierter Feedly-Zuordnungen.
- [ ] Timer erst nach erfolgreichem Laden des aktuellen sichtbaren Dokuments starten;
  Einstellung, Aktivität, Reader-Sichtbarkeit und manuelle Ungelesen-Sperre beachten.
  Fokusverlust/Verdecken/Artikelwechsel invalidiert die Zeitmessung.
- [ ] Regressionen: beide Toggle-Richtungen, zweimalige identische Mutation, Auto-read
  ausgeschaltet, Netzwerk hängt, Load-Fehler, Fokus weg/zurück während 800 ms,
  schmale Ansicht mit verdecktem Reader, Undo und persistierte Zähler nach Neustart.
- [ ] Ungelesen-Listenverhalten verbindlich klären: §5.2 entfernt die gelesene Zeile beim
  Weitergehen; `m2-status.md` beschreibt inzwischen einen Sitzungssnapshot. Abweichung
  mit vorhandener Produktentscheidung belegen oder ursprüngliches Verhalten herstellen.

### D2 — Cursor, Suchfilter und Listenfenster reparieren (P1)

**Fundstellen:** `storage::query_articles/search`, `model.rs::build_rows/append_rows`,
`window.rs::load_page/sync_store/reload_meta_keep`.

SQL paginiert nach `sort_ms`, UI liefert `published_ms`. `(sort_ms,id)` ist über mehrere
lokale Feeds/Konten nicht eindeutig. Der Cursor entsteht nach UI-Dedup; vollständige
Seiten mit nur bekannten Duplikaten können den Fortschritt blockieren. Ein echtes
Ende wird nie gespeichert. Suche lädt bei jeder Folgeseite wieder dieselben 200 Treffer
und ignoriert Scope/Filter. Feedly-Erfolg ersetzt die gesamte geladene Liste durch Seite 1.

- [ ] Sortierschlüssel und total eindeutigen Tie-Breaker aus DB mitliefern; Cursor aus
  der letzten **rohen abgearbeiteten** Zeile bilden oder bereits logisch korrekt in SQL
  paginieren. Explizites `has_more/end` und korrektes Fehler-/Cancellation-Handling.
- [ ] Such-API um Konto/Feed/Gruppe/Unread/Saved sowie echte Pagination ergänzen;
  Titel höher gewichten. Alte Suchgenerationen verwerfen, keine Such-UI-Blockaden.
- [ ] Begrenztes Listenfenster statt unbegrenzt wachsendem `rows`/ListStore (§7.2).
  Diffs/Splices statt `remove_all` bei Status/Sync; Auswahl und Scrollanker erhalten.
  „N neue Artikel“ mit expliziter Übernahme auch für Feedly.
- [ ] Tests: zukünftige/fehlende/korrigierte Datumswerte, identische Sortiertupel,
  201+ Treffer, doppelte Entry-Zuordnungen über Seitengrenzen, Ende und Fehlerseite,
  Konto-/Feedfilter, Sync während Seite 5 sichtbar ist, 100.000 Artikel.

### D3 — OPML-Hierarchie, Merge und Discovery korrigieren (P1)

**Fundstellen:** `app/src/opml.rs::parse_opml`, `window.rs::import_opml/export_opml`,
`provider-local/src/lib.rs::extract_alternate_links/attr/looks_like_feed`.

Jedes `</outline>` poppt die Gruppe, auch wenn das Start-Tag ein Feed war und keine
Gruppe gepusht hat. Tiefenüberschreitung meldet nur einen Einzelfehler und importiert
weiter. Der DB-Import flacht Hierarchien ab (`parent=None`), verwendet globale
Gruppennamen-/URL-Lookups und hat keine Gesamttransaktion. Export behandelt mehrere
Gruppenzuordnungen als verschachtelten Pfad und lässt `htmlUrl` weg.
Discovery schneidet den Originalstring mit Byteoffsets aus `to_lowercase()`:
Unicode mit längenverändernder Kleinschreibung kann falsche Slices/Panics erzeugen.

- [ ] OPML-Elementstack getrennt vom Gruppenpfad führen; Root/Struktur, Attribute,
  vollständigen Abschluss, Schemes und harte Limits validieren. Limitverletzung bricht
  vollständig vor Änderungen ab. Datei begrenzt und außerhalb GTK einlesen.
- [ ] Import in einer Transaktion, Lookups nach `(Zielkonto, Parent, Name)` bzw.
  `(Zielkonto, normalisierte URL)`, echte lokale Parent-Gruppen. Merge/Fehlerbericht
  nachvollziehbar; niemals lokale Gruppen an einen Feedly-Feed hängen.
- [ ] Export nach Konto wählbar; Parent-Pfade von Mehrfachmitgliedschaften unterscheiden,
  Website erhalten; Roundtrip über **Parser → DB → Export → Parser** testen.
- [ ] Discovery mit vorhandenem HTML-Parser statt Stringscanner; Links/Schemes validieren,
  URL-Deduplikation vollständig, JSON Feed berücksichtigen, nicht jedes `<?xml` blind
  als Feed behandeln. DNS/TLS/Timeout/Formatfehler differenziert anzeigen.
- [ ] Tests: explizites Feed-Endtag, unbenannte Outline, 33 Ebenen, gleiche Gruppennamen
  unter verschiedenen Parents/Konten, doppelte normalisierte URLs, XML-Sonderzeichen,
  abgebrochene Datei, Importfehler mittendrin, zwei Gruppen pro Feed, Unicode vor `<link>`.
- [ ] UI gemäß `m7-offen.md` 1.7: Zielbibliothek, Gruppe, Ausgangsstatus/„nur neu ab jetzt“,
  HTTPS-Präferenz, HTTP-Kennzeichnung, Eingaben bei Fehler erhalten, Bericht exportieren.

### D4 — Reader-Latenz, Mediencache und Positionsrennen reparieren (P1)

**Fundstellen:** `window.rs::load_reader_html/drain_once/capture_position/after_load_finished`,
`provider-local/src/media.rs`, `feedly_sync.rs::ingest_entries`.

Der gesamte Artikel wartet vor dem Rendern auf bis zu 25 sequenzielle Bildabrufe
(jeweils bis 30 s Timeout). Veraltete Jobs laufen weiter. Die Ergebnisprüfung nutzt nur
Artikel-ID, keine Konto-/Dokumentgeneration. Positionsaufnahme liest erst asynchron
DB-Daten und danach den dann aktuellen WebView; so kann Position B bei Artikel A landen.
Restore prüft ebenfalls keine aktuelle Generation. Bilder werden per globalem
String-Replace ersetzt, also auch in Linkzielen/Text. `filetime_touch` ist ein No-op,
Prune läuft beim Start/Retention statt bei Cachewachstum; Feedly befüllt `article_media`
nicht, wodurch Saved-Bilder dort nicht gepinnt sind.

- [ ] Bereinigten Text und bereits gecachte Bilder sofort anzeigen; entfernte Medien
  begrenzt asynchron mit reservierten Abmessungen ergänzen. Alte Jobs abbrechen,
  Offlinefehler rasch als Platzhalter zeigen; Generation/Kontoschlüssel überall prüfen.
- [ ] Nur geparste `img.src`-Attribute ersetzen, niemals beliebige Stringvorkommen.
  Gesamtbudget pro Dokument berücksichtigen (auch Base64-Kopien und dekodierte Bilder).
- [ ] Position am zugehörigen Dokument vor Navigation aufnehmen; spätere Capture-/Restore-
  Antworten gegen Generation prüfen. Beim Schließen speichern; Theme/Zoom korrekt erhalten.
- [ ] Cache-Metadaten mit tatsächlichem letztem Zugriff und Konto-/Referenzzuordnung;
  atomare Cachewrites, gleichzeitige gleiche Downloads deduplizieren, Beschneidung nach
  Writes und Limitänderung. Feedly-Medienreferenzen genauso pflegen wie lokale.
- [ ] Tests: hängendes erstes Bild, schneller Wechsel A→B→A, gleiche ID in anderem Konto,
  Position nach Wechsel, Theme während Download, gleicher URL-Text als Link und Bild,
  LRU-Reihenfolge, Saved-Pins, Offline-Lesen und Cachegrenzen.

### D5 — GTK-Hauptthread entlasten und Fehler sichtbar machen (P1)

**Fundstellen:** `window.rs::start_outbox_tick/backup_dialog/load_prefs/import_opml_dialog`,
Keyring-Aufrufe, `drain_once`; Anforderungen §7–8.

`recv()` im GTK-Thread sowie synchrones `secret-tool.output()/wait()` widersprechen
`m2-status.md`. Auch eine `spawn_local(async ...)`-Closure macht darin ausgeführte
Dateizugriffe/Parserarbeit nicht automatisch nebenläufig. Der 120-ms-Drain fügt bereits
Antwortlatenz hinzu, die im reinen DB-Benchmark nicht enthalten ist.

- [ ] Alle DB-, Keyring-, Datei- und schwere Parserarbeit auf Worker auslagern;
  typisierte Ergebnisse/Fehler asynchron ans UI liefern. DB-Startfehler und verlorene
  Worker-Verbindungen als erreichbaren Fehlerzustand behandeln.
- [ ] Eventgesteuerte, begrenzte UI-Verarbeitung; große Batches über mehrere Frames
  verteilen. Sidebar nicht bei jeder Einzelmutation vollständig neu bauen.
- [ ] Verzögerten DB-Worker/Keyring und großen Import mocken: Navigation/Scrollen bleiben
  bedienbar. End-to-End-Auswahl-/Suchlatenz einschließlich Queue/Render messen.

## E — Noch fehlender vollständiger Produktumfang

Diese Punkte folgen auf A–D; die Details in `m7-offen.md` 1.1–1.7 gelten ergänzend.

| Paket | Konkrete Umsetzung | Abschlussnachweis |
|---|---|---|
| Feedverwaltung | Kontextmenü/Menütaste/Shift+F10, Umbenennen, Gruppen, Verschieben, Abbestellen; lokale Parent-Gruppen und Fähigkeiten je Provider | Jede Operation per Maus/Tastatur, persistiert nach Neustart |
| Sicheres Abbestellen | Feed deaktivieren/archivieren oder Inhalte logisch unabhängig erhalten; Reabo übernimmt Status | Saved-Inhalte bleiben in UI erreichbar, keine weiteren Abrufe |
| Sortierung | Neueste/älteste zuerst je Konto, beide Cursor-Richtungen | Mehrseitige Tests mit Zeitgleichheit und Umschalten ohne Verlust |
| Aktionen | Ctrl+K, Shortcutübersicht, einzelne Buchstabenkürzel umbelegbar, Splitter-Actions, Fokus-/Esc-Kette, Text-Undo/IME | Eine zentrale Actiondefinition; alle §6-Aktionen getestet |
| Persistenz | Fenstergröße, Spaltenbreiten, Ansicht, Suchbegriff, Auswahl, Gruppenfaltung, Listenanker; Nur-Lesen-Modus | Neustart/Theme/Sync behalten Zustand; unter Wayland Fensterposition nur soweit unterstützt |
| Medien-/Speicherseite | Externe Bilder blockieren, Offline-Vorladen, DB-/Cache-/Pins getrennt | Wirkt sofort, auch auf laufende Jobs; Werte aus tatsächlichem Bestand |
| Bildvorschauen | Echte begrenzte Bilder sichtbarer Zeilen; aktuell zeigt `list.rs` nur Feed-Initialen | Einstellung zeigt/versteckt Bilder, kein Download aller Feedbilder |
| Reader-Zustände | Auszug/fehlender Inhalt/offline/Renderfehler unterscheidbar, Retry/Original/Text-Fallback | WebKit-Prozessabbruch beendet App nicht; Artikel ohne HTML bleibt bedienbar |
| Lokalisierung | Deutsche und englische Übersetzungen, gettext oder gleichwertig, Pluralformen, Datums-Locale | Beide Sprachen vollständig; fehlt in der bisherigen Restliste (§16) |
| Accessibility | AT-SPI-Namen/Rollen/Status, Fokus, 200 % Text, High Contrast, reduzierte Bewegung, RTL/CJK | Dokumentierter Orca-/Tastatur-/Skalierungsdurchgang |

**Zwei Korrekturen zur bisherigen Restliste:**

1. `storage::remove_feed` darf nicht einfach den Feed löschen und nur `article_contents`
   behalten: `articles.feed_id` hat `ON DELETE CASCADE`, Listen benötigen den Feed-Join.
   Das würde gespeicherte Inhalte unzugänglich machen. Lebenszyklus zuerst modellieren.
   Außerdem Benutzer-Titel separat vom Publisher-Titel speichern, sonst überschreibt
   `net::fetch_and_store` jede lokale Umbenennung beim nächsten Abruf.
2. „Link kopieren“ ist bereits in `reader.rs` und als `win.copy-link` implementiert;
   Gruppenfaltung existiert ebenfalls in der Sitzung. Nicht doppelt bauen, sondern prüfen.
   Bei Faltung gibt es einen Fehler: `sidebar::add_account_block` markiert nur sichtbare
   Gruppenfeeds als zugeordnet; zugeklappte Feeds erscheinen anschließend unten als
   ungruppiert. Gruppenzugehörigkeit unabhängig von Sichtbarkeit bestimmen.

## F — Architektur und Wartbarkeit während der Reparatur verbessern

`crates/sync/src/lib.rs` ist leer, `domain` enthält weitgehend ungenutzte Strukturen,
und etwa 2.930 Zeilen in `window.rs` mischen UI, Persistenz, Sync, Import und Readerjobs.
Das ist kein Anlass für einen Komplettneubau, erschwert aber genau die nötigen Tests.

- [ ] Kontozustand, Providerfähigkeiten, typisierte Identitäten und Statusoperationen
  in eine GTK-unabhängige Domäne ziehen. Den Sync-Coordinator im vorgesehenen
  `sync-engine` implementieren und HTTP, Uhr, Storage-/Fehlergrenzen injizierbar machen.
- [ ] `window.rs` auf Präsentation/Actions beschränken; OPML-Service, Settings,
  Reader-Controller und Kontoverwaltung an ihren Verantwortungsgrenzen auslagern.
- [ ] `Any`/Downcasts und große Ergebnistupel schrittweise durch typisierte Nachrichten
  ersetzen. Ignorierte `Result`s in persistenzkritischen Pfaden beseitigen.
- [ ] Prefs beim Laden validieren/clampen; negative/überlaufende Größen, NaN/Infinity
  und unbekannte Enumwerte dürfen keine riesigen Budgets oder ungültiges CSS erzeugen.
- [ ] Formatierung und Clippy-Warnungen kontrolliert bereinigen, CI mit dokumentierter
  Toolchain/Systemabhängigkeiten einrichten. `rust-version=1.85` wirklich gegen Lockfile
  prüfen oder auf die tatsächlich unterstützte Mindestversion anheben.

## G — Abschlussprüfungen und Releaseunterlagen

### Automatisierte Gates

- [ ] Alle Regressionen A–D, relevante Testmatrix §17.1 und `m7-offen.md` 2.1 bestehen.
  Tests für Sync-Service, Restore und GUI-Actions ergänzen; reine DTO-Tests ersetzen
  keine Coordinator-/Konflikttests. Nicht die Zahl der „Suites“ als Qualitätsmaß verwenden.
- [ ] Property-/Fuzz-Tests für OPML/Unicode-URLs/HTML und Mutationssequenzen, reproduzierbare
  Seeds, Zeit- und Größenbudgets. SQLite-Fault-Injection an Commitgrenzen.
- [ ] `cargo test --workspace --locked`, Clippy und Formatcheck reproduzierbar in CI.
  Bestehende falsche Count-/Dedup-Erwartungen an den Identitätsvertrag anpassen.
- [ ] Abhängigkeiten-/Lizenzaudit mit aktuellen Advisories (`cargo audit`/`cargo deny`)
  und überprüften Systembibliotheken dokumentieren. Befund, Datum, Versionen und
  tatsächlich akzeptierte Ausnahmen nennen; keine erfundene Sicherheitsfreigabe.

### Reale Abnahme

- [ ] `docs/abnahme-protokoll.md` anlegen: Commit, Umgebung, Soll, Ist, Beleg und offene
  Blocker je Fall. Keine bereits früher behauptete Abnahme ungeprüft übernehmen.
- [ ] Feedly-App ↔ Feedly-Web: Read/Unread und Saved/Unsaved je Richtung, online,
  offline, Neustart, verlorenes ACK, Tokenablauf/erneute Anmeldung, alter Saved-Artikel,
  entfernte Subscription, langsamer Pull während lokaler Mutation. Live-Schreibtests
  nur mit dafür autorisierten Testartikeln/Konto durchführen.
- [ ] Hyprland: echte 60-s-Scrollsequenz gleichzeitig mit 500 importierten Artikeln,
  500 Feeds/100.000 Artikel, 60/120 Hz soweit Hardware vorhanden; Frame-/Input-/Render-
  Latenz und Speicher inklusive WebKit messen. Fehlende Hardware als offen kennzeichnen.
- [ ] 100/125/150/200 % Skalierung, gemischte Monitore, 200 % Text, Hell/Dunkel,
  High Contrast, reduzierte Bewegung, Orca; visuelle Regression der drei Referenzansichten.
- [ ] WebKit-Crash, fehlendes Portal/Keyring, offline, langsamer Datenträger und Disk-full
  im isolierten Testprofil; Daten bleiben erreichbar und Fehler verständlich.
- [ ] Arch-Paket in sauberer Buildumgebung bauen und auf frischem Runtime-System ohne
  Entwicklerwerkzeuge installieren; lokaler Modus, Feedly, Desktopintegration und Upgrade
  einer älteren Bibliothek testen. Ein `makepkg`-Build selbst benötigt Buildwerkzeuge.

### Releaseunterlagen richtigstellen

- [ ] App-ID/Name und echte Paketquelle finalisieren, Desktop/AppStream/Icon konsistent;
  gültiges PKGBUILD mit deterministischem Quellordner, Lizenzinstallation, Metadatenprüfungen.
- [ ] LICENSE-MIT, LICENSE-APACHE, README mit Build/Test/Installation, Bedienung,
  Kontoverbindung, Datenorten und sicherer Wiederherstellung ergänzen.
- [ ] `m3/m4/m5/m6/m7-status.md`, `known-limitations.md`, `privacy.md` und
  `feedly-api-vertrag.md` anhand der endgültigen Implementierung korrigieren.
  Derzeit sind unter anderem „vollständiges Saved-Inventar“, Unread-Inventar,
  LRU, erhaltene Retention-Metadaten, UI ohne blockierende Calls und reine
  Feedly-API-Netzzugriffe nicht zuverlässig wahr. Backupziel ist ein Dateidialog,
  nicht fest das Download-Verzeichnis. Bilder können auf fremden CDNs liegen.
- [ ] Performancebericht präzisieren: `startup-ready` misst ersten Idle-Turn, nicht
  zwingend nutzbare Daten/Reader; reine DB-Zeit ist keine Auswahlreaktionszeit.
  Der dokumentierte 166,67-ms-Ausreißer widerspricht einer pauschalen Zusage
  „nie länger als 50 ms blockiert“; Frame-Ticks allein beweisen dies ohnehin nicht.
- [ ] Öffentlich nutzbaren Feedly-Login, API-Vollständigkeit und notwendige externe
  Voraussetzungen als echte Freigabe-Gates führen. Falls sie ungelöst bleiben,
  keine „vollständige Version gemäß Plan“ erklären. Spätere Features (§3.2), Flatpak
  und Originalseiten-Extraktion bleiben außerhalb dieses Fertigstellungsauftrags.

## Empfohlene Umsetzungsetappen und Abschlussdefinition

1. **Bestand schützen:** A1, B1; Fehlerszenarien zuerst in isolierten Tests sichern.
2. **Identität und Persistenz:** A2–A4, typisierte Domänenoperationen aus F.
3. **Sync stabilisieren:** C1–C4, B2; dabei den GTK-unabhängigen Coordinator aufbauen.
4. **Lesefluss reparieren:** D1–D5 einschließlich echter UI-/Service-Regressionen.
5. **Produktumfang schließen:** E und übrige Punkte aus `m7-offen.md` 1.x.
6. **Veröffentlichung nachweisen:** F abschließen, G vollständig protokollieren.

Pro Etappe kleine fachliche Commits mit Problem, Änderung und konkreter Testevidenz.
Statusdateien dürfen „erledigt“ nur melden, wenn das beschriebene Verhalten tatsächlich
geprüft wurde. Einen Index führen, der jede Muss-Anforderung aus §3–17 und jedes
DoD-Kriterium §19 auf Implementierung und Test/Abnahme verweist. Externe Blocker mit
fehlender Voraussetzung und nächstem Schritt benennen; nicht als bestanden abhaken.

**Fertig bedeutet:** kein offener P0/P1-Befund aus diesem Review, vollständiger vereinbarter
V1-Umfang, grüne aussagekräftige Regressionen, sichere Migration/Restore, dokumentierter
Zwei-Client-Sync sowie reale Installations-/Wayland-/Accessibility-/Performanceabnahme.
