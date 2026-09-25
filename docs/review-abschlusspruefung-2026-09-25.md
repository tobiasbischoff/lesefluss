# Abschlussprüfung vom 2026-09-25: noch nicht freigabefähig

Geprüft wurde der Arbeitsbaum auf `b9421ae`, einschließlich der noch nicht
committeten Reparaturen. Es gibt deutliche Verbesserungen, aber die Aussage
„alles erledigt“ ist nicht durch den produktiven Code gedeckt. Dieser Bericht
ergänzt `m7-offen.md` und ersetzt dessen Produkt- und Abnahmeanforderungen nicht.
Anwendungscode wurde bei dieser Prüfung nicht geändert.

## Nachweise und Grenzen

- `cargo test --workspace --locked`: **139 Tests bestanden**. Die lokalen
  HTTP-Mocks benötigten die Freigabe zum Öffnen von Testports.
- `cargo fmt --all -- --check` und `git diff --check`: bestanden.
- `cargo clippy --workspace --all-targets --locked`: bestanden, mit Warnungen.
  Der zusätzliche strengere Lauf mit `-- -D warnings` scheitert. Diese strengere
  Einstellung ist aktuell nicht im CI-Workflow verlangt.
- Separates Rust-Repro unter `/tmp/omareed-review3-repro.rs`: Queue-Fehler mit
  dem echten Coordinator und RefCell-Lebensdauer des Refresh-Ausdrucks bestätigt.
  Das ist kein GTK-End-to-End-Test.
- Keine Live-Abnahme mit Feedly, keine vollständige interaktive GTK/WebKit-Abnahme
  und kein tatsächlicher GitHub-Actions-Lauf. Keine pauschale Sicherheitsfreigabe.

## A1 · Hoch: manueller Refresh hält einen RefCell-Borrow fest

**Fundstelle:** `crates/app/src/window.rs:3379`, `run_token_action`, Zweig
`TokenAction::Refresh`; `request_feedly_sync` ab Zeile 3422.

Der `if let Some(account_id) = self.state.borrow()...`-Ausdruck hält den
unveränderlichen Borrow während des Zweigs. Der unmittelbar aufgerufene
`request_feedly_sync` versucht `borrow_mut()` auf demselben State: Panic
`RefCell already borrowed`, sobald ein Feedly-Konto vorhanden ist und der
Token-Callback den Refresh startet.

**Reparatur:** ID in einem eigenen Block in eine lokale Variable kopieren und
den Borrow beenden, bevor irgendeine Aktion aufgerufen wird. Weitere
`if let`-/`match`-Ausdrücke mit `state.borrow()` auf verschachtelte mutable
Zugriffe prüfen. Einen Test des tatsächlichen Action-/Callback-Pfades ergänzen.

## A2 · Hoch: vorgemerkter Sync startet nicht mehr

**Fundstellen:** `crates/sync/src/lib.rs:128` (`finish`),
`crates/app/src/window.rs:1156` (`FeedlySyncDone`), `:3392` (`QueuedSync`).

`finish()` setzt für den Folgelauf bereits `running = Some(next)`. Die UI
fordert denselben Lauf über `QueuedSync -> request_feedly_sync -> request()`
nochmals an. Ergebnis ist `Queued`, und es wird kein Netzwerk-Task gestartet.
Weitere Anforderungen bleiben hinter diesem nicht existierenden Lauf hängen.
Im Fehlerzweig wird das Ergebnis von `finish()` sogar verworfen; auch dort
kann ein als laufend markierter, tatsächlich nicht gestarteter Auftrag entstehen.

**Reparatur:** Reservierung und Start eindeutig trennen. Einen von `finish()`
zurückgegebenen Auftrag direkt ausführen, ohne ihn erneut anzufordern; alternativ
`finish()` darf noch keinen Lauf reservieren. Fehlerpfad ebenfalls behandeln.
Testfolge: Start A, Refresh während A, Abschluss A, genau ein tatsächlicher
Start B, Abschluss B, anschließend erneuter Start möglich; auch mit Fehler A.

## A3 · Hoch: Coordinator gilt nicht für alle Netzwerkwege

**Fundstellen:** `window.rs:3593` (`start_feedly`), `:3642` (Outbox-Tick),
`:2769` (`mark_scope_server_with`), `:3546` (Abmelden),
`crates/app/src/net.rs:41` (Sync-Ereignisse).

Initial-Sync, Outbox und serverseitiges Markieren starten direkt, ohne
`coordinator.request`. Auth-/Quota-Pausen und `block()` stoppen diese Wege
nicht. `block()` bricht bereits laufende Tasks ebenfalls nicht ab. Alle
Ereignisse enthalten weder Konto- noch Laufkennung; ein Outbox-Fehler kann
dadurch einen parallel laufenden Delta-Sync in der UI als beendet behandeln.
Im Outbox-Gruppenloop wird nach 401/403/429 außerdem mit weiteren Gruppen
fortgefahren. Pull-429 verliert beim Umwandeln in `StorageError::Schema` die
strukturierte Retry-Zeit; die UI setzt ohne diese Zeit keine Quotenpause.

**Reparatur:** Alle vier Wege über denselben Dispatcher führen, mit
kontogebundener Laufkennung und notwendigen Job-Parametern. Genau ein
Abschlussereignis pro Lauf; veraltete Ergebnisse ignorieren. Abmelden muss
laufende Arbeit abbrechen oder deren weitere Requests/DB-Writes zuverlässig
invalidieren. Auth/Quota beendet den aktuellen Versand und sperrt alle Wege.
HTTP-Status und Retry-Zeit typisiert bis zur Steuerung erhalten.

**Abnahme:** Überlappung von Refresh, Timer, Initial, Outbox und Serveraktion;
401/429 zwischen zwei Outbox-Gruppen; Logout während Request und erneuter Login.

## A4 · Hoch: vollständiger Leseabgleich existiert nur in Hilfsfunktionen

**Fundstellen:** `feedly_sync.rs:493` (`reconcile_unread`), `:545`
(`load_missing_contents`), `:583`/`:880` (produktive Sync-Einstiege),
`:257` (`ingest_entries`).

`reconcile_unread` und `load_missing_contents` werden außerhalb ihrer Tests
nicht aufgerufen. Initial/Delta laden keinen vollständigen Ungelesen-Bestand
und laden ältere Saved-/Unread-Inhalte nicht über diese Funktionen nach.
Zusätzlich setzt `reconcile_unread` nur lokal ungelesene, remote fehlende
Artikel auf gelesen; die Gegenrichtung fehlt. `ingest_entries` übernimmt
`unread=true` bei bestehenden Artikeln ebenfalls nicht als Statusänderung.
`load_missing_contents` fragt nur bereits vorhandene DB-Artikel ohne Inhalt ab;
vollständig unbekannte IDs aus Saved-/Unread-Mengen werden nicht entdeckt.
Einträge unbekannter Origins werden beim Import übersprungen.

**Reparatur:** Einen tatsächlich verwendeten Sync-Ablauf bauen: vollständige
Statusinventare lesen, beide Statusrichtungen revisionsgeschützt abgleichen,
unbekannte und inhaltslose IDs bestimmen, per `.mget` in Batches nachladen und
auch Origins nicht abonnierter gespeicherter Artikel darstellen. Vollständigkeit
pro Phase führen. Abos/Gruppen im Delta aktualisieren; dieser Leseabgleich ist
nicht durch die externe Freigabe für Remote-Schreiboperationen blockiert.

**Abnahme:** Alle vier Statusrichtungen, alter gespeicherter Artikel außerhalb
des Inhaltsfensters, unbekannte Origin, neues/entferntes Abo – jeweils durch
den echten Initial-/Delta-Einstieg mit injizierbarem Client.

## A5 · Hoch: Nachbestätigung kann neuere Benutzerabsicht überschreiben

**Fundstelle:** `feedly_sync.rs:767–785`, Erfolgszweig von `process_outbox`.

Nach ACK und anschließendem `.mget` werden Abweichungen mit
`enqueue_outbox(..., desired)` neu eingereiht. Dabei fehlt die Prüfung gegen
die gesendete Revision. Beispiel: Read=true wurde gesendet; während der
Bestätigungsabfrage setzt der Nutzer Read=false. Liefert `.mget` noch false,
reiht der alte Callback true erneut ein und überschreibt die neuere Absicht.
Revisionssicheres `outbox_ack` allein verhindert das nicht. Ein Fehler oder
eine unvollständige Antwort der Bestätigungsabfrage wird zudem still übergangen.

**Reparatur:** Nachbestätigung an dieselbe gesendete Revision/Generation binden.
Atomar nur dann erneut einreihen, wenn seitdem keine neuere Benutzeränderung
erfolgte – auch wenn die ursprüngliche Outbox-Zeile bereits gelöscht wurde.
Fehlende IDs und Abfragefehler ausdrücklich als unbestätigt behandeln.
Regressionstest mit verzögerter `.mget`-Antwort und zwischenzeitlichem Toggle.

## A6 · Mittel: unvollständiger Sync wird als erfolgreich gespeichert

**Fundstellen:** `feedly_sync.rs:337` (`fetch_id_inventory`), `:424`
(`reconcile_saved`), `:583`/`:880` (Initial/Delta).

Bei Cursorzyklus/Seitenlimit liefert das ID-Inventar `Incomplete`.
`reconcile_saved` protokolliert das und gibt `Ok(())` zurück. Beide Sync-Einstiege
setzen danach trotzdem `last_sync` und senden `FeedlySyncDone`. Ein sicherer
Zeitstempel allein beweist keine abgeschlossene Statusphase. Beim Initial-Sync
bleibt `status_account` außerdem ein leerer String; der abschließende
Kontostatus wird nicht auf das tatsächlich angelegte Konto geschrieben.

**Reparatur:** Unvollständigkeit bis zum Zyklusergebnis transportieren; keine
vollständige Erfolgsmeldung/Watermark für fehlgeschlagene Pflichtphasen. Falls
Phasen getrennte Watermarks bekommen, diese explizit modellieren. Die aus
`profile()` erhaltene Konto-ID für alle Statusupdates und Events verwenden.

## A7 · Hoch: Tokenbindung wird nicht geprüft; Keyring-Erfolg falsch erkannt

**Fundstellen:** `feedly_sync.rs:58–123`, `:147`, `:591`;
`window.rs:3349` und Outbox-Tick.

`keyring_account()` hat keinen Aufrufer. Token und aktives Konto werden getrennt
gewählt; die Outbox nimmt das erste Feedly-Konto aus der Liste. Nach einem
Kontowechsel kann daher ein Token des neuen Profils mit der Outbox des alten
Profils kombiniert werden. Eine gespeicherte Kontobindung ohne Prüfung schützt
davor nicht.

`keyring_store` und `bind_account` verwenden außerdem `secret_tool_text`.
Dieser Helper wertet einen erfolgreichen Prozess mit leerem stdout als
Fehlschlag. Ein erfolgreicher `secret-tool store` braucht aber keinen Textwert
auszugeben. Dadurch wird der Dateifallback auch nach erfolgreichem Speichern
benutzt, und Bindungserfolg wird falsch gemeldet.

**Reparatur:** Lookup-Ergebnis und Command-Erfolg getrennt auswerten. Erst Profil
validieren, dann Token atomar dem Konto zuordnen; beim Versand Identität prüfen,
auch im Dateifallback. Explizites aktives Konto statt `find(kind == feedly)`.
Tests für Exit 0 ohne stdout, Kontowechsel A→B mit wartender Outbox A und
gesperrten Keyring. Das lässt sich ohne öffentliche OAuth-Freigabe testen.

## A8 · Mittel: lokaler manueller Refresh wird durch Feedly verdrängt

**Fundstelle:** `window.rs:3294`, `do_refresh`.

Sobald ein Feedly-Konto existiert, gilt jeder Scope außer einem expliziten
anderen Account als Feedly-Scope. Ein ausgewählter lokaler Feed/eine lokale
Gruppe startet daher Feedly und kehrt vor den lokalen Fetches zurück.
Auch Global aktualisiert auf diesem Weg keine lokalen Feeds.

**Reparatur:** Provider anhand der tatsächlich im Scope liegenden Feeds
ermitteln. Lokale Fetches und nötigen Feedly-Sync getrennt anfordern, bei
Global beide. Tests mit lokaler Gruppe, lokalem Feed, lokalem Konto und Global
bei gleichzeitig vorhandenem Feedly-Konto.

## A9 · Mittel: CI und ausgewiesene Testnachweise noch fehlerhaft

**Fundstellen:** `.github/workflows/ci.yml:30` und `:46`;
`crates/provider-feedly/src/lib.rs:607–639`.

- Der Workflow prüft `webkit6gtk-4.1`; benötigt wird `webkitgtk-6.0`.
  Lokal existiert letzteres (2.52.6), der erste Prüfaufruf scheitert.
- Der Arch-Container hat keinen eingerichteten unprivilegierten Buildbenutzer;
  `makepkg` darf nicht als root laufen. Paketbau unter eigenem Benutzer mit
  passenden Besitzrechten und explizitem Ausgabeverzeichnis einrichten.
- Vier vermeintliche Tests haben kein `#[test]`:
  `pager_stops_on_cursor_cycles`, `pager_reports_safety_limit_as_incomplete`,
  `pager_without_any_page_is_not_success`, `empty_json_is_not_a_valid_inventory`.
  Sie werden nicht ausgeführt; Warnungen weisen darauf hin. Das Abnahmeprotokoll
  darf sie nicht als bestandene Testnachweise führen.

**Reparatur:** Workflow korrigieren und tatsächlich ausführen; die vier Tests
aktivieren, Ergebnisse prüfen, produktive Sync-Einstiege injizierbar machen.
`m7-offen.md` S2/S4/S5 und `abnahme-protokoll.md` an die tatsächliche Verdrahtung
anpassen. Hilfsfunktionstest ist kein Nachweis einer fertigen App-Funktion.

## Empfohlene Arbeitsreihenfolge

1. A1/A2 beheben und Action-/Dispatcher-Regressionen ergänzen.
2. A3/A7 als gemeinsame Konto-/Laufsteuerung reparieren, danach A5 absichern.
3. A4/A6 in einen vollständigen, mockbaren Sync-Zyklus integrieren.
4. A8 und A9 beheben; Abnahme- und Restarbeitsdokumente korrigieren.
5. Noch offene L-/P-/Q-Punkte aus `m7-offen.md` tatsächlich abarbeiten oder
   ausdrücklich als nicht fertig ausweisen. Insbesondere WebKit-Laufzeit,
   OPML/Verwaltung, Lokalisierung, Accessibility und Performance fehlen weiter.
6. Erst danach Gesamtprüfung und Git-Veröffentlichung als fertiger Stand.

## Git-Befund und Übergabe

Vor diesem Bericht: **37 geänderte versionierte Dateien**, dazu die unversionierte
`docs/review-nachpruefung-2026-09-25.md`. Index leer, HEAD unverändert `b9421ae`,
Branch `main`, kein Upstream und **kein Remote** eingerichtet. Die Reparaturen
sind somit weder staged noch committed; ein Push ist in dieser Konfiguration
nicht möglich. Dieser neue Bericht kommt als weitere unversionierte Datei hinzu.

Begrenzter Mustercheck der versionierbaren Dateien: keine Treffer für die
geprüften Private-Key-/GitHub-/AWS-Schlüsselmuster und keine auffälligen
DB-/Token-/Logdateien. Das ist keine umfassende Secret-Prüfung.

Ein Zwischenstandscommit ist möglich, sollte aber die offenen Fehler ehrlich
benennen. Kein pauschales `git add .`: Änderungen nach Themen sichten/stagen,
beide Reviewdateien bewusst aufnehmen, `git diff --cached --check` und
`git diff --cached` prüfen, dann committen. Für einen Push muss zuerst das
beabsichtigte Remote-Ziel bekannt und eingerichtet sein. Bei dieser Prüfung
wurden weder Index noch Commits noch Remotes verändert und nichts gepusht.
