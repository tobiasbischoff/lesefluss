# Nachprüfung nach Umsetzung des Fertigstellungsplans

> **Bearbeitungsstand 25.09.2026:** Die Fundstellen unten bleiben unverändert als
> Nachweis erhalten. R1–R10 sind inzwischen behoben und automatisiert abgesichert
> (139 Tests grün, Formatcheck grün, Clippy ohne Fehler); R11 ist im CI-Workflow
> ergänzt, ein GitHub-Lauf steht aus. Offen bleiben die Live-Abnahme mit echtem
> Konto (X2), die öffentliche Anmeldung (X1) und Teile des Produktumfangs
> (P/Q in `m7-offen.md`).

**Datum:** 25.09.2026 · **Basis:** `b9421ae` · **Ergebnis des Reviews: nicht freigabefähig
(Stand vor der Behebung).**

Die Umsetzung hat substanzielle Verbesserungen gebracht. Die Aussage in `m7-offen.md`,
die technischen Review-Pakete seien erledigt und es blieben im Wesentlichen Nutzerabnahmen,
ist jedoch falsch. Mehrere ursprüngliche Fehler bestehen weiter; außerdem sind neue
Fehler hinzugekommen. Besonders der Feedly-Pager verursacht unmittelbar falsche Ergebnisse.

Maßstab bleibt [review-fertigstellungsplan.md](review-fertigstellungsplan.md).
Diese Nachprüfung verändert keinen Anwendungscode. Keine echten Konten wurden verändert,
keine Restore-Aktion auf Nutzerdaten ausgeführt. GUI-/Hardwareabnahme und Live-Feedly-Test
sind hier nicht erfolgt. Befunde sind unten als isoliert reproduziert oder aus dem
konkreten Codepfad abgeleitet nachvollziehbar beschrieben.

## Prüfung und tatsächliche Verbesserungen

- `cargo test --workspace --locked`: **82 Tests bestanden** (22 App, 8 Feedly,
  16 lokaler Provider, 7 Reader, 29 Storage). Für lokale HTTP-Testserver war eine
  Wiederholung außerhalb der Socket-beschränkten Sandbox nötig.
- `cargo fmt --all -- --check`: **fehlgeschlagen**. Das ist ein verpflichtender Schritt
  in der neuen CI und damit kein bloß optionaler Formatierungswunsch.
- Zusätzliche isolierte Proben gegen die aktuellen kompilierten Bibliotheken: vier
  Fehler bestätigt; außerdem erzeugtes Reader-JavaScript mit `node --check` geprüft
  und als syntaktisch ungültig bestätigt.
- Verbessert: Status und Outbox werden jetzt in einer Storage-Transaktion geschrieben;
  aktuelle Remote-Statuspfade enthalten Kontofilter; globale Counts unterscheiden lokale
  Feeds; Retention erhält Metadaten und schützt Pending; FTS wird besser gepflegt;
  lokale Abrufe schließen Feedly-Feeds aus; Bodygrößen werden während des Lesens begrenzt;
  neue Bilder werden auf Abmessungen geprüft und SVG abgewiesen; OPML-Import arbeitet
  transaktional mit Parent-Gruppen; Toggle-Semantik und Auto-read-Einstellung wurden korrigiert.
- Auch Feedverwaltung, frühzeitiges Text-Rendering und Releaseunterlagen sind weiter.
  Diese Verbesserungen ersetzen aber keine Prüfung der vollständigen Aufrufketten.

# Nachprüfung 2026-09-25 — Bearbeitungsstand

Die Fundstellen unten bleiben unverändert als Nachweis erhalten. Der Stand vom
25.09.2026: **R1–R10 sind im Code behoben und automatisiert abgesichert**
(139 Tests grün, `cargo fmt --all -- --check` grün, Clippy ohne Fehler), **R11 ist im
CI-Workflow ergänzt**; ein GitHub-Lauf des Workflows steht noch aus. Live-Abnahmen
gegen ein echtes Feedly-Konto (X2) und die öffentliche Anmeldung (X1) sind weiterhin
offen. Details je Abschnitt in `m7-offen.md`, Nachweise in `abnahme-protokoll.md`.

## R1 — P0: Feedly lädt keine Streamseiten und kann Saved-Markierungen entfernen

**Fundstellen:** `crates/app/src/feedly_sync.rs::initial_sync/delta_sync`
(insbesondere die drei `while pager.accept(...)`-Schleifen);
`crates/provider-feedly/src/lib.rs::Pager::accept` (ab Zeile 150).

Jede Schleife startet mit `continuation=None`. `accept(None, 0, …)` setzt sofort
`complete=true` und liefert `false`. Damit wird der Schleifenrumpf **kein einziges Mal**
ausgeführt. Erst-Sync lädt Metadaten, aber keine Artikel. Delta-Sync lädt weder
Inhaltsseiten noch Saved-IDs. `saved_pager.into_result()` liefert trotzdem Erfolg.
Anschließend werden lokal gespeicherte Artikel ohne schützende Mutation anhand der
leeren Remote-Menge entspeichert und der Sync-Zeitpunkt fortgeschrieben.

**Isoliert reproduziert:** derselbe Schleifeneinstieg wie im App-Code ergibt:

```text
sync_loop: requests=0, complete=true, result_ok=true
```

Auch nur `while` durch `loop` zu ersetzen reicht nicht: `accept` wird zusätzlich nach
einer Antwort aufgerufen; derselbe Cursor darf nicht erneut beim nächsten Eintritt
registriert werden. Außerdem fehlt `into_result()` für die Inhalts-Pager.

**Auftrag:** Zuerst Seite abrufen, dann genau einmal deren Continuation auswerten;
alle Pager-Abbrüche propagieren, fehlgeschlagene/unvollständige Phasen dürfen weder
negative Reconciliation noch Watermark auslösen. Den **App-Syncpfad** gegen einen
Mockserver testen: erste Seite, mehrere Seiten, leer+Cursor, Zyklus, Limit, Fehler
auf Seite 2. Sicherstellen, dass ein existierender Saved-Artikel bei keinem dieser
Fehlerfälle entspeichert wird. Reine Pager-Unit-Tests genügen nicht.

## R2 — P1: Konfliktschutz vergleicht nicht vergleichbare Revisionen

**Fundstellen:** `storage/src/lib.rs::pull_generation` (1643), `bump_outbox` (357),
`apply_remote_status` (1675); `feedly_sync.rs::ingest_entries`.

`pull_generation` ist `MAX(revision)` aller Felder eines Kontos. Die Revision wird aber
pro Artikel/Feld separat erhöht. Hat Artikel A Revision 10, erkennt ein Pull mit
Generation 10 eine danach erfolgte Änderung an B mit Revision 1 nicht als neuer.
Nach ACK ist Pending weg; ein alter Pull überschreibt B. Zusätzlich ermittelt
`ingest_entries` seine Generation erst **nach** dem HTTP-Abruf und den Inhalts-Upserts.

**Isoliert reproduziert:** A zehnmal ändern/ACK, Pull starten, B auf ungelesen ändern/ACK,
altes gelesen-Ergebnis auf B anwenden:

```text
stale_pull: generation=10, b_revision=1, b_unread=false (expected true)
```

**Auftrag:** Konto-globalen monotonen Mutationszähler oder echten Snapshot der betroffenen
Feldrevisionen verwenden. Generation vor Beginn jedes relevanten HTTP-Abrufs erfassen
und unverändert bis zum atomaren Apply transportieren. Test mindestens mit zwei Artikeln
und unterschiedlichen Revisionshöhen; vorhandener Ein-Artikel-Test verdeckt den Fehler.

## R3 — P1: Alte Outbox-Fehler können neue Änderungen dauerhaft stilllegen

**Fundstellen:** `feedly_sync.rs::process_outbox`,
`storage::outbox_mark_inflight/outbox_fail/outbox_fail_permanent` (1765/1841).

ACK berücksichtigt Revisionen, Fehler dagegen nur Zeilen-IDs. Während ein alter Request
läuft, kann `bump_outbox` dieselbe Zeile mit neuer Absicht überschreiben. Dessen späteres
404 markiert dann die **neue** Revision als dauerhaft fehlgeschlagen. Claim ist weiterhin
von der Abfrage getrennt; parallele Prozessoren sind nicht zentral ausgeschlossen.

**Isoliert reproduziert:** Saved=true lesen, Saved=false neu enqueue'n, Fehler der ersten
Revision anwenden:

```text
old_failure: pending_new_revision=0, stuck=1
```

**Auftrag:** Claim und Requestrevision atomar erfassen; Erfolg und jede Fehlerart nur auf
passende Revision anwenden. Pro Konto/Entität Versandreihenfolge koordinieren, einschließlich
neuer Aktion während laufendem Request. Tests mit verzögerten ACKs, 404, 429 und Timeout.
401/403/429 werden aktuell weiterhin mit festen Wartezeiten erneut versucht; zentraler
Auth-/Quota-Stopp und Retry-After fehlen trotz als erledigt bezeichnetem C4.

## R4 — P0: Restore bleibt bei WAL und Zweitstart gefährlich

**Fundstellen:** `app/src/main.rs::apply_pending_restore` (29), `main` (132);
`storage::validate_candidate/migrate`.

Restore findet weiterhin **vor** GTK-Instanzregistrierung statt. Ein Zweitstart kann also
die Datenbank einer laufenden ersten Instanz ersetzen. Die Sicherheitssicherung kopiert
nur `library.db`; noch nicht eingecheckte WAL-Daten fehlen. Danach werden WAL/SHM vor
dem Rename gelöscht. Scheitert Rename oder kommt es dazwischen zum Crash, ist die
Zusage „alter Bestand erhalten“ nicht abgesichert. Auch nach einem vorherigen Absturz
können bestätigte Transaktionen ausschließlich im WAL stehen.

Validierung prüft vier Tabellennamen und Maximalversion, aber weder vollständige
Schemastruktur/Fremdschlüssel noch erfolgreiche Migration auf dem Kandidaten. Die neuen
Tests verwenden geschlossene, saubere Datenbanken und decken diese Grenzen nicht ab.
`migrate` legt weiterhin kein automatisches Backup vor Schemaänderungen an.

**Auftrag:** Exklusive Bibliotheks-/Instanzsperre vor Restore, konsistente Sicherung
inklusive WAL via SQLite, Kandidat vollständig prüfen und vor Aktivierung migrieren,
Ausfallsicherheit aller Austauschgrenzen gewährleisten. Keine Sidecars einer offenen
Datenbank löschen. Tests für offene Erstinstanz, Crash-WAL, Renamefehler, Disk-full,
formal gültige DB mit falschem Schema und fehlgeschlagene Migration ergänzen.

## R5 — P1: Netzwerkpolicy greift bei Redirects zu spät und verhindert kein Rebinding

**Fundstellen:** `provider-local/src/lib.rs::HttpClient::new/fetch_feed/fetch_image/fetch_raw`
(ab Zeile 96), `netpolicy.rs::check_url`.

Reqwest folgt Redirects weiterhin automatisch (`Policy::limited(5)`). Geprüft werden
nur Anfangs-URL und finale URL **nach** `send().await`. Zu diesem Zeitpunkt wurde das
Redirectziel bereits kontaktiert; ein Zwischenziel wird überhaupt nicht geprüft.
DNS wird zur Vorprüfung separat aufgelöst, aber die Verbindung nicht an diese geprüften
Adressen gebunden. Eine zweite, andere DNS-Antwort kann damit die Prüfung umgehen.

**Auftrag:** Jeden Redirect vor dem Request validieren, DNS-/Verbindungsadressen verbindlich
an die Policy koppeln, Proxies in der Sicherheitsannahme berücksichtigen. Tests müssen
nachweisen, dass ein lokaler Mock-Zielserver **keinen Request erhält**, nicht bloß dass die
Methode hinterher `Err` liefert. Bisherige URL-Unit-Tests belegen diesen Schutz nicht.

## R6 — P1: Feedly-Erstanmeldung öffnet ohne vorhandenes Token keinen Dialog

**Fundstellen:** `window.rs::connect_feedly_dialog/with_token/run_token_action`
(2753/2663/2678), `show_feedly_token_dialog`.

„Verbinden“ ruft `with_token(CheckConnect)` auf. Dessen Callback macht bei `None` gar
nichts. Genau beim neuen Benutzer ohne Token wird daher der Eingabedialog nie erreicht.
Mit vorhandenem Konto/abgelaufenem Token wird stattdessen der alte Erst-Sync wiederholt.

`keyring_account` und `forget_token` sind zwar implementiert, werden vom Produktpfad nicht
aufgerufen. Der einzige `save_token`-Aufruf übergibt `None`; eine tatsächliche Profilbindung
ist nicht angeschlossen. Der Schreibaufruf steht zudem im GTK-Dialogcallback und kann den
Hauptthread mit `secret-tool` blockieren. Token-Dateien werden nicht atomar ersetzt und
Symlinks weiterhin verfolgt.

**Auftrag:** Kein-Token-Fall explizit zur Anmeldung führen, abgelaufene Tokens ersetzbar
machen, Profil prüfen und Konto+Token gemeinsam zuordnen; echten Logout mit Jobinvalidierung
anbinden. Alle Keyringoperationen zeitlich begrenzt auf Worker. UI-/Service-Tests für
frisches Profil, Erneuerung, falsches Konto, Logout und gesperrten Keyring ergänzen.

## R7 — P1: UI-Identität und Paginierung weiterhin nicht durchgängig korrigiert

**Fundstellen:** `window.rs::dedupe_by_article_id` (121), `load_page` (1054),
`model.rs::article/row_pos/set_article`; `storage::query_articles_ordered` (1043).

Storage-Counts unterscheiden lokale Feeds inzwischen korrekt. Die UI dedupliziert
weiter `(account,id)` und führt Auswahl, Statusaktionen und Reader teilweise nur über
`id`. Gleiche lokale GUIDs werden versteckt; gleiche IDs in unterschiedlichen Konten
können die falsche Zeile treffen. Gruppen-Counts verwenden noch `COUNT(DISTINCT a.id)`.

SQL-Cursor `(sort_ms,id)` ist weiterhin nicht total eindeutig. In der UI ist
`has_more = rows.len() > 200`, obwohl SQL `LIMIT 200` erhält. Damit ist der Zweig immer
false und ersetzt den neuen `sort_ms`-Cursor wieder durch `published_ms`. Auch ein
explizites Ende wird nicht gesetzt.

**Isoliert reproduziert:** zwei lokale Feeds mit gleicher ID und Zeit, Seitengröße 1:

```text
tie_pagination: total=2, page1=1, page2=0 (expected 1)
```

**Auftrag:** Typisierten logischen Schlüssel bis in UI/Reader/Undo führen. Total eindeutigen
Cursor, `limit+1` oder explizite Page-Metadaten und Endzustand implementieren. Tests mit
gleicher ID in zwei lokalen Feeds/zwei Konten, zukünftiger Zeit, 201+ Zeilen und echter
UI-Nachladekette. Tests, die lediglich Storage isoliert korrekt bedienen, reichen nicht.

## R8 — P1: Medienbrücke erzeugt ungültiges JavaScript; Positionsgeneration stimmt nicht

**Fundstellen:** `app/src/reader.rs::apply_media` (217),
`window.rs::reader_doc/load_reader_html/capture_position` (2387/2408/2455).

Aus dem unveränderten `apply_media`-Formatierungscode erzeugtes JavaScript:

```js
document.dispatchEvent(new CustomEvent('lf-media', { detail: {\"url\":'https://example.com/a.png',\"data\":'data:image/png;base64,AAAA'} }));
```

**Reproduziert:** `node --check` meldet `SyntaxError: Invalid or unexpected token`.
WebKit-Fehler werden im Callback ignoriert; nachgeladene Bilder erreichen den Reader nicht.

Außerdem rendert `reader_doc` das HTML mit der **bisherigen** `document_generation`.
Erst danach setzt `load_html_doc` die neue Generation. Die Metadaten im Dokument und der
native Wert stimmen beim normalen Laden nicht überein; `capture_position` verwirft die
Antwort. Bildjobs vergleichen wiederum `read_gen`, also den Auto-read-Timerzähler, statt
ausschließlich eine unabhängige Dokumentgeneration.

**Auftrag:** Payload per JSON-Serializer erzeugen; erzeugtes JS auf Syntax und echte
WebKit-Wirkung testen. Neue Dokumentgeneration **vor** dem Rendern reservieren und für
HTML, Jobs, Capture/Restore identisch verwenden, getrennt von Timern. Konto-/Feedkey
nicht ignorieren. Tests für erstes Laden, A→B→A, Media vor/nach Load-Finished und Themewechsel.

## R9 — P1: Cache-Pruning löscht auch gepinnte Saved-Bilder

**Fundstelle:** `provider-local/src/media.rs::get_or_fetch` (160).

Nach jedem Download wird `self.prune(&HashSet::new())` ausgeführt. Pins aus der Datenbank
werden an dieser Stelle nicht berücksichtigt. Sobald der Cache voll ist, dürfen so
auch Bilder gespeicherter Artikel entfernt werden, obwohl Retention-Pruning dieselben
Dateien schützen würde. Parallelabrufe derselben URL teilen zudem dieselbe `.part`-Datei;
Cachehits umgehen die neue `image_acceptable`-Prüfung, einschließlich Altbeständen.

**Auftrag:** Pinstatus zentral in der Cacheverwaltung führen, bei jedem Prune anwenden;
Downloads je Schlüssel deduplizieren, eindeutige temporäre Dateien und Cachehit-Validierung.
Regression: Saved-Bild pinnen, Cache füllen, weiteren Artikel laden, Saved-Bild muss bleiben.

## R10 — P1: Redo speichert für sichtbare Artikel keinen Gegen-Zustand

**Fundstellen:** `window.rs::undo/redo/apply_status_inner` (2063/2082/1968).

Undo übergibt `record_undo=false`. `apply_status_inner` füllt den übergebenen Batch aber
nur, wenn genau dieses Flag true ist. Für sichtbare Artikel landet deshalb ein leerer
Redo-Batch auf dem Stack; Redo meldet Erfolg, ohne den Zustand wiederherzustellen.
Bei unsichtbaren Artikeln wird stattdessen der alte Sollzustand erneut gespeichert.

**Auftrag:** Erzeugen des Gegen-Batches und Verwerfen des bisherigen Redo-Verlaufs
separat steuern. Vorherzustände auch außerhalb der geladenen Liste aus der DB beziehen.
Actiontests für Toggle→Undo→Redo, mehrere Batches und Quellenwechsel durchführen.

## R11 — P1: CI ist auf einem frischen Container nicht ausführbar

**Fundstelle:** `.github/workflows/ci.yml`.

Der Arch-Container installiert nur `git`, nicht die GTK-/Adwaita-/WebKitGTK-
Entwicklungsabhängigkeiten. `cargo build` kann sie in `archlinux:base-devel` nicht finden.
Zusätzlich schlägt der verpflichtende Formatcheck im aktuellen Repository nachweislich
fehl. `channel="stable"` ist keine fest gepinnte Toolchain, anders als die Dokumentation nahelegt.

**Auftrag:** Systemabhängigkeiten aus der Buildanleitung in CI installieren, Formatierung
konsistent herstellen, Workflow auf frischer Umgebung ausführen und tatsächlich grünen
Lauf verlinken. Toolchain-Reproduzierbarkeit korrekt benennen bzw. Version festlegen.

## Noch nicht erledigter Planumfang

Diese Punkte sind im Code weiterhin offen und dürfen nicht aus `m7-offen.md` verschwinden:

- Vollständiger Unread-Inventarabgleich, Nachladen alter Saved/Unread-Inhalte, regelmäßiger
  Abo-/Gruppen-Snapshot und Verhalten bei entfernten Remote-Abos fehlen weiter.
  `ingest_entries` überspringt unbekannte Origins und setzt Remote-Unread weiterhin nicht
  zurück; Reads-/Reconciliation-DB-Fehler werden teilweise ignoriert.
- End-to-End-Fehler bei lokaler Persistenz: UI zeigt optimistischen Erfolg, Ergebnis von
  `apply_status_with_outbox` wird weiterhin nicht ausgewertet.
- Befehlspalette, Shortcutübersicht, individuelle Belegungen und Splitter-Actions fehlen.
- DE/EN ist nur für wenige Texte angeschlossen. Settings, Sidebar und viele Dialoge sind
  fest deutsch. Sprachdetektions-Tests beweisen keine vollständig übersetzte App.
- Bilderblockierung, Offline-Vorladen, vollständige Speicherübersicht sowie umfassende
  Ansicht-/Such-/Scrollpersistenz fehlen. Sortierung ist ein globaler Pref, nicht je Konto.
- `sync-engine` ist weiter leer. Ein UI-Flag für Delta-Sync koordiniert nicht Erst-Sync,
  Outbox und Serveraktionen gemeinsam. Serverseitiges „Alles gelesen“ umgeht weiterhin
  die transaktionale Outbox.
- Gruppenfaltung hat weiterhin den alten Fehler: Feeds eingeklappter Gruppen werden als
  ungruppiert unten wieder angehängt (`sidebar::add_account_block`, unverändert).
- Tests für echte GTK-Actions, Sync-Orchestrierung, Crashgrenzen, Fault-Injection und
  Property/Fuzz fehlen weitgehend; sie sind im ursprünglichen Plan keine optionalen Extras.

## Nächster Arbeitsauftrag

1. **R1 sofort beheben**, bevor weitere Live-Sync-Abnahmen stattfinden. Bereits lokal
   verlorene Saved-Markierungen nur aus verlässlich vollständigem Remote-Snapshot wiederherstellen.
2. R4/R5 absichern; danach R2/R3/R6 und ein integrierter Sync-Mocktest einschließlich
   Persistenz, HTTP und UI-Status. Nicht nur zusätzliche Hilfsfunktions-Tests schreiben.
3. R7–R10 mit echten Aufrufketten-/GUI-Tests korrigieren; R11 reproduzierbar grün bekommen.
4. Verbleibenden Produktumfang und ursprüngliche Abnahme-Gates zurück in die Restliste nehmen.
5. `abnahme-protokoll.md` korrigieren: Marker-200/Saved-Stream-200 und fortgeschriebene
   Watermarks belegen weder vier funktionierende Statusrichtungen noch funktionierenden
   Delta-Sync. Messwerte älterer Commits sind keine neuen Laufzeitnachweise.

Erst danach erneute Freigabeprüfung. Mehr Tests und vorhandene Helper sind ein Fortschritt;
entscheidend ist, dass die App sie in der richtigen Reihenfolge und mit richtigen Schlüsseln nutzt.
