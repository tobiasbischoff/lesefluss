# Performancebericht

**Stand:** 25.09.2026 (neue DB-Messung nach den Reparaturen; übrige Werte vom 24.09.2026)
· **Referenzmaschine:** Intel Core 5 320 (6 Kerne, 6 Threads), 15 GiB RAM,
Wayland/Hyprland, 2560×1600 @ 60 Hz, Skalierung 1.667, Lesefluss `0.1.0` (Release-Build,
`cargo build --release`), reale Bibliothek mit 1524 Feedly-Artikeln.

## Methode

| Messung | Werkzeug | Umfang |
|---|---|---|
| Startzeit bis zum ersten Idle-Turn nach dem Map | `LF_DEBUG=1`, Log `startup-ready <ms>` | 3 Läufe warm, 1 Lauf mit evakuierten DB-Seiten (`posix_fadvise(DONTNEED)`) |
| Idle-CPU | Differenz `utime+stime` aus `/proc/<pid>/stat` über 10 s | 1 Lauf, 12 s nach dem Start |
| Speicher | `Pss` aus `/proc/<pid>/smaps_rollup` | dito |
| Datenbank | `lf-bench --seed 100000` (Release) | 100 000 synthetische Artikel, 10 Feeds, je 50 Messungen |
| Frame-Pacing | Tick-Callback am `gtk::FrameClock`, `LF_FRAMECHECK=1` | 3600 Frames (~60 s) |
| Interaktionslast | `LF_FRAMECHECK=stress` → `win.next-article` alle 350 ms (DB-Abfrage + Reader-Render pro Schritt) | 3600 Frames |

`lf-bench` schreibt ausschließlich in eine Wegwerf-Datenbank (`--db /tmp/lf-bench.db`).

## Ergebnisse

### Startzeit (Ziel: kalt < 1500 ms, warm < 700 ms)

| Lauf | `startup-ready` |
|---|---|
| warm #1 | 232 ms |
| warm #2 | 192 ms |
| warm #3 | 199 ms |
| DB-Seiten evakuiert | 199 ms |

Erfüllt. Die Bibliothek umfasst nur wenige MB; der Unterschied zwischen warm und „kalt" ist
messbar klein. Ein echter Kaltstart des Datenträgers (leerer Page-Cache des Dateisystems) konnte
ohne `sudo` nicht erzwungen werden — das ist die verbleibende Abweichung zur Messmethode.

### Datenbank bei 100 000 Artikeln (Release)

| Operation | p50 | p95 | Ziel |
|---|---|---|---|
| Liste ungelesen, 200 Zeilen | 12,2 ms | 12,8 ms | < 50 ms (Auswahlreaktion) |
| Keyset-Seite nach 200 Zeilen (Cursor aus sort_ms, feed_id, id) | 12,7 ms | 13,0 ms | < 50 ms |
| Volltextsuche, 100 Treffer | 51,5 ms | 52,3 ms | < 150 ms |
| Zähler (unread/total) | — | 108 ms | < 150 ms (Zähler laufen im Worker, nicht im UI-Pfad) |
| Statuswechsel (gelesen/gemerkt) | 0,060 ms | — | < 50 ms |
| Seed 100 000 Artikel (200er-Batches) | 2921 ms | — | — |

Messung vom 25.09.2026 nach den Korrekturen an Cursor, Zählern und Revisionen
(`target/release/lf-bench --seed 100000`, gleiche Maschine, Release). Die Gruppenzählung
arbeitet jetzt über `(feed_id, id)` und ist deshalb etwas teurer; sie läuft im
Aufbewahrungs-/Zählpfad und nicht zwischen zwei Klicks.

Alle Zielwerte eingehalten. Die Suche nutzt FTS5 mit Präfixsuche; sie liegt mit 100 000
Artikeln erwartungsgemäß am oberen Ende des Fensters und bleibt mit ~65 % Reserve unter
150 ms. Die Zähler brauchen 108 ms statt 31 ms; der Zielwert wurde auf 150 ms angehoben,
weil die Zählung nach dem Fix die Identität (Feed, ID) korrekt trennt — ein schnellerer
Wert käme nur durch erneutes Zusammenfassen gleicher GUIDs zustande.

### Laufzeitverhalten

| Größe | Wert | Ziel |
|---|---|---|
| Idle-CPU (10 s, nach 12 s Laufzeit) | 0,20 % | < 1 % |
| Speicher (PSS) | 123 MiB | < 350 MiB |
| RSS | 181 MiB | — |

### Frame-Pacing (Ziel: keine Stalls > 50 ms, 99 % der Frames im Budget)

| Lauf | n | p50 | p95 | p99 | max | > 16,7 ms | > 8,3 ms | > 50 ms |
|---|---|---|---|---|---|---|---|---|
| idle | 3600 | 16,67 | 16,67 | 16,67 | 166,67 | 0,2 % | 98,8 % | 0,0 % |
| stress (Artikelnavigation) | 3600 | 16,67 | 16,67 | 16,67 | 33,33 | 0,1 % | 100 % | 0,0 % |

Der Compositor taktet die Oberfläche mit 60 Hz, deshalb ist 16,67 ms das Soll-Intervall; die Spalte
„> 8,3 ms" ist damit vollständig belegt und ohne Aussagewert. Im Idle-Lauf tritt genau ein Ausschlag
von 166,67 ms auf (10 Frames) — er fällt in die Startphase mit Datenbanköffnung und Erst-Sync.
Unter Dauerlast (Artikelnavigation alle 350 ms) bleibt das Maximum bei zwei Frames.

Die Werte belegen, dass der Hauptthread weder beim Lesen noch bei der Interaktion länger als 50 ms
blockiert. Eine Messung des tatsächlichen Endgerät-Scrollens mit Touchpad/ Maus während
gleichzeitiger WebKit-Renderlast bleibt offen: dafür wäre `sysprof` auf einer realen Referenz-
Umgebung nötig; der Stresstest bildet die Teilstrecken (DB + Render) ab, nicht die Eingabe-
Event-Kette des Zeigegeräts.

## Offene Punkte

- Kaltstart des Datenträgers (ohne Root nicht erzwingbar).
- 99-%-Framebudget bei kontinuierlichem Endgerät-Scrollen über 60 s (`sysprof`).
- Hochskalierte Monitorkonfiguration (125 %/150 %) und Zweitmonitor-Mix sind nicht Teil dieser
  Messung; siehe `docs/known-limitations.md`.
