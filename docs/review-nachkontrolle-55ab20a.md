# Nachkontrolle auf 55ab20a – 25.09.2026

**Ergebnis (Stand 25.09.2026, Commits `087de81` bis `1595e0c`):** Die Befunde
B1–B7 sind je einzeln behoben und mit Regressionstests über die produktiven
Einstiege belegt; `cargo test --workspace` ist mit **177 Tests** grün, Formatierung
und strenges Clippy bestehen, der Release-Build ist grün. Eine technische
Abschlussfreigabe bleibt dennoch aus: Live-Feedly (X2), öffentliche OAuth-Anmeldung
(X1), ein echter GitHub-Actions-Lauf sowie die Geräte-/GTK-Abnahme wurden nicht
durchgeführt. Die pauschale Aussage „A1–A9 behoben“ in `m7-offen.md` und
`abnahme-protokoll.md` wurde deshalb durch diese Einzelpunkte ersetzt.

Geprüft wurde ursprünglich der saubere Arbeitsbaum auf `55ab20a`; die Beseitigung
der Befunde ist nachfolgend in sieben Commits erfolgt. Dieser Bericht ergänzt
die bisherige Abschlussprüfung.

## Stand der Befunde

| Befund | Stand | Umsetzung | Beleg |
|---|---|---|---|
| B1 Outbox-Abschluss | behoben | `process_outbox` ist ein Future mit genau einem Abschluss (leerer Claim/Fehler eingeschlossen), Claims werden revisionssicher freigegeben, `.mget`-Fehler typisiert, 401/403/429 stoppen | `outbox_single_end_tests` (3 Tests), `087de81` |
| B2 Reservierte Läufe | behoben | `start_feedly_run` registriert den reservierten Lauf; `finish_run` prüft Identität, Pause und `finish()` genau einmal | `dispatcher_kette_vom_erfolg_bis_zum_wiederabbruch`, `e506fe3` |
| B3 Abbruch in allen Phasen | behoben | `ctx.is_cancelled()` in Delta-Entry, Reads, Inhalten, Saved, Unread, Watermark, Nachladen, Initial und Serveraktionen; Abbruch ist `cancelled()`-SyncFailure ohne Watermark/`ready` | `cancel_tests`, `3c38ba3` |
| B4 Credential-Bindung | behoben | Credential aus Token **und** Konto-ID in Keyring/Datei; `token_matches_account(None, _) == false`; aktives Konto in `UiState`; Outbox-Tick läuft über `with_token`; `token_from_disk` (toter Doppelpfad) entfernt | `dateifallback_haelt_die_kontobindung_fest`, `initial_sync_bindet_das_validierte_profilkonto`, `4baefed` |
| B5 Erst-Sync-Vollständigkeit | behoben | Initial wertet `saved`/`unread` mit `require_complete` aus; Watermark, Status und Ereignis tragen die validierte Profil-ID statt des Dispatcher-Ankers | `unvollstaendige_statusphase_beendet_auch_den_erstsync`, `initial_sync_bindet_das_validierte_profilkonto`, `4baefed` |
| B6 Metadatenabgleich | behoben | `sync_subscriptions` läuft im Delta-Zyklus; Test prüft neue Quelle aktiv, entfernte deaktiviert, gespeicherte Artikel erhalten | `delta_gleicht_abos_und_gruppen_ab`, `4baefed` |
| B7 Layoutwiederherstellung | behoben | `read_layout` liefert `Result<Option<String>, String>` statt Still-Schlucken per Downcast; kein Hilfs-Thread mit `.join()` mehr; `apply_layout` protokolliert Fehler | `layout_lesen_liefert_wert_oder_nennt_den_fehler`, `read_layout_ohne_worker_meldet_fehler`, `1595e0c` |

Nicht abgedeckt sind weiterhin die Live-Punkte X1/X2, der GitHub-CI-Lauf und die
in `m7-offen.md` gelisteten Produkt-/Laufzeitpunkte (P1, P4, P5, P6, Q3).

## Was jetzt nachgewiesen ist

- `cargo test --workspace --locked`: **164 Tests bestanden** (mit Freigabe für
  lokale HTTP-Testports; die Sandbox allein blockiert diese Ports).
- `cargo fmt --all -- --check`: bestanden.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: bestanden.
- `git diff --check b9421ae..HEAD`: bestanden.
- Der direkte RefCell-Fehler des manuellen Refreshs wurde behoben; lokaler
  Refresh wird nun über einen separaten Plan angestoßen. Der Keyring-Helper
  unterscheidet Exit-Status und Lookup-Ausgabe. Die vier zuvor nicht aktivierten
  Provider-Tests sind aktiviert.
- Leseabgleich und Nachladen werden jetzt im produktiven Delta-Sync aufgerufen.
  Die verzögerte Outbox-Nachbestätigung besitzt jetzt eine Revisionsprüfung.
  Diese Fortschritte beseitigen nicht die unten beschriebenen Ablaufprobleme.

Zusätzliche isolierte Reproduktionen verwenden die **unveränderten** Module
`feedly_sync.rs` und `dbworker.rs` direkt per `#[path]`. Nur der Netzwerk-Runtime-
und Ereignisadapter ist ein kleiner Ersatz für das GTK-unabhängige Testharness.
Er wartet auf das tatsächliche Ende des gestarteten Futures. Kein Live-Feedly,
kein echter Schlüsselbund und keine produktive Bibliothek werden verwendet.
Harness: `/tmp/omareed-review4-harness`; Ausgaben:
`/tmp/omareed-review4-repro-results.log`, `/tmp/omareed-review4-layout-repro.log`.
Die Repros bestätigen absichtlich das fehlerhafte Ist-Verhalten; daraus müssen
für die Reparatur Tests mit dem korrekten Soll-Verhalten entstehen.

## B1 · Hoch · Outbox beendet den Coordinator-Lauf nicht zuverlässig

**Fundstelle:** `crates/app/src/feedly_sync.rs:1126`, `process_outbox`.

Nach erfolgreichem Upload und erfolgreicher `.mget`-Bestätigung endet der Future
ohne `FeedlySyncDone`. Auch eine leere Outbox und ein Fehler beim Claim kehren
ohne Abschluss zurück. Der Coordinator bleibt dadurch auf `Outbox` stehen;
spätere Refreshs werden nur vorgemerkt. Das ist mit erfolgreichem Upload und
Bestätigung gegen einen Mockserver reproduziert: zwei Requests, Future beendet,
Ereigniskanal leer.

Umgekehrt schicken mehrere Gruppen jeweils `FeedlySyncFailed`, obwohl danach
noch weitere Requests laufen können. Der erste Fehler räumt den aktiven
UI-Lauf ab; weitere Ereignisse werden als veraltet verworfen. Ein 401/429 aus
der Bestätigungsabfrage wird außerdem pauschal zu `degraded` und stoppt den
Versand nicht.

Alle Zeilen werden vorab als `inflight` geclaimt. Ein Abbruch/401/429 setzt nur
die gerade behandelte Gruppe zurück; noch nicht gesendete Gruppen bleiben
`inflight`, bis ein gesonderter Reset stattfindet.

**Implementierungsauftrag:** Outbox als einen Future mit genau einem
abschließenden Ergebnis modellieren: Erfolg, Fehler oder Abbruch, einschließlich
leerem Claim. Gruppenmeldungen dürfen den Lauf nicht vorzeitig beenden.
Nicht versendete Claims bei jedem vorzeitigen Ende revisionssicher freigeben;
HTTP-Steuerungsfehler auch aus `.mget` typisiert behandeln und Versand stoppen.

**Abnahme:** echte Outbox-Einstiege für leer, vollständig bestätigt, Claim-Fehler,
gemischte Gruppen, 429/401 beim Upload und bei `.mget`; genau ein Abschluss,
keine verwaisten Claims, anschließender Refresh startet tatsächlich.

## B2 · Hoch · Folgeläufe werden gestartet, aber nicht als aktiv registriert

**Fundstellen:** `window.rs:1358–1373`, `:3499–3514`, `:3612–3637`;
Fehlerpfad `:1409` und `:1430`.

Der Erfolgszweig löscht `active_feedly_run`, reserviert den nächsten Auftrag und
startet ihn über `ReservedRun`. `start_feedly_run` erzeugt dann zwar einen neuen
`RunCtx`, speichert ihn aber nicht in `active_feedly_run`. Dessen einzige
Zuweisung erfolgt bei `Decision::Start` in `request_feedly_job` – diesen Weg
umgeht der reservierte Lauf absichtlich. Seine Abschlussereignisse werden somit
von `feedly_event_is_current` verworfen. Auch sein Cancel-Handle fehlt in der UI.

Der Fehlerzweig ruft `finish()` zweimal auf: der erste Aufruf reserviert den
Folgelauf und verwirft das Ergebnis; der zweite beendet diese Reservierung
wieder. Der vorgemerkte Auftrag geht verloren.

**Implementierungsauftrag:** Einen gemeinsamen Startpunkt für normale und
reservierte Läufe schaffen, der Konto, Laufkennung, Cancel-Handle und Running-
Zustand vor dem Spawn setzt. `finish` pro Abschluss genau einmal aufrufen,
Pausen vorher berücksichtigen und dessen Ergebnis direkt ausführen. Fehlendes
Token beim reservierten Start muss die Reservierung ebenfalls auflösen.

**Abnahme:** Mit echtem Dispatcher/Event-Handler: A starten, B vormerken,
A erfolgreich/fehlerhaft beenden, B starten und beenden, C starten. Zusätzlich
B während der Ausführung abmelden. Ein isolierter Coordinator-Test reicht nicht.

## B3 · Hoch · Abbruchsignal verhindert Requests und DB-Writes nicht

**Fundstellen:** `feedly_sync.rs:1356` (`delta_sync`), `:860`
(`load_contents`), `:972` (Initial), `:1079` (Serveraktion).

Delta prüft den Abbruch weder am Einstieg noch in seinen Pull-/Reconcile-Phasen.
Schon ein **vor dem Start** gesetztes Cancel-Flag führt weiterhin zu Requests,
Watermark und Erfolgsmeldung. Das wurde gegen einen Mockserver reproduziert.
`load_contents` bricht nur die Schleife ab und gibt Erfolg zurück, sodass der
Aufrufer danach weiterhin eine Watermark schreibt. Initial prüft erst nach dem
Profil-DB-Write und anschließend lange nicht mehr; die Serveraktion prüft nur
vor dem HTTP-Request, nicht vor dem DB-Write nach dessen Antwort.

Veraltete UI-Ereignisse zu ignorieren verhindert diese Auswirkungen nicht.

**Implementierungsauftrag:** Abbruch als explizites Ergebnis durch alle Phasen
führen. Vor Requests und insbesondere im DB-Auftrag vor Mutationen die gültige
Konto-/Laufgeneration prüfen. Nach HTTP-Antworten erneut prüfen. Abgebrochene
Phasen dürfen weder Watermark noch `ready` setzen. B2 muss denselben Cancel-
Handle im Dispatcher und im Worker verwenden.

**Abnahme:** Vorab abgebrochener Lauf erzeugt null Requests und null Writes;
Logout während verzögerter Pull-/Marker-Antwort lässt anschließend keine
DB-Änderungen und keine weiteren Requests dieses Laufs zu.

## B4 · Hoch · Tokenbindung bleibt im produktiven Ablauf unvollständig

**Fundstellen:** `feedly_sync.rs:238`, `:293`, `:972`;
`window.rs:3440`, `:3724`, `:3850–3880`, `:2109`.

Der Dialog ruft ausschließlich `save_token(&token, None)` auf. Initial liest
zwar das Profil, bindet das Token anschließend aber nicht. Der Kommentar
„wird beim ersten Sync gebunden“ ist deshalb falsch. Bei fehlender Bindung
liefert `token_matches_account(None, account)` sogar `true`.
Der Outbox-Tick liest das Token und startet direkt `request_feedly_job`, ohne
die Prüfung in `with_token` zu durchlaufen. `account_id_of_kind` bleibt ein
Wrapper um `first_account_of_kind`, keine explizite Wahl des aktiven Kontos.

Bei einem Kontowechsel kann weiterhin das neue Token mit alten Kontodaten bzw.
deren Outbox kombiniert werden. Die neue Prüfung sichert diesen Weg nicht ab.

**Implementierungsauftrag:** Token erst nach Profilvalidierung als zusammen-
gehöriges Credential aus Token und Konto-ID aktivieren. Bindung auch für den
Dateifallback persistieren; fehlende Bindung vor Versand validieren statt
freigeben. Sämtliche Startwege einschließlich Outbox müssen dasselbe gebundene
Credential erhalten. Aktives Konto explizit verwalten. Neue Anmeldung und
Tokenwechsel dürfen alte Outbox-Aufträge nicht mit neuen Credentials starten.

**Abnahme:** frisches Profil, Keyring/Dateifallback, A→B mit wartender Outbox A,
fehlende/veraltete Bindung und Timer gleichzeitig mit Tokenwechsel; den echten
Login-/Tick-Pfad testen, nicht nur den Vergleich zweier Strings.

## B5 · Hoch · Initial-Sync überspringt Vollständigkeitsprüfung

**Fundstellen:** `feedly_sync.rs:1022–1049`, Abschluss `:1054–1074`;
Kontoanker `window.rs:3810–3829`.

Delta wertet die `PhaseResult`s mit `require_complete` aus. Initial verwirft
beide Rückgabewerte von `reconcile_saved` und `reconcile_unread`. Bei
Cursorzyklus in beiden Inventaren meldet Initial trotzdem Erfolg und schreibt
`last_sync`. Exakt dieser Fall wurde reproduziert. Der vorhandene A6-Test
deckt nur Delta ab.

Zusätzlich verwenden die abschließenden Statusupdates `ctx.account_id`, nicht
die validierte Profil-ID. Beim ersten Verbinden ist der Context-Anker regelmäßig
das erste vorhandene Konto, z. B. `local`; Watermark und Status landen dann auf
unterschiedlichen Konten. Das wurde durch Lesen der Aufrufkette festgestellt.

**Implementierungsauftrag:** Pflichtphasen beider Einstiege identisch auswerten.
Profilkonto und temporäre Dispatcher-Kennung sauber unterscheiden bzw. nach
Profilvalidierung korrekt zuordnen. Status, Watermark, aktives Konto und
Ereignisidentität müssen anschließend konsistent sein.

**Abnahme:** Initial und Delta jeweils mit vollständigem, begrenztem und
zyklischem Inventar; frischer Login bei ausschließlich lokalem Ausgangskonto.

## B6 · Mittel · Abo-/Gruppenabgleich fehlt weiterhin im Delta

**Fundstellen:** `feedly_sync.rs:914`, `:999`, `:1356`.

`sync_subscriptions` hat entgegen seinem Kommentar nur einen produktiven
Aufrufer: Initial. Delta aktualisiert weder Abos noch Gruppen. Neue Remote-
Quellen können daher beim Artikelimport als inaktive unbekannte Origin
entstehen; entfernte Abos bleiben aktiv. Der vorhandene Delta-Mock bietet
Antworten für Subscriptions/Categories an, prüft aber nicht deren Abruf.

**Implementierungsauftrag:** Den vollständigen Metadatenabgleich in den
regulären Zyklus integrieren; Aktivierung, Entfernung, Umbenennung und
Gruppenzuordnungen konsistent behandeln. Remote-Leseabgleich nicht als von
der Freigabe für Remote-Schreiboperationen abhängig markieren.

**Abnahme:** Delta mit hinzugefügtem und entferntem Abo sowie geänderter Gruppe;
Requests und daraus folgende DB-Änderungen explizit prüfen.

## B7 · Mittel · gespeicherte Fenstergröße wird nie wiederhergestellt

**Fundstellen:** `crates/app/src/dbworker.rs:64–73`, `window.rs:1886`.

`read_layout` sendet einen Job mit Rückgabetyp `Option<String>`
(`db.get_pref(...).ok().flatten()`), versucht danach aber einen Downcast auf
`storage::Result<Option<String>>`. Der Downcast scheitert immer und liefert
`None`, selbst bei gespeichertem gültigem Layout. Das ist isoliert mit
gespeichertem `1200;900` reproduzierbar. Außerdem wartet der aufrufende Thread
über `.join()` trotz des zusätzlich gestarteten Threads synchron.

**Implementierungsauftrag:** Den tatsächlichen Rückgabetyp konsistent halten,
vorzugsweise den vorhandenen asynchronen DB-Antwortpfad verwenden. Regression:
Layout speichern, mit `read_layout` lesen und denselben Wert erhalten; bei
Neustart sichtbare Fenstergröße prüfen.

## Reihenfolge und Abschlusskriterien

1. B1/B2 zusammen reparieren: ein tatsächlicher Auftrag, ein aktiver Kontext,
   ein Abschluss; Tests über den Dispatcher und Ereignisverbraucher.
2. B3/B4/B5: Abbruch, Credentials und Profilzuordnung gemeinsam absichern.
3. B6/B7 beheben; bestehende Tests um die fehlenden produktiven Pfade erweitern.
4. Statusdokumente korrigieren: keine pauschalen Häkchen allein aus Helper-
   Tests; Initial, Delta, Outbox, UI-Dispatcher und Login getrennt nachweisen.
5. Danach weiterhin offene Produkt-/Laufzeitpunkte aus `m7-offen.md` abnehmen.
   Live-Feedly, GTK/WebKit-Gesamtprüfung und ein echter CI-Lauf wurden in dieser
   Nachkontrolle nicht durchgeführt. Release/Paketbau wurde nicht erneut geprüft.

## Git

Vor Erstellung dieses Berichts war der Arbeitsbaum **sauber**, einschließlich
Index. Die bisherigen Reparaturen liegen in zwölf Commits seit `b9421ae` vor;
HEAD ist `55ab20a`, Branch `main`. Es gibt weiterhin **kein Remote und keinen
Upstream**. Damit sind die Änderungen diesmal ordentlich committed, aber ein
Push ist in dieser Konfiguration nicht möglich.

Dieser Bericht ist die einzige neue unversionierte Repo-Datei aus der Prüfung.
Keine Anwendungscode-, Index-, Commit- oder Remote-Änderungen vorgenommen.
