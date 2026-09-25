# Konsolidierte Restarbeiten: lokaler Modus, Sync und Veröffentlichung

**Stand:** 25.09.2026 · **Codebasis:** `b9421ae`.
**Adressat:** implementierender Agent. Diese Liste ersetzt die vorherige Fassung,
die mehrere nur teilweise reparierte Bereiche bereits als erledigt bezeichnete.

Grundlagen:

- [Letzte technische Nachprüfung mit Fundstellen und Reproduktionen](review-nachpruefung-2026-09-25.md)
- [Ausführlicher Fertigstellungsplan](review-fertigstellungsplan.md)
- [Ursprüngliche Produktspezifikation](../Implementationsplan-RSS-Reader-Omarchy.md)

Das ist eine konsolidierte Arbeitsliste auf Basis der letzten Prüfung, keine erneute
Abnahme. Seit dieser Prüfung ist HEAD unverändert. 82 vorhandene Tests bestanden;
zusätzliche Proben bestätigten trotzdem kritische Fehler. Formatcheck schlug fehl.

**Wichtig:** Die Freigabe eines öffentlichen Feedly-OAuth-Clients ist ein externer
Blocker. Sie blockiert weder die Fertigstellung des lokalen Readers noch die Reparatur
und Mockserver-Prüfung des Syncs. Ein lokaler Release kann separat abgenommen werden;
eine vollständige Veröffentlichung gemäß ursprünglichem Plan inklusive Feedly braucht
zusätzlich einen geklärten Zugangsweg und bestandene reale Sync-Abnahmen.

## 1. Sofortmaßnahmen und Reihenfolge

- [x] **S0 — Fehlerhaften Feedly-Sync zuerst sichern (R1, 2026-09-25):**
  `Pager::after_page` wertet jede geholte Seite genau einmal aus; `Inventory::Complete`
  ist Voraussetzung für `reconcile_saved`, `Incomplete` (Zyklus, Limit) löst nur ein
  Log aus. Inhalts-, ID- und Read-Phasen propagieren Fehler, `set_last_sync` läuft
  erst nach vollständigem Abschluss. 8 App-Pfad-Regressionen gegen einen
  Mockserver mit echter DB (erster/mehrere/leere+Cursor/Zyklus/Limit/503 auf Seite 2/
  unvollständiges Inventar/Abgleich beider Richtungen), 6 Provider-Tests, 88 Tests
  workspaceweit grün, `cargo fmt --all -- --check` grün, `cargo clippy -- -D warnings` grün. Die Watermark-Semantik aus S1
  ist seit 2026-09-25 mit erledigt (Checkpoint statt lokaler Endzeit). Offen bleibt die
  Live-Abnahme X2. Bereits lokal verlorene Saved-Markierungen lassen sich nur aus einem
  verlässlich vollständigen Remote-Snapshot wiederherstellen; das ist Teil von X2.
- [x] **A1–A9 (Abschlussprüfung 2026-09-25):** alle neun Befunde behoben; ein
  Dispatcher führt Erst-Sync, Delta, Outbox und Serveraktionen mit Laufkennung,
  Abbruch und Pausen; vollständiger Leseabgleich im echten Sync-Zyklus mit beiden
  Statusrichtungen und `.mget`-Nachladen; Tokenbindung wird geprüft; unvollständige
  Statusphasen gelten als Fehler; CI baut das Paket als unprivilegierter Benutzer.
  163 Tests, strenges Clippy und Formatcheck grün. Details je Befund in
  `review-abschlusspruefung-2026-09-25.md`.
- [x] **L1/L2 (R4/R5, 2026-09-25):** Restore läuft nach exklusiver `flock`-Sicherung
  und GTK-Einzelinstanz-Registrierung; Sicherung über `VACUUM INTO` inklusive WAL,
  Kandidat wird vollständig geprüft (Tabellen- und Spaltensatz, Fremdschlüssel,
  Quick-Check) und vor der Aktivierung migriert, Tausch als `library.db.new` + Rename
  mit Recovery beim nächsten Start; Migrationsbackup vor jeder Schemaänderung.
  Redirects werden manuell und Hop für Hop geprüft (`Policy::none()`), der Resolver
  gibt nur policy-konforme Adressen an den Connector. Tests belegen, dass ein lokaler
  Mock **keinen** Request erhält, sowie Absturz-WAL, Renamefehler, falsches Schema,
  Crash beim Tausch und gesperrte Zweitinstanz.
- [x] **L3–L7 (R6–R10, 2026-09-25):** Ohne Token führt „Verbinden“ in den Dialog,
  Profilbindung im Schlüsselbund, echtes „Feedly trennen“ mit Coordinator-Sperre,
  alle Secret-Zugriffe mit Zeitlimit im Worker, atomares Ersetzen ohne
  Symlink-Verfolgung. Typisierter Schlüssel (Feed + ID) bis in Liste, Statusaktionen
  und Undo; Cursor (sort_ms, feed_id, id) total eindeutig, Laden mit 201 Zeilen und
  explizitem Ende. Reader-JS aus dem JSON-Serializer (`node --check` grün),
  Dokumentgeneration vor dem Rendern reserviert. Cache-Pins werden bei jedem Prune
  berücksichtigt, Downloads je Schlüssel dedupliziert, Altbestände werden geprüft.
  Undo/Redo erzeugt echte Gegen-Batches.
- [x] **S1–S6 (2026-09-25):** Watermark = sicherer Checkpoint; account-globale
  Mutationssequenz statt `MAX(revision)` (Migration 13), Generation vor dem HTTP-Abruf
  erfasst; Outbox-Claim atomar, Erfolg **und** Fehler an die gesendete Revision
  gebunden, 429 mit `Retry-After`, 401/403 als zentraler Auth-Stopp; Bestätigung nach
  Upload per `.mget` mit erneuter, revisionsfester Absicht; getrennte Unread-/Saved-
  Inventare, Nachladen fehlender Inhalte per `.mget` (Objektform mit Fallback auf das
  JSON-Array), Coordinator in `sync-engine` für Erst-/Delta-Sync, Outbox und
  Serveraktionen mit Prioritäten und Pausen. Ende-zu-Ende-Mocks: 12 App-Pfad-,
  8 Provider-, 8 Coordinator- und 40 Storage-Tests; insgesamt 139 Tests grün.
- [ ] **P1–P6/Q2/Q5:** bleiben im Umfang (siehe unten). Erledigt wurden 2026-09-25:
  Sortierung je Konto (P2), Bilderblockierung plus Speicherübersicht (P3), Gruppierung
  eingeklappter Gruppen (L7), Auswertung des Persistenzergebnisses (L6), Log-Redaktion
  (S5) und der CI-Workflow mit Systembibliotheken sowie Paketbau (Q1/Q4).
- [ ] Live-Abnahme X2 und öffentliche Anmeldung X1 bleiben externe Blöcke.

Pro fachlichem Fix: Regression zunächst reproduzieren, implementieren, betroffenen
Aufrufpfad testen, Commit und Beleg dokumentieren. Keine produktiven Datenbanken für
Crash-, Restore- oder Disk-full-Tests verwenden. Keine Abschlussmarkierung allein
aufgrund eines vorhandenen Helpers oder einer grünen Gesamttestsuite.

## 2. Lokaler Modus und gemeinsam genutzte Infrastruktur

### L1 — Sichere Wiederherstellung und Migrationen · kritisch · R4 · erledigt 2026-09-25

- [x] Exklusive Bibliotheks-/Instanzsperre **vor** Restore und Workerstart; Zweitstart
  darf niemals Dateien der laufenden Instanz ersetzen.
- [x] Konsistente SQLite-Sicherung einschließlich WAL statt Kopie nur von `library.db`.
- [x] Kandidat vollständig auf Schema, Integrität und Fremdschlüssel prüfen, im Staging
  migrieren und erst dann ausfallsicher aktivieren. Sidecars nicht vor gesichertem Austausch löschen.
- [x] Fehler bei Copy/Flush/Rename/Migration erhalten den vorherigen Bestand und melden
  den tatsächlichen Zustand. Vor Schemaänderungen automatische konsistente Sicherung.
- [x] Tests: Crash mit uncheckpointetem WAL, laufende Erstinstanz, Renamefehler,
  falsches Schema, fehlgeschlagene Migration. Ein echter Disk-full-Versuch bleibt als
  Gerätevorbehalt offen (der Fehlerpfad ist über fehlende Staging-Dateien abgedeckt).

**Fertig wenn:** Jeder getestete Abbruch lässt einen vollständigen alten oder neuen
Bestand zurück; Backup, Outbox und Einstellungen bleiben konsistent.

### L2 — Netzwerk- und Inhaltsgrenzen · hoch · R5 · erledigt 2026-09-25

- [x] Jeden Redirect **vor** dem Request prüfen, nicht erst die finale URL nach `send`.
- [x] DNS-Prüfung und tatsächliche Verbindungsadresse koppeln (`PolicyResolver` gibt nur
  freigegebene Adressen an den Connector); Rebinding, IPv4/IPv6, Loopback/private Netze
  sind abgedeckt. Proxywege sind nicht abgesichert und in `known-limitations.md` benannt.
- [x] Explizite Intranetfreigabe nur für die betreffende Origin; keine globale Ausnahme
  für automatisch geladene Artikelbilder.
- [x] Streamende Größenlimits (chunked ohne Content-Length) sind per Test abgesichert;
  HTML/XML-Tiefenlimits bleiben eine Parserfrage ohne eigenen Nachweis.

**Fertig wenn:** Verbotene Mock-Zielserver erhalten keinen Request; übergroße Antworten
werden während des Lesens abgebrochen. Ein nachträgliches `Err` allein genügt nicht.

### L3 — Eindeutige Artikelidentität und Listenpagination · hoch · R7 · erledigt 2026-09-25

- [x] Lokal `(feed_id, article_id)` durchgängig für UI, Auswahl, Status, Undo und Timer;
  Feedly-Artikel tragen die globale Eintrags-ID im Konto, die Dedupe über
  `(Konto, ID)` bleibt. Medien und Capture nutzen denselben Schlüssel.
- [x] Lokale Feeds mit gleicher GUID werden nicht zusammengefasst; Gruppen-Counts zählen
  je (Feed, ID).
- [x] Total eindeutiger Cursor (sort_ms, feed_id, id); `sort_ms` wird nicht überschrieben.
- [x] Laden mit 201 Zeilen, `has_more` zuverlässig, Ende wird explizit markiert.
- [x] Trimmen, Sync und Suche erhalten Auswahl und Anker; Suche nutzt denselben Cursor.
      Rückwärtsblättern des Listenfensters ist nicht implementiert und bleibt offen.

**Tests:** gleiche GUID in zwei lokalen Feeds, gleiche ID in zwei Konten, gleiche Zeiten,
zukünftige Zeit, mehr als 200 Treffer, Listenende, Fehlerseite, Sync beim Lesen auf Seite 5.

### L4 — Reader-Bilder und Lesepositionen · hoch · R8 · erledigt 2026-09-25

- [x] Ungültiges JavaScript aus `apply_media` durch JSON-Serialisierung ersetzt
  (`node --check` und Auswertung als Test).
- [x] Dokumentgeneration vor HTML-Erzeugung reserviert; HTML, Medienjobs und Capture
  verwenden dieselbe Generation und den vollständigen Artikelschlüssel.
- [x] Dokumentgeneration vom Auto-read-Timer getrennt; alte Antworten werden verworfen.
- [x] Text sofort, Bilder schrittweise; Dateizugriff und Kodierung laufen im Worker.
- [ ] Offen: WebKit-Wirkung und A→B→A, Themewechsel und Neustart sind **ohne** laufende
  Oberfläche nicht automatisiert geprüft; Position beim Wechsel/Schließen nutzt weiterhin
  die Read-Timer-Logik und ist nur manuell abnehmbar.

**Tests:** erzeugtes JS syntaktisch gültig und Wirkung im WebKit geprüft; erstes Laden,
A→B→A, gleichnamige IDs, Bild vor/nach Load-Finished, Themewechsel und Neustart.

### L5 — Offline-Bildcache zuverlässig machen · hoch · R9 · erledigt 2026-09-25

- [x] Pinstatus wird beim Download aus der Datenbank gesetzt und bei **jedem** Prune
  berücksichtigt.
- [x] Gleichzeitige Downloads derselben URL werden dedupliziert, temporäre Dateien sind
  eindeutig (kein gemeinsames `.part`).
- [x] Cachehits und Altbestände werden geprüft und bei Verstoß entfernt; Abmessungen,
  Größe und Tracking-Pixel gelten auch für Bestandsdateien.
- [ ] Offen: „Tatsächliches LRU“ ist nur eine mtime-Näherung; Konto-/Artikelreferenzen
  werden nicht getrennt geführt.

**Fertig wenn:** Ein weiteres heruntergeladenes Bild kann ein gepinntes Saved-Bild nicht
verdrängen; gespeicherte Inhalte bleiben offline lesbar und der Cache wächst kontrolliert.

### L6 — Statusänderungen, Undo/Redo und Fehlerbehandlung · hoch · R10 · erledigt 2026-09-25

- [x] Gegen-Batches werden unabhängig vom Verwerfen des Redo-Verlaufs erzeugt
  (`StatusMode::User`/`Counterpart`); außerhalb des Listenfensters wird aus dem
  DB-Vorzustand heraus gearbeitet.
- [x] Das Persistenzergebnis wird ausgewertet: bei DB-Fehler wird die Anzeige
  zurückgesetzt, Zähler neu geladen und der Fehler gemeldet.
- [ ] Auto-read: Timer wird bei Fokusverlust/Verdecken invalidiert, manuelles Ungelesen
  hat Vorrang — vorhanden, aber ohne GUI-Test abgewiesen; bleibt manuell abnehmbar.
- [ ] Fehlende Inhalte, Feed-Auszug und WebKit-Crash werden unterschieden; Text-Fallback,
  Original öffnen und Wiederholen sind nicht vollständig umgesetzt.

**Tests:** Toggle→Undo→Redo, mehrere Batches, Quellenwechsel, idempotentes Setzen,
DB-Schreibfehler, Auto-read aus, Fokus weg/zurück, verdeckter Reader, WebKit-Crash.

### L7 — Feed-/Gruppenverwaltung und OPML vollständig prüfen

- [x] Eingeklappte Gruppen behalten ihre Feeds (Test `eingeklappte_gruppen_behalten_ihre_feeds`).
- [ ] Verschachtelte lokale Gruppen anlegen/verschieben/umbenennen, Mehrfachzuordnungen,
  Abbestellen und Wiederabo einschließlich Status-/Saved-Erhalt durchgängig testen.
- [ ] OPML-Roundtrip über **Parser → Datenbank → Export → Parser**, nicht nur Parser/Writer:
  Hierarchie, gleiche Gruppennamen unter verschiedenen Parents, Mehrfachmitgliedschaften,
  Kontoisolation, URLs, Website, Sonderzeichen und Limits erhalten.
- [ ] Hinzufügen/Import: Gruppe/Zielbibliothek und Ausgangsstatus wählen, „nur neu ab jetzt“,
  verständliche DNS/TLS/Timeout/Formatfehler, HTTPS-Präferenz/HTTP-Kennzeichnung,
  Eingabe bei Fehler erhalten, Ergebnisbericht exportieren.

**Fertig wenn:** Gesamter lokaler Workflow ohne Feedly funktioniert: hinzufügen/importieren,
lesen/suchen/markieren, gruppieren, exportieren, offline nutzen, sichern/wiederherstellen.

## 3. Feedly-Sync reparieren — unabhängig von öffentlicher App-Freigabe

### S1 — Pagination, Vollständigkeit und Watermarks · kritisch · R1

- [x] Ersten HTTP-Abruf immer ausführen; Continuation genau einmal pro Antwort verarbeiten.
- [x] Leere Seite mit Cursor weiterführen; Zyklen und Seitenlimit ergeben „unvollständig“.
- [x] Sämtliche Inhalts-/Status-Pagerfehler propagieren. Keine negative Reconciliation
  oder Fortschrittsmarkierung nach übersprungenem/fehlgeschlagenem Abruf.
- [ ] Watermark an sicheren Start-/Servercheckpoint und tatsächlich abgeschlossene Phase
  binden, statt pauschal lokale Endzeit zu speichern.

**Abnahme:** App-Sync gegen Mockserver mit echter DB: erster Request, mehrere Seiten,
leer+Cursor, Zyklus, Limit, Fehlerseite, DB-Fehler. Saved bleibt bei unvollständigem Abruf erhalten.
Stand 2026-09-25: alle Fälle außer DB-Fehler und End-to-End über `initial_sync`/`delta_sync`
(`FeedlyClient::new` verdrahtet die Produktivbasis, dort ist der Mockserver noch nicht
einschleusbar) automatisiert grün; kein Live-Nachweis.

### S2 — Vollständigen Leseabgleich implementieren

- [x] Ungelesen- und Gespeichert-ID-Mengen werden getrennt über
  `user/<id>/category/global.unread` und `tag/global.saved` gelesen; fehlende IDs
  wirken nur bei nachgewiesener Vollständigkeit negativ.
- [x] Fehlende Inhalte gespeicherter oder ungelesener Artikel werden per `.mget`
  nachgeladen (Batch 200). Nicht mehr abonnierte Origins bleiben lokal bestehen.
- [x] `.mget` sendet zuerst die dokumentierte Objektform und fällt einmalig auf das
  JSON-Array der Referenzimplementierung zurück (Test beweist beide Versuche).
- [x] Fehler in Reads-/Status-DB-Operationen werden nicht mehr verschluckt, sondern
  brechen die Phase ohne Watermark ab.
- [ ] Offen: Abos/Gruppen werden weiterhin nur beim Erst-Sync abgeglichen; entfernte
  Remote-Abos deaktivieren nichts. Das ist zusammen mit X3 zu klären.

**Abnahme:** Alle vier Statusrichtungen, unbekannte Origins, alter Saved-Artikel,
entferntes Abo und begrenzte API-Historie in Integrationstests; reale Kontofähigkeiten separat belegen.

### S3 — Pull-/Push-Konflikte revisionsfest machen · hoch · R2/R3 · erledigt 2026-09-25

- [x] Konto-globale Mutationssequenz (`account_sequence` + `field_revisions.sequence`,
  Migration 13) statt `MAX(revision)`.
- [x] Generation vor HTTP-Start erfasst und unverändert bis zum Apply mitgeführt
  (auch in `reconcile_saved`/`reconcile_unread`).
- [x] Outbox-Claim atomar (`outbox_claim`); ACK **und** Fehler an die gesendete Revision
  gebunden; 429 mit `Retry-After`, 401/403 als zentraler Auth-Stopp.
- [x] Alte 404/429/Timeout-Antwort legt keine neuere Absicht still (Tests dafür).
- [x] Nach Upload erfolgt ein Statusabruf per `.mget`; Abweichungen erzeugen eine
  revisionsfeste erneute Absicht und werden sichtbar gemeldet.

**Tests:** A mit Revision 10, B mit Revision 1; alter Pull nach B-ACK; neue Absicht während
altem Schreibrequest; verlorenes ACK; Neustart mit inflight; partielle Batches; Undo nach Upload.

### S4 — Einen Coordinator, Quoten und verständliche Zustände fertigstellen · erledigt 2026-09-25

- [x] `sync-engine::SyncCoordinator` koordiniert Erst-Sync, Delta-Sync, Outbox und
  Serveraktionen pro Konto; Prioritäten und vorgemerkte Folgearbeit, kein Parallelzyklus.
- [x] Serverweite Sammelaktion („alles gelesen“) sendet erst serverseitig und aktualisiert
  danach lokal; Fehler bleiben ohne lokale Wirkung.
- [x] Auth-/Quota-Stopp als Coordinator-Pause; `Retry-After` wird ausgewertet, der manuelle
  Refresh umgeht die Pause nicht, eine neue Anmeldung hebt sie auf. Jitter ist nicht
  ergänzt (feste, aber revisionsfeste Backoffs).
- [x] Zustände bleiben getrennt sichtbar (Kontostatus, `unsynced`, gespeicherte
  Sicherungen, Toast mit Grund).
- [x] Coordinator ist GTK-unabhängig und mit acht Tests abgesichert; `sync-engine` ist
  nicht mehr leer und wird vom Produktivpfad genutzt.

### S5 — Bestehenden persönlichen Tokenpfad funktionsfähig machen · hoch · R6 · erledigt 2026-09-25

- [x] Ohne Token führt „Verbinden“ in den Dialog; mit Konto wird „neu verbinden“ angeboten.
- [x] Profil wird geprüft und per `bind_account` im Schlüsselbund festgehalten.
- [x] „Feedly trennen“ entfernt Token **und** Kontobindung, sperrt den Coordinator und
  lässt lokale Daten und Outbox unangetastet.
- [x] Alle Secret-Zugriffe laufen im Worker mit 10-s-Grenze; Fallback schreibt atomar
  über eine eigene temporäre Datei, ersetzt Symlinks und bleibt bei 0600.
- [x] `dbg_log` redigiert Queryparameter und Token; Test vorhanden.
- [ ] Offen: Ein Dialogtest für frisches Profil/Erneuerung/gesperrten Keyring steht aus
  (Headless-Umgebung), das Verhalten ist aber pfadseitig verdrahtet.

**Abnahme:** Frisches Profil, Loginfehler, Erneuerung, anderes Konto, fehlender/gesperrter
Keyring und Logout funktionieren ohne UI-Hänger oder Verlust lokaler Änderungen.

### S6 — NetNewsWire als Referenz nutzen, nicht ungeprüft übernehmen

Referenz: NetNewsWire `b4361413fc1850110f9f42652f0f84e7a51e9d64`,
[FeedlyAccountDelegate](https://github.com/Ranchero-Software/NetNewsWire/blob/b4361413fc1850110f9f42652f0f84e7a51e9d64/Modules/Account/Sources/Account/Feedly/FeedlyAccountDelegate.swift)
und [FeedlyAPICaller](https://github.com/Ranchero-Software/NetNewsWire/blob/b4361413fc1850110f9f42652f0f84e7a51e9d64/Modules/Account/Sources/Account/Feedly/FeedlyAPICaller.swift).

- [x] Ablauf strukturell übernommen und strenger ausgeführt: Outbox senden → Quoten
  abgleichen → Inhalte → **getrennte** Unread-/Saved-Inventare → fehlende Inhalte per
  `.mget` → Bestätigung per Statusabruf → geprüfter Fortschritt (Checkpoint).
- [x] Erster Request, Truncation-Schutz (Zyklus/Limit/Fehlerseite), getrenntes Nachladen
  und koordinierter Batchversand sind als Rust-Tests gegen einen Mockserver abgesichert
  (12 App-Pfad- und 8 Provider-Tests).
- [x] Revisions- und Crashanforderungen bleiben strenger als die Referenz; fremde
  Zeit-/Mengenlimits werden nicht als Feedly-Garantie behandelt, Zugangsdaten nicht
  übernommen.
- [ ] Offen: Abo-/Gruppen-Snapshot (S2) und Verhalten bei entfernten Remote-Abos bleiben
  unbehandelt; das ist in S2/X3 als offen geführt.

## 4. Übriger verbindlicher Produktumfang

- [ ] **P1 — Tastatur:** Ctrl+K-Palette, Shortcutübersicht, einzelne Belegungen,
  Splitter-Actions, Fokus-Rückgabe, Esc-Kette, Text-Undo und IME korrekt implementieren.
- [ ] **P2 — Zustand:** *teilweise erledigt 2026-09-25:* Sortierung ist je Konto
  (`sort_<konto>`) und fällt auf die globale Vorgabe zurück; Identität und Cursor sind
  total eindeutig (L3). **Offen:** Persistenz von Auswahl, Quelle, Filter, Suchbegriff,
  Gruppenfaltung und Scrollanker je Ansicht/Konto über Neustarts, Spaltenbreiten,
  Wayland-Fensterposition.
- [ ] **P3 — Datenschutz/Speicher:** *teilweise erledigt 2026-09-25:* Schalter
  „Externe Bilder blockieren“ (kein Nachladen, Platzhalter bleiben) und Speicherübersicht
  getrennt nach Datenbank/WAL, Bildcache und gepinnten Dateien. **Offen:** Offline-Vorladen
  gewünschter Artikel, Löschen einzelner Cache-Einträge, Auswirkung von Änderungen auf
  laufende Bildjobs.
- [ ] **P4 — Lokalisierung:** alle produktiven Texte inklusive Settings, Sidebar,
  Fehler-/Import-/Kontodialoge auf DE/EN umstellen; Plurale, Datumsdarstellung und
  Reader-Sprache korrekt. Einige übersetzte Tooltips sind keine vollständige Lokalisierung.
- [ ] **P5 — Accessibility:** alle Funktionen ohne Maus, AT-SPI-Namen/Rollen/Status,
  High Contrast, reduzierte Bewegung, Textskalierung bis 200 %, RTL/CJK und sichtbarer Fokus.
- [ ] **P6 — Reaktionsfähigkeit:** verbleibende Datei-/Keyring-/Parser-/Cachearbeit aus GTK
  entfernen, Eventverarbeitung begrenzen, End-to-End-Latenz einschließlich Queue und
  WebKit messen. Fehlerseiten statt stiller Workerabbrüche/leerer Fenster.

## 5. Tests, Distribution und nachvollziehbare Abnahme

- [ ] **Q1 — CI reparieren (R11):** *Workflow ergänzt 2026-09-25:* Der Arch-Container
  installiert gtk4, libadwaita, webkitgtk-6.0 und die Header-/Laufzeitpakete, prüft sie mit
  `pkg-config` und baut anschließend das Paket; `cargo fmt --all -- --check` besteht lokal
  (Bestand nachformatiert), `cargo test --workspace` 139/139 grün, Clippy ohne Fehler,
  `makepkg` erzeugt 5,3 MiB. **Offen:** ein tatsächlicher GitHub-Lauf des Workflows und
  die ehrliche Toolchain-Angabe: `rust-toolchain.toml` pinnt `stable` (bewegliches Ziel),
  `Cargo.lock` ist eingecheckt.
- [ ] **Q2 — Aussagekräftige Regressionen:** App-Aufrufketten und GTK-Actions testen,
  nicht nur Helper. Fault-Injection an Commit-/Restoregrenzen sowie Property/Fuzz für
  OPML, URLs, HTML und Mutationsfolgen aus der ursprünglichen Testmatrix ergänzen.
- [ ] **Q3 — Reale Laufzeitprüfung:** 500 Feeds/100.000 Artikel, 60 s echtes Scrollen
  parallel zu 500 importierten Artikeln; End-to-End-Latenz, Speicher inklusive WebKit,
  60/120 Hz soweit vorhanden, Skalierung 100/125/150/200 %, Monitormix, Orca.
  WebKit-Crash, Disk-full, fehlende Portale und Offlinebetrieb im Testprofil prüfen.
- [ ] **Q4 — Paket/Release:** *teilweise erledigt 2026-09-25:* Paketbau aus dem
  Repository mit `makepkg` grün (5,3 MiB), CI baut das Paket mit. **Offen:** Installation
  und Upgrade auf einem sauberen Runtime-System, Datenmigration ohne Verlust im Test,
  Abhängigkeits-/Lizenzaudit inklusive Systembibliotheken, README-Abgleich.
- [ ] **Q5 — Dokumentation korrigieren:** `abnahme-protokoll.md`, Statusdokumente,
  `privacy.md`, `known-limitations.md` und API-Vertrag dürfen keine unerfüllten Fähigkeiten
  behaupten. Pro Kriterium Commit, Testfall, Ergebnis und Umgebung nennen.
  Alte Performancemessungen und HTTP 200 sind keine neue vollständige Abnahme.

Automatisierbare Fehler-/Integrationsprüfungen sind Agentenarbeit, keine pauschalen
„nur vom Nutzer prüfbaren“ Aufgaben. Physisches Scrollgefühl, bestimmte Hardware und
reale Kontofähigkeiten brauchen zusätzlich geeignete Geräte bzw. autorisierte Konten.

## 6. Externe Feedly-Fragen separat führen

- [ ] **X1 — Öffentlicher Login:** Mit Feedly eigenen OAuth-Client und unterstützten
  Anmelde-/Refreshweg klären. PKCE nicht ohne bestätigte Unterstützung voraussetzen.
  Ohne Freigabe bleibt der persönliche Tokenmodus klar gekennzeichnet; keine öffentliche
  komfortable Anmeldung versprechen. Diese Klärung ist kein Grund, L/S/P/Q anzuhalten.
- [ ] **X2 — Reale Sync-Abnahme:** Mit autorisiertem Konto App ↔ Feedly-Web alle vier
  Statusrichtungen, Offlinephase, Neustart, verlorene Antwort, Tokenablauf, alten Saved-
  Artikel und entferntes Abo prüfen. Fehlenden Zugang als konkreten Blocker benennen.
- [ ] **X3 — Remote-Schreibfähigkeiten:** Abo-/Gruppenänderungen nur nach bestätigtem
  Endpoint-/Kontoverhalten freischalten, sonst klar als nicht unterstützt ausweisen.

## 7. Abschlusskriterien und ausdrücklich spätere Funktionen

**Lokaler Modus fertig:** L1–L7, gemeinsamer Produktumfang P1–P6 und relevante Q-Gates
bestehen ohne Feedly-Konfiguration; keine offene Datenverlust-/Sicherheitslücke.

**Sync technisch repariert:** S0–S6 bestehen gegen durchgängige Mocktests; persönliche
Live-Abnahme X2 bestätigt Verhalten. Ohne X2 lautet der Status „Mocktests bestanden,
Live-Abnahme offen“, nicht „vollständiger Feedly-Sync“.

**Öffentliche Gesamtversion fertig:** lokaler Modus und Sync abgenommen, öffentlicher
Zugangsweg geklärt, Paket/Upgrade, Accessibility und Performance nachgewiesen.

Nicht in diesen Auftrag ausweiten: Originalseiten-Volltextextraktion, Flatpak,
Hintergrunddienst, zusätzliche Anbieter, KI-Funktionen und Favicons. Vorhandene brauchbare
Implementierung weiterverwenden; kein pauschaler Neubau und keine Wiederholung bereits
belastbar bestandener Arbeiten ohne betroffene Änderung.
