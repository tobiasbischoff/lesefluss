# M1 — Status und Abnahme

**Stand:** 23.09.2026. Build: `cargo run --bin lesefluss-app` (App-ID
`io.github.PROJEKTINHABER.Lesefluss`).

## Umgesetzt

- **Drei Spalten** (Quellen 248 px, Artikel 336 px, Reader variabel) via zwei
  `AdwNavigationSplitView`; adaptive Schwellen 1119 px / 779 px.
  - Quellen: Smart-Ansichten (Ungelesen/Alle/Gespeichert) mit Zählern, Gruppen
    ein-/ausklappbar, Feeds mit Akzent-Punkt und Ungelesen-Badge, Footer mit
    Sync-Status.
  - Artikelliste: `GtkListView` + `SignalListItemFactory` (Zeilen-Recycling,
    vollständiger Reset + Signaltrennung beim Unbind), Tagesüberschriften
    (Heute/Gestern/Datum), Zeilen mit Quelle/Zeit/Titel/Auszug/64-px-Thumbnail,
    ungelesen = Semibold + Status-Icons (gespeichert/gelesen).
  - Reader: WebKit6-WebView (ephemere NetworkSession), eigenes Reader-Dokument
    mit Token-CSS (68 ch/760 px, 18 px/1,6), Kopf aus Titel/Quelle/Autor/Zeit,
    Leerzustand „Feedübersicht mit Icon + Zähler" (§4.1), Fehlerzustand mit
    „Erneut versuchen" bei Webprozess-Absturz.
- **Theme-Tokens** (§4.4): eine Quelldatei (`crates/reader/src/tokens.rs`) erzeugt
  GTK-CSS und Reader-CSS; Dunkel/Hell über `AdwStyleManager`, Sidebar-Verlauf
  dunkel als optionales Erkennungsmerkmal; Themewechsel rendert Reader neu und
  erhält die Leseposition (scrollY-Capture).
- **Actions & Tastatur** (§6, Teilmenge M1): j/k, n/p, m, s, o, Strg+R, Strg+N,
  Strg+Shift+M (Dialog mit Bereich + Anzahl), Strg+Z (Undo-Stack), Strg+F
  (WebKit-FindController), Strg++/-/0 (Typografie), F6/Shift+F6 (Bereichsfokus),
  Alt+← (zurück), Strg+W/Strg+Q. Alle Befehle als Gio-Actions mit Accels.
- **Gelesenstatus:** Auto-gelesen nach 800 ms sichtbar + aktivem Fenster
  (Generation-Counter, Fokusverlust verwirft), manueller Ungelesen-Schutz bis
  zum nächsten Öffnen; in der Ungelesen-Ansicht bleibt die gelesene Zeile bis
  zum Verlassen der Auswahl stehen (`keep_visible`), Zähler aktualisieren sofort.
- **Zustände:** leer (kein Artikel/keine Artikel), Fehler (Webprozess),
  Toasts für Kontoaktionen; Feed-hinzufügen-Dialog legt lokal an (Abruf = M2).

## Bekannte Abweichungen / offen in M1

- **AdwBreakpoint unzuverlässig:** Bei mehreren Breakpoints auf demselben Bin
  wertete libadwaita 1.9.3 nur das zuletzt hinzugefügte aus (reproduziert,
  Vertauschtest). Collapse wird deshalb event-driven über Surface-`width`-Notify
  + `fullscreened`/`maximized` mit denselben Schwellen gesteuert
  (`App::install_breakpoints`). Verhalten identisch, Mechanik dokumentiert
  abweichen.
- **Befehlspalette (Strg+K) und lokale Suche (Strg+L):** fehlen bewusst bis M2
  (brauchen Datenbank/FTS).
- **Buchstabenkürzel abschaltbar / umbelegbar:** Einstellung folgt in M6.
- **Drag-and-drop, Feed-Verwaltung per Kontextmenü:** M2/M3.
- Automatisierte UI-Tests: Tastatur-Simulation per `wtype` war in dieser Sitzung
  unzuverlässig (virtuelle Tastatur erreicht Fenster nur intermittent);
  Abnahme daher manuell + headless-Tests ab M2.

## Manuelle Abnahmeliste (bitte am laufenden Fenster prüfen)

1. Drei Spalten breit; Fenster schmaler ziehen → Quellen werden Overlay (<1120),
   darunter Einzelansicht Quellen→Artikel→Reader mit Zurück-Navigation.
2. Klick/Enter auf Artikel öffnet ihn; Pfeiltasten wechseln Auswahl mit
   Vorschau-Verzögerung; j/k, n/p, m, s wirken; Strg+Z macht Status rückgängig.
3. Ungelesen-Zähler sinken sofort; gelesene Zeile bleibt in „Ungelesen" bis zur
   nächsten Auswahl sichtbar.
4. Strg+F sucht im Artikel; Strg++/-/0 ändert Lesetypografie ohne Positionsverlust.
5. F6/Shift+F6 zyklisieren Fokus Quellen→Liste→Reader; Tab-Reihenfolge sinnvoll.
6. Dunkel/Hell (System-Theme wechseln) färbt UI und Reader konsistent um.
7. Link im Artikel klicken → öffnet Standardbrowser, nicht den WebView.
