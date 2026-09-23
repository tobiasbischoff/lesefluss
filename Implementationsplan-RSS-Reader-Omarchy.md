# Native RSS-App für Omarchy: Produkt- und Implementationsspezifikation

**Arbeitsname:** Lesefluss — vor Veröffentlichung auf Namenskollisionen prüfen.  
**Stand:** 23. September 2026.  
**Zielplattform:** Omarchy Linux auf Arch-Basis, Hyprland, native Wayland-Sitzung.  
**Adressat:** Der Agent, der die Anwendung anschließend implementiert.  
**Status:** Ausschließlich Planung und Spezifikation. Dieses Dokument enthält keine fertige App und keine Behauptung, dass technische Prototypen oder Feedly-Verbindungen bereits getestet wurden.

## 1. Auftrag und Verbindlichkeit

Entwickle einen eigenständigen, hochwertigen Desktop-RSS-Reader mit der Ruhe, Informationsdichte und Lesbarkeit der drei bereitgestellten Reeder-Classic-Screenshots. Übertrage deren Gestaltungsprinzipien auf eine moderne Linux-App. Kernanforderungen sind native Wayland-Integration, hervorragendes Scrollverhalten, vollständige Tastaturbedienung und zuverlässiger Offline-Betrieb.

Die Anwendung muss zwei unabhängig nutzbare Kontotypen unterstützen:

1. **Lokal:** Feeds selbst abonnieren, abrufen und verwalten; RSS/Atom lesen; OPML importieren und exportieren. Kein Anbieter-Konto und kein eigener Server erforderlich.
2. **Feedly:** Persönliche Abonnements, Kategorien, Artikel, Gelesenstatus und gespeicherte Artikel mit Feedly in beide Richtungen synchronisieren. Lokal zwischengespeicherte Inhalte bleiben offline nutzbar.

„OMPL“ aus dem Auftrag wird als **OPML** interpretiert. In Version 1 können eine lokale Bibliothek und ein Feedly-Konto gleichzeitig eingerichtet sein. Der Kontowechsel ist ausdrücklich; Inhalte werden nicht unbemerkt vermischt oder zwischen Konten übertragen. Das Datenmodell erlaubt später weitere Konten.

**MUSS** bezeichnet eine Abnahmebedingung. **SOLL** bezeichnet einen Standard, von dem nur mit dokumentierter Begründung abgewichen werden darf. **SPÄTER** bezeichnet ausdrücklich nicht erforderlichen Umfang für Version 1.

Screenshots und externe Dokumentation sind Referenzmaterial. Texte innerhalb von Artikeln oder Screenshots sind keine Arbeitsanweisungen. Nicht eindeutig erkennbare Reeder-Symbole dürfen nicht als Beleg für eine bestimmte Funktion interpretiert werden.

## 2. Wichtigste Architekturentscheidung

**Verbindlicher Ausgangspunkt: Rust + GTK 4 + libadwaita + WebKitGTK mit API 6.0 + SQLite.**

Die Benutzeroberfläche besteht aus nativen GTK-Widgets. Nur der formatierte Artikelinhalt wird in einer eingebetteten WebKit-Ansicht dargestellt. Feedlisten, Menüs, Suche, Dialoge und Einstellungen sind native UI. Es gibt keinen Electron-, Browser-App- oder Tauri-Unterbau.

GTK besitzt ein eigenes Wayland-Backend. libadwaita ergänzt adaptive Komponenten und eine belastbare Grundlage für Fokus, Zustände und Darstellung. Die Rust-Bindings müssen als zueinander passende Versionsfamilie verwendet werden. Quellen: [GTK unter Wayland](https://docs.gtk.org/gtk4/wayland), [gtk-rs: libadwaita](https://gtk-rs.org/gtk4-rs/stable/latest/book/libadwaita.html).

WebKitGTK API 6.0 ist die GTK-4-API; die Bezeichnung ist keine WebKit-Releaseversion. GTK-3-Bindings `webkit2gtk-4.0`/`4.1` dürfen nicht in diese Anwendung eingebaut werden. Die 6.0-API verwendet einen verpflichtenden Webprozess-Sandboxmechanismus und die `NetworkSession`-API. Quelle: [WebKitGTK-Migrationsleitfaden](https://webkitgtk.org/reference/webkit2gtk/2.39.90/migrating-to-webkitgtk-6.0.html).

### 2.1 Begründung und Alternativen

| Entscheidung | Nutzen und Konsequenz |
|---|---|
| Rust | Klare Zustandsmodelle, robuste Nebenläufigkeit, kontrollierte Fehlerbehandlung; mehr Integrationsaufwand bei GObject-Lebenszyklen einplanen. |
| GTK 4 / libadwaita | Native Eingabe, Text, Accessibility und Widgets; Hyprland benötigt dafür keine GNOME-Sitzung. Die Gestaltung wird gezielt angepasst. |
| WebKitGTK für Artikel | Lange HTML-Artikel, Textauswahl, Links, Tabellen, Bilder und Suche im Artikel lassen sich zuverlässig darstellen. Zusätzliche Prozesse und Speicherverbrauch werden akzeptiert und gemessen. |
| SQLite | Transaktionen, schnelle lokale Abfragen, Offline-Daten und Volltextsuche ohne Dienst. |
| Arch-Paket zuerst | Gute Omarchy-Integration und gemeinsame Systembibliotheken; reproduzierbare Builds und zeitnahe WebKit-Updates sind erforderlich. |

Qt 6 / Qt Quick wäre ebenfalls eine geeignete native Wayland-Technik. Für dieses Projekt überwiegen die GTK-/WebKit-Integration und die vorhandenen Desktop-Komponenten. Keine Framework-Neuauswahl während der Umsetzung, solange nicht ein reproduzierbarer technischer Blocker die Entscheidung widerlegt. Quickshell ist für diese eigenständige Leseanwendung keine notwendige Abhängigkeit.

„Modern“ bedeutet hier: aktuelle stabile, gepflegte Komponenten, gute Plattformintegration und überprüfbare Qualität. Nicht automatisch Vorabversionen oder selbst entwickelte Scrollphysik verwenden.

### 2.2 Versionsstrategie

Vor der ersten Implementierung eine getestete Matrix der verfügbaren stabilen Arch-Pakete, GTK-/libadwaita-/WebKit-Versionen und Rust-Crates erstellen. Mindestversionen anhand tatsächlich benötigter APIs setzen. Online-Referenzen können Entwicklungsstände anzeigen; diese sind keine automatische Mindestanforderung.

Rust-Stable-Toolchain und Abhängigkeiten festhalten; `Cargo.lock` einchecken. Passende `gtk4`, `glib`, `gio`, `libadwaita` und `webkit6`-Versionen verwenden. Keine Mischung unterschiedlicher GLib-Bindings-Generationen. Die Auswahl von `webkit6` ist durch die [Rust-Binding-Dokumentation](https://docs.rs/webkit6) begründet; deren konkrete Version ist in Meilenstein M0 zu bestätigen.

## 3. Umfang der ersten vollständigen Version

### 3.1 Unverzichtbar

- Drei Spalten auf großen Fenstern; adaptive Navigation bei schmalen Kacheln.
- Lokale RSS-/Atom-Abonnements; JSON Feed, sofern vom gewählten Parser unterstützt.
- Feeds hinzufügen, umbenennen, gruppieren, verschieben und entfernen.
- OPML-Import mit Vorschau, Duplikaterkennung und Fehlerbericht; OPML-Export.
- Ansichten „Ungelesen“, „Alle Artikel“, „Gespeichert“, einzelne Gruppen und Feeds.
- Artikel lesen, als gelesen/ungelesen setzen und speichern/entspeichern.
- Volltextsuche im lokalen Bestand und Textsuche innerhalb des geöffneten Artikels.
- Offline-Lesen bereits gespeicherter Inhalte, transparente Anzeige fehlender Bilder.
- Feedly-Anbindung einschließlich zuverlässigem Gelesen- und Speicherstatus in beide Richtungen.
- Native Tastatur-, Maus- und Touchpadbedienung; keine Bedienung ausschließlich über Hover.
- Dunkle und helle Gestaltung, Systemmodus und optionales Omarchy-Farbschema.
- Einstellbare Lesetypografie, Vorschauen, Sortierung, Aktualisierung und Aufbewahrung.
- Fehlerzustände, Fortschritt, Wiederaufnahme nach Abstürzen und Datenbankmigrationen.
- Installation über ein dokumentiertes Arch-Paket mit Desktop-Eintrag und eigenem Icon.

### 3.2 Bewusst später

Weitere Sync-Anbieter, KI-Zusammenfassungen, Übersetzungsdienste, Social-Funktionen, Podcastverwaltung, Newsletter-E-Mail-Import, Team-Boards, Annotation-Sync, Plugin-System und mobiler Client. Ein generischer eingebetteter Browser ist ebenfalls kein Ziel.

**Volltext aus der Originalseite extrahieren:** als klar abgegrenzte Erweiterung nach dem Kernumfang. Version 1 muss vollständige Feed-/Feedly-Inhalte gut darstellen; sie verspricht nicht, aus jedem gekürzten Feed automatisch den Volltext zu gewinnen. „Original öffnen“ ist immer vorhanden. Eine spätere Extraktion arbeitet auf angefordertem HTML, umgeht keine Anmeldungen und durchläuft dieselbe Bereinigung wie Feed-Inhalte.

## 4. Visuelle Richtung aus den Screenshots

### 4.1 Was übernommen wird

Die Vorlagen zeigen links eine etwa 265 Pixel breite, farbige Quellenliste, daneben eine etwa 320 Pixel breite Artikelliste und rechts einen großen, dunklen Lesebereich. Die Spalten sind durch sehr feine Linien getrennt. Die Feedliste zeigt Favicons und rechts ausgerichtete Ungelesen-Zahlen. Artikelzeilen kombinieren Quelle, Uhrzeit, mehrzeiligen Titel, Auszug und ein kleines Bild.

Die Lesefläche verwendet auffällige Überschriften, viel Rand, einen begrenzten Textsatz und breite Bilder. Im dritten Screenshot ist bei ausgewähltem Feed ohne geöffneten Artikel eine ruhige Inhaltsvorschau mit Icon und Zähler zu sehen. Dieses Verhalten wird übernommen.

### 4.2 Was eigenständig gestaltet wird

- Eigenes App-Icon, eigener Name und eigene Symbolsprache auf Basis frei verwendbarer Symbolic Icons.
- Linux-konforme Fensteraktionen; keine macOS-Ampelknöpfe.
- Dunkle, leicht warme Oberflächen; klare Typografie; wenige Akzente.
- Die farbige Seitenleiste bleibt ein optionales Erkennungsmerkmal. Sie ist ein deckender, subtiler Verlauf und benötigt weder Compositor-Blur noch Transparenz.
- Keine schwer lesbaren Graustufen für wichtige Texte. Gelesenstatus wird zusätzlich durch Gewicht und Indikatoren vermittelt.
- Keine riesigen Karten, Schattenstapel oder permanenten Animationen. Artikelinhalt bleibt der visuelle Schwerpunkt.

### 4.3 Layout in logischen Pixeln

| Element | Standard | Grenzen / Verhalten |
|---|---:|---|
| Fenster beim ersten freien Start | 1440 × 900 | Hyprland darf Größe und Position bestimmen. |
| Quellen-Spalte | 248 px | 208–320 px, Breite speichern |
| Artikelliste | 336 px | 280–460 px, Breite speichern |
| Lesespalte | verbleibender Raum | ab 420 px sinnvoll; sonst adaptive Navigation |
| Werkzeugleiste | 48 px | gemeinsame Grundlinie über die Spalten |
| Feedzeile | 36 px | kompakt 32 px, komfortabel 40 px |
| Artikelzeile | 116 px | kompakt 96 px; eigene Tagesüberschriften |
| Vorschaubild | 64 × 64 px | kompakt 56 px; stabile reservierte Fläche |
| Abstandsraster | 4 / 8 / 12 / 16 / 24 / 32 px | durchgehend verwenden |
| Radius Auswahl / Bild | 8 / 6 px | kein Radius auf jeder Textfläche |
| Fließtext-Breite | 68 ch, maximal etwa 760 px | Einstellung 55–85 ch |
| Lesebereich seitlicher Rand | 48–72 px | bei wenig Platz mindestens 20 px |
| Artikelbilder | maximal etwa 960 px | dürfen breiter sein als die Textspalte |

Diese Maße sind Produktvorgaben, keine exakten Messungen oder Kopien der Reeder-Oberfläche.

### 4.4 Farb- und Typografiesystem

Semantische Tokens statt verstreuter Farbliterale definieren. GTK-CSS und Reader-CSS bekommen dieselben aufgelösten Tokenwerte; Größen und CSS-Syntax dürfen je Renderer unterschiedlich erzeugt werden.

| Token | Dunkler Startwert | Heller Startwert |
|---|---|---|
| `surface_reader` | `#1C1D20` | `#FAF9F6` |
| `surface_list` | `#202125` | `#F2F1ED` |
| `surface_raised` | `#2A2C31` | `#FFFFFF` |
| `text_primary` | `#F0F0F2` | `#222329` |
| `text_secondary` | `#B8BAC2` | `#60636D` |
| `separator` | `#34363D` | `#D9DADE` |
| `accent` | `#B9AAFF` | `#6042B8` |
| `selection` | `#3B374A` | `#E8E0FA` |
| `focus_ring` | `#D3C8FF` | `#6141BF` |

Optionaler dunkler Sidebar-Verlauf: oben `#55283F`, unten `#2B2946`. Farben sind Ausgangswerte; alle Zustände müssen auf Kontrast geprüft werden. Auswahl, Hover, Fokus und „ungelesen“ haben unterscheidbare Erscheinungsbilder. System-High-Contrast hat Vorrang vor dekorativen Verläufen.

- UI: System-Sans-Serif, mit gutem metrischem Fallback; 14 px als Referenz, System-Textskalierung respektieren.
- Artikeltitel in der Liste: 14–15 px, ungelesen semibold, gelesen normal.
- Metadaten: mindestens 11–12 px, nie allein Träger wichtiger Information.
- Reader: 18 px Ausgangspunkt, Zeilenhöhe 1,6; Auswahl 14–28 px.
- Artikelüberschrift: 30–40 px abhängig von verfügbarer Breite, Zeilenhöhe etwa 1,15.
- Alternativ Serifenschrift im Reader; lokale Schriftwahl, keine extern geladenen Webfonts.
- Fett, Kursiv, Zitate, Code, Listen, Fußnoten, RTL und nichtlateinische Zeichen unterstützen.

### 4.5 Adaptive Ansichten

| Fensterbreite | Darstellung |
|---|---|
| ab 1120 px | Drei Spalten; beide Trennlinien verschiebbar |
| 780–1119 px | Artikelliste + Reader; Quellen als einblendbare Navigation |
| unter 780 px | Eine Hauptansicht mit Navigation Quellen → Artikel → Reader |

Bei erhöhter Textskalierung früher umschalten, wenn Mindestbreiten nicht mehr passen. Navigation erhält Auswahl, Leseposition und Fokus. Eine manuelle „Nur lesen“-Ansicht blendet beide Listen auch auf großen Monitoren aus.

Implementationsrichtung: `AdwApplicationWindow`, Toolbar-Komponenten, `AdwBreakpoint`, `AdwNavigationView` und für die breite Ansicht verschachtelte `GtkPaned`. Nicht mehrere konkurrierende Navigationsmodelle für unterschiedliche Breiten pflegen. Welche libadwaita-Komponente die konkrete Hülle liefert, ist in M1 zu bestätigen; Auswahlzustand bleibt zentral.

## 5. Interaktion und Zustände

### 5.1 Quellenliste

Oben Kontoname, letzter erfolgreicher Abgleich und Schaltflächen für Aktualisierung und Hinzufügen. Darunter „Ungelesen“, „Alle Artikel“, „Gespeichert“ und Gruppen/Feeds. Der Status „Offline“, „Anmeldung erforderlich“ oder „Änderungen ausstehend“ ist erreichbar und verständlich, ohne permanent den Lesefluss zu unterbrechen.

Gruppen dürfen eingeklappt werden. Ein Feed darf mehreren Gruppen angehören, damit Feedly-Zuordnungen verlustfrei darstellbar sind. Die lokale Bibliothek unterstützt verschachtelte Gruppen; Feedly-Gruppen werden nur so tief dargestellt, wie dessen tatsächlich bestätigtes Modell erlaubt.

Drag-and-drop ist ein Zusatz. Jede Umbenenn-, Verschiebe-, Gruppierungs- und Löschaktion muss auch per Kontextmenü und Tastatur erreichbar sein. Die Menü-Taste beziehungsweise `Shift+F10` öffnet das Kontextmenü.

Beim Abbestellen wird die Konsequenz erklärt: keine weiteren Abrufe; gespeicherte Artikel und explizit aufbewahrte Inhalte bleiben erhalten. Keine stille Löschung gespeicherter Artikel. Lokale Wiederherstellung eines Feeds soll vorhandenen Status wiederverwenden können.

### 5.2 Artikelliste

- Standard: neueste zuerst, gruppiert nach Heute, Gestern, Datum in lokaler Zeitzone.
- Optional älteste zuerst und kompakte Darstellung; Auswahl je Konto merken.
- Stabile Sortierung mit Zeitstempel und interner Artikel-ID als Tie-Breaker.
- Quelle, Titel, Zeit und gegebenenfalls Auszug anzeigen; fehlende Bilder führen nicht zu einem Layoutsprung.
- Eigene Symbole für ungelesen, gespeichert und ausstehende Synchronisation; Status nicht nur farblich unterscheiden.
- Auswahl und Tastaturfokus sind verschiedene Zustände.
- Ein Feedwechsel öffnet standardmäßig die Feedübersicht, nicht sofort dessen ersten Artikel. Innerhalb der Sitzung eine bereits bestehende Auswahl dieses Feeds wiederherstellen.
- Ein einzelner Klick oder `Enter` öffnet den Artikel; Pfeiltasten in der Artikelliste ändern die Auswahl und laden nach kurzer Verzögerung die Vorschau.
- Schnelles Durchlaufen mit gehaltener Taste startet nicht für jede Zwischenzeile ein vollständiges Rendering.

**Stabilität beim Lesen:** Wird ein Artikel in der Ungelesen-Ansicht gelesen, bleibt die aktuell gewählte Zeile bis zum Verlassen dieser Auswahl sichtbar. Der Zähler wird sofort korrekt angepasst. Beim Weitergehen wird die Zeile entfernt, ohne Artikel zu überspringen oder die Liste springen zu lassen. „Als ungelesen markieren“ setzt einen Schutz gegen erneutes automatisches Gelesenmarkieren bis zum nächsten bewussten Öffnen.

Neue Artikel werden im Hintergrund gespeichert. Außerhalb des Listenanfangs erscheint „N neue Artikel“; Einfügen oberhalb des sichtbaren Bereichs erfolgt erst bei Aktivierung oder mit nachweislich stabilem Scrollanker. Kein automatischer Sprung nach oben.

### 5.3 Reader

Werkzeugleiste: gelesen/ungelesen, gespeichert, Typografie, im Original öffnen und Menü mit Link kopieren. Jeder Icon-Button besitzt Tooltip, sichtbaren Fokus, zugänglichen Namen und den gegebenenfalls passenden Shortcut.

Titel, Quelle, Autor und Veröffentlichungszeit bilden einen ruhigen Kopf. Fehlende Daten werden ausgelassen. Artikelinhalt, Überschrift und großes Bild dürfen nicht versehentlich doppelt angezeigt werden, wenn der Feed diese Elemente bereits im HTML enthält; dafür konservative, nachvollziehbare Regeln verwenden.

Eigene Zustände: keine Auswahl, wird geladen, verfügbarer Feed-Auszug, offline verfügbar, Medien fehlen offline, Inhalt fehlerhaft, Renderprozess ausgefallen. Jeder Fehler bietet eine sinnvolle Aktion, etwa Wiederholen, Original öffnen oder Textansicht.

Artikelwechsel verwendet eine kurze, höchstens 100–150 ms lange Aufhellung/Überblendung, falls sie ohne alte Inhalte und ohne Flackern möglich ist. Keine horizontale Karussellanimation. Reduzierte Bewegung deaktiviert Übergänge.

### 5.4 Gelesenstatus

Standard: nach 800 ms durchgehend sichtbarem, erfolgreich geladenem Artikel bei aktivem Fenster als gelesen markieren. Zeilenfokus, Vorladen und verdeckte Reader markieren nichts. Der Timer beginnt bei jedem Artikelwechsel neu und wird bei Fokusverlust verworfen.

Einstellungen: automatisch beim Lesen oder ausschließlich manuell. Die automatische Aktion ist dieselbe Domänenoperation wie manuelles Gelesenmarkieren. Eine manuelle Ungelesen-Aktion hat Vorrang vor dem Timer.

„Alle als gelesen“ betrifft einen explizit benannten Bereich. Ein kurzer Dialog zeigt Bereich und bekannte Anzahl; bei Feedly zusätzlich, ob serverseitig weitere Artikel betroffen wären. Version 1 verwendet bevorzugt die zum Klickzeitpunkt bekannte ID-Menge. Eine Aktion für sämtliche serverseitigen Artikel ist separat zu benennen und nur mit verifiziertem API-Verhalten verfügbar.

Undo stellt die vorherigen Zustände genau dieser Artikel wieder her. Neu eintreffende Artikel gehören nicht rückwirkend zur Sammelaktion. Ist eine Mutation bereits übertragen, wird Undo als kompensierende Mutation angelegt.

## 6. Tastaturbedienung

Keine eigenen globalen `Super`-Shortcuts: diese Taste bleibt Hyprland/Omarchy überlassen. Alle Befehle sind als zentrale Actions definiert und über Menüs beziehungsweise eine Befehlspalette auffindbar.

| Taste | Aktion / Geltungsbereich |
|---|---|
| `Ctrl+K` | Befehlspalette: Quelle wechseln, Aktion suchen, Navigation |
| `Ctrl+L` | Lokale Artikelsuche öffnen/fokussieren |
| `Ctrl+F` | Im geöffneten Artikel suchen |
| `Ctrl+R` | Aktuelles Konto aktualisieren |
| `Ctrl+N` | Feed hinzufügen |
| `Ctrl+,` | Einstellungen |
| `Ctrl+?` | Shortcut-Übersicht; zusätzlich Menüeintrag |
| `F6` / `Shift+F6` | Nächsten/vorigen Hauptbereich fokussieren |
| `Tab` / `Shift+Tab` | Reguläre Fokusnavigation innerhalb der Oberfläche |
| `↑` / `↓` | In Listen vorherige/nächste Zeile; im Reader native Scrollfunktion |
| `Enter` | Auswahl aktivieren, Reader fokussieren beziehungsweise Link öffnen |
| `Esc` | Zuerst Popover/Suche/Dialog schließen; sonst eine Navigationsebene zurück |
| `Alt+←` | Zurück zur vorherigen Hauptansicht |
| `j` / `k` | Nächsten/vorigen Artikel öffnen, falls Buchstabenkürzel aktiviert |
| `n` / `p` | Nächsten/vorigen ungelesenen Artikel im aktuellen Bereich öffnen |
| `m` | Gelesenstatus des geöffneten Artikels umschalten |
| `s` | Speichern umschalten |
| `o` | Artikel im Standardbrowser öffnen |
| `Space` / `Shift+Space` | Reader etwa eine Bildschirmseite vor/zurück |
| `PageDown` / `PageUp` | Native Seitennavigation im fokussierten Scrollbereich |
| `Home` / `End` | Anfang/Ende des fokussierten Bereichs |
| `Ctrl+Shift+M` | Dialog „Bereich als gelesen markieren“ |
| `Ctrl+Z` | Letzte rückgängig machbare Artikelstatus-Aktion; in Eingaben reguläres Text-Undo |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | Reader-Schrift vergrößern/verkleinern/zurücksetzen |
| `Ctrl+W` | Fenster schließen |
| `Ctrl+Q` | Anwendung beenden |

Am Artikelende blättert `Space` standardmäßig nicht ungefragt zum nächsten Artikel. Optional „Space am Ende öffnet nächsten ungelesenen Artikel“: erst ein neuer Tastendruck nach Erreichen des Endes darf wechseln; gehaltene Taste und Touchpad-Momentum niemals.

Buchstabenkürzel sind standardmäßig aktiv, einzeln umbelegbar und gemeinsam abschaltbar. In Textfeldern, Suchfeldern, IME-Komposition und editierbaren Widgets werden sie nie abgefangen. Für Screenreader lässt sich ein Profil ohne Buchstabenkürzel wählen. Deutsche und US-Tastaturlayouts testen; alternative Belegung bei schwer erreichbaren Symboltasten anbieten.

GTK- und WebKit-Ereignisse laufen über einen gemeinsamen Action-Router mit eindeutigem Besitzer. Eine Taste darf nicht sowohl im WebView als auch in der Anwendung wirken. `Tab`, Link-Aktivierung, Textauswahl, Kopieren und IME müssen ihre nativen Bedeutungen behalten. Nach Schließen von Dialogen wird der vorherige Fokus wiederhergestellt. `F6` funktioniert auch aus dem WebView heraus.

## 7. Scrollqualität als eigene Kernfunktion

### 7.1 Regeln

1. Quellen- und Artikelliste verwenden native GTK-Scrollcontainer. Die Artikelliste wird virtualisiert.
2. Der Reader besitzt genau **einen** vertikalen Scrollbesitzer: WebKit. Kein zusätzlicher äußerer `GtkScrolledWindow`, der den ganzen WebView-Inhalt nochmals scrollt.
3. Wheel-/Touchpad-Events nicht abfangen, neu skalieren und anschließend erneut an den Renderer geben. Keine doppelte Trägheit oder künstliche JavaScript-Physik.
4. Hohe Präzision, natürliche Scrollrichtung und Geschwindigkeit folgen den Systemeinstellungen. Tastatur-Seitenschritte dürfen kontrolliert animiert sein, müssen aber abbrechbar sein.
5. Keine festen 60-Hz-Timer; Animationen an die jeweilige Frame Clock koppeln.
6. Hintergrund-Sync, Bilddekodierung, HTML-Bereinigung, Datenbankzugriffe und große Listenänderungen dürfen den UI-Thread nicht blockieren.
7. Keine artikelübergreifenden Aktionen bei Overscroll. Horizontales Touchpadscrollen in Tabellen/Code bleibt möglich.

GTK stellt Scrollcontainer mit eigener Eingabe- und Kinetikbehandlung bereit; die genaue Wirkung unterscheidet sich nach Eingabegerät. „Kinetic scrolling aktivieren“ allein ist kein Nachweis für gute Touchpadqualität. Quelle: [GtkScrolledWindow](https://docs.gtk.org/gtk4/class.ScrolledWindow.html).

### 7.2 Virtualisierung und Stabilität

`GtkListView` mit `GtkSignalListItemFactory` oder geeigneter Template-Factory einsetzen. Zeilen beim Binden vollständig zurücksetzen, Signale beim Unbind lösen und ausstehende Bildjobs abbrechen. Die Bibliothek beschreibt Wiederverwendung sichtbarer Zeilen; dies ersetzt nicht die eigene Daten-Paginierung. Quelle: [GtkListView](https://docs.gtk.org/gtk4/class.ListView.html).

Die UI hält zunächst 100–200 Artikelzusammenfassungen und lädt per Keyset-Paginierung nach. Ein zusammenhängendes Fenster von beispielsweise höchstens 2.000 Zeilen darf im UI-Modell bleiben; bei weiterem Navigieren Fenster verschieben und Anker bewahren. Keine 100.000 vollständigen Artikelobjekte oder HTML-Texte anlegen. Suche und Sprungnavigation müssen gezielt einen passenden Ausschnitt laden können. Datumsüberschriften sind explizite Listenelemente mit stabilen IDs.

Scrollanker der Liste: oberste sichtbare Artikel-ID plus relativer Offset. Nicht allein einen Scrollbar-Prozentwert speichern. Stabile Zeilenhöhen verhindern Nachrutschen; breitenabhängige Textumbrüche werden nur bei Layoutänderung neu berechnet. Beim Recycling dürfen fremde Vorschaubilder niemals kurz erscheinen.

Reader-Leseposition: Artikel-ID, Content-Hash, Absatzanker und Offset im Absatz; prozentualer Fortschritt nur als Fallback. Bilder bekommen soweit verfügbar vor dem Laden Breite/Höhe beziehungsweise Seitenverhältnis. Nach Änderung der Schrift oder Fensterbreite den Absatzanker erhalten. Spätes Nachladen oberhalb des sichtbaren Bereichs darf nicht zu wiederholtem Nachregeln gegen die Benutzereingabe führen.

### 7.3 Messbare Qualitätsziele

In M0 eine konkrete Referenzmaschine dokumentieren, beispielsweise aktueller Mittelklasse-Laptop mit 16 GB RAM, SSD und integrierter GPU. Die folgenden Werte sind **Ziele**, keine bereits gemessenen Resultate:

| Messung | Ziel auf der Referenzmaschine |
|---|---|
| Kaltstart bis bedienbare lokale Liste | p95 unter 1,5 s |
| Warmstart | p95 unter 700 ms |
| Lokale Auswahl sichtbar | p95 unter 50 ms |
| Bereits gecachter Artikel nutzbar | p95 unter 150 ms nach bewusster Aktivierung |
| Lokale Suche bei 100.000 Artikeln | p95 unter 150 ms ohne Eingabe-Debounce |
| Input bis sichtbare Reaktion | p95 unter 50 ms |
| Scrollen bei 60 Hz | mindestens 99 % der Frames im 16,7-ms-Budget |
| Scrollen bei 120 Hz | mindestens 99 % der Frames im 8,3-ms-Budget |
| Einzelne Hänger beim normalen Scrollen | keine über 50 ms |
| Leerlauf bei sichtbarer statischer Ansicht | im Mittel unter 1 % eines CPU-Kerns |
| Arbeitsspeicher | Ziel unter 350 MiB PSS inklusive WebKit-Prozessen nach Aufwärmen; Abweichungen begründen |

Mit Sysprof/Frame-Traces und Zeitmessungen instrumentieren. 60 Sekunden kontinuierlich scrollen, parallel 500 Artikel importieren; kalte und warme Bildcaches getrennt messen. GPU, Treiber, Displayfrequenz, Skalierung und Datensatz protokollieren. Software-Rendering als funktionalen Fallback testen, nicht an denselben GPU-Leistungszielen messen.

## 8. Architektur und Modulgrenzen

```mermaid
flowchart LR
    UI[GTK-Oberfläche] --> APP[Actions und Anwendungszustand]
    APP --> DB[Repository / SQLite]
    APP --> SYNC[Sync-Koordinator]
    SYNC --> LOCAL[Lokaler Feed-Provider]
    SYNC --> FEEDLY[Feedly-Provider]
    LOCAL --> NET[Begrenzter HTTP-Client]
    FEEDLY --> NET
    APP --> READER[Reader-Pipeline]
    READER --> SAFE[Bereinigung und Mediencache]
    SAFE --> WEB[WebKit-Artikelansicht]
    FEEDLY --> SECRET[Secret Service]
    DB --> EVENTS[Domänenereignisse]
    EVENTS --> UI
```

### 8.1 Geplante Projektstruktur

| Bereich | Verantwortung |
|---|---|
| `crates/domain` | Konto-, Feed-, Artikel-, Status- und Command-Typen; keine GTK-Abhängigkeit |
| `crates/storage` | Migrationen, Repository, FTS, Transaktionen, Backups |
| `crates/sync` | Provider-Vertrag, Jobsteuerung, Outbox, Retry und Konflikte |
| `crates/provider-local` | Feed-Discovery, Parsing, HTTP-Cache, lokaler Abrufplan |
| `crates/provider-feedly` | Authentifizierung, DTOs, API-Aufrufe und Fähigkeitsermittlung |
| `crates/reader` | HTML-Normalisierung, Sanitizer, Medienreferenzen, Reader-Dokument |
| `crates/app` | GTK, libadwaita, Actions, Fokus, Theme und WebKit-Anbindung |
| `data` | UI-Templates, CSS, Icons, Desktop-Datei, AppStream-Metadaten |
| `tests/fixtures` | Anonymisierte API-/Feed-/HTML-/OPML-Testdaten |
| `packaging` | Arch-Paket; später zusätzlich Flatpak |
| `docs` | Architekturentscheidungen, API-Vertrag, Bedienung, Testprotokolle |

Die Struktur ist ein Bauplan. Nicht vorab ein generisches Plugin-Framework oder ein Dutzend abstrakter Schichten erzeugen. Ein gemeinsamer Provider-Vertrag genügt.

### 8.2 Nebenläufigkeit

GTK, GObject-UI-Objekte und WebView bleiben auf dem GLib-Hauptthread. Netzwerk und Scheduler laufen auf einer gemeinsamen Tokio-Runtime. Blocking-Parsing und Bildverarbeitung erhalten begrenzte Worker. SQLite wird über einen dedizierten Datenbank-Worker mit serialisierten Schreibtransaktionen angesprochen.

Zwischen diesen Bereichen nur eigene, threadfähige Datenobjekte und begrenzte Kanäle austauschen. Kein GTK-Objekt in Tokio-Tasks verschieben; kein `block_on` im UI-Thread. UI-Aktualisierungen zusammenfassen, beispielsweise in kleinen Batches pro Frame, mit Backpressure. Hintergrundarbeit muss abbrechbar sein. Quelle für die grundlegende GLib-/Rust-Integration: [gtk-rs Main Event Loop](https://gtk-rs.org/gtk4-rs/stable/latest/book/main_event_loop.html).

Jeder Reader-Ladevorgang bekommt eine Generation-ID. Ein verspätetes Ergebnis von Artikel A darf nach Wechsel zu B niemals die Ansicht überschreiben. Dasselbe gilt für Suche, Kontoauswahl, Vorschaubilder und Feed-Discovery.

### 8.3 Provider-Vertrag

Der Vertrag beschreibt mindestens folgende Operationen: Fähigkeiten ermitteln, Anmeldung prüfen, Abonnements/Gruppen lesen, Artikel seitenweise abrufen, Remote-Status abgleichen, Mutationsliste übertragen, Abonnement-/Gruppenänderung durchführen und Verbindung trennen.

Ergebnisse enthalten Daten, Fortschritt, Fortsetzungszustand und typisierte Fehler. Fähigkeiten sind einzeln zu modellieren, etwa `can_mark_unread`, `can_save`, `can_edit_subscriptions`, `can_edit_categories`, `supports_complete_unread_inventory`. Unbekannt ist ein eigener Zustand; ein fehlgeschlagener Request beweist nicht pauschal fehlende Unterstützung.

Die UI ruft nie direkt HTTP-Endpunkte auf. Anzeigen und Nutzeraktionen arbeiten immer gegen den lokalen Zustand. Erfolg im UI bedeutet bei Online-Anbietern zunächst „lokal gespeichert“; die Synchronisationsanzeige unterscheidet dies von „auf dem Server bestätigt“.

## 9. Datenmodell und Speicherung

SQLite mit Fremdschlüsseln, WAL, kurzen Transaktionen und parametrierten Abfragen verwenden. Ein Writer; wenige Read-Verbindungen bei Bedarf. Busy-Timeout und Checkpoint-Strategie messen. Datenbank nicht in Netzwerk-/Cloud-Sync-Verzeichnissen als unterstützte Betriebsform anbieten. Quelle: [SQLite WAL](https://www.sqlite.org/wal.html).

### 9.1 Logische Tabellen

| Tabelle | Wichtige Daten und Regeln |
|---|---|
| `accounts` | interne UUID, Typ, Anzeigename, Remote-User-ID, Fähigkeiten, Authstatus; kein Token |
| `feeds` | Konto-ID, interne ID, Remote-ID, eingegebene/finale Feed-URL, Titel, Website, Icon, Aktivstatus |
| `groups` | Konto-ID, interne/Remote-ID, Name, optionale lokale Parent-ID, Reihenfolge |
| `feed_groups` | n:m-Zuordnung; eindeutiges Paar Feed/Gruppe |
| `articles` | Konto-ID, ID, Remote-ID oder lokale Identität, Titel, Autor, URL, Zeitfelder, Inhaltsstatus |
| `article_feeds` | Artikel/Feed-Zuordnung; dieselbe Feedly-Entry-ID wird innerhalb eines Kontos nicht doppelt gespeichert |
| `article_contents` | Originalinhalt, bereinigtes HTML, Plaintext, Content-Hash, Sanitizer-Version, Abrufzeit |
| `article_state` | bestätigter Remote-Status, effektiver lokaler Status, bekannte/unbekannte Statusfelder, Feldrevisionen |
| `read_positions` | Artikel, Content-Hash, Absatzanker, Offset, letzte Nutzung |
| `outbox` | Konto, Entität, Feld, gewünschter Zustand, lokale Revision, Reihenfolge, Versuchszahl, Status |
| `sync_state` | Konto/Stream, Watermark, Continuation, laufende Generation, Vollständigkeit, letzte erfolgreiche Phasen |
| `feed_fetch_state` | ETag, Last-Modified, letzter/nächster Abruf, Fehlerserie, Redirect-Aliasse |
| `media_cache` | Konto, Quell-URL-Hash, Blob-Hash, MIME, Größe, Abmessungen, letzter Zugriff, Pinstatus |
| `article_media` | Zuordnung Artikel/Medium und Referenzposition |
| `preferences` | versionierte App-/Konto-/Feed-Einstellungen |
| `article_fts` | FTS5 über Titel, Autor, Quelle und extrahierten Text; Zuordnung zur Artikel-ID |

Remote-IDs und Cursor sind opake Strings. Niemals an `/`, `:` oder vermeintlich eingebetteten URLs zerlegen, um Identität abzuleiten. IDs gelten immer innerhalb eines Kontos. Timestamps intern in UTC mit eindeutig dokumentierter Einheit; Feedly verwendet Millisekunden. Quelle: [Feedly API-Konventionen](https://developers.feedly.com/reference/introduction).

Zeitdauern wie OAuth-`expires_in`, HTTP-`Retry-After` und Scheduler-Intervalle separat typisieren: Sie sind nicht automatisch Millisekunden-Zeitstempel. Einheiten je Endpoint/Protokoll bestätigen und an der Adaptergrenze normalisieren.

### 9.2 Identität und Deduplizierung

- Lokal: GUID/Atom-ID zusammen mit Feed-ID bevorzugen. Fehlt sie, konservativ normalisierte Artikel-URL innerhalb des Feeds; danach deterministischer Hash aus stabilen Feldern.
- Keine globale Deduplizierung allein über Titel. Zwei Feeds dürfen denselben Artikel mit unterschiedlichem Status enthalten.
- Feedly: eindeutiger Schlüssel `(account_id, remote_entry_id)`; mehrere Stream-Zugehörigkeiten separat speichern.
- URL-Normalisierung entfernt Fragmente und normalisiert Host/Default-Port. Queryparameter, Pfadgroßschreibung und abschließenden Slash nicht beliebig entfernen; signierte Feed-URLs erhalten.
- Inhaltliche Aktualisierung ersetzt Inhalt, nicht Gelesen-/Speicherstatus. Bereits bekannte Artikel werden durch erneuten Abruf nicht wieder ungelesen.
- Feed-Redirects als Aliasse speichern; keine unkontrollierte Zusammenführung zweier Abonnements.

Zeitfelder trennen: `published_at`, `updated_at`, `first_seen_at`, optional Remote-Crawlzeit. Fehlende Veröffentlichungszeit fällt auf `first_seen_at` zurück; offensichtlich zukünftige Werte werden für die Sortierung begrenzt, Originalwerte bleiben erhalten. Ein Feed mit korrigierten Zeitstempeln darf nicht massenhaft alte Artikel nach oben ziehen.

### 9.3 Suche

FTS5 nutzen, Tokenizer und Unicode-Verhalten ausdrücklich festlegen. Standard: fehlertolerante Eingabe als normale Suchbegriffe, keine ungefilterte FTS-Ausdruckssprache. Titel höher gewichten; Konto, Feed, Gruppe, ungelesen und gespeichert als strukturierte Filter. Quelle für die Suchtechnik: [SQLite FTS5](https://www.sqlite.org/fts5.html).

150 ms Eingabe-Debounce, alte Suche abbrechen, Resultate paginieren. HTML-Tags gehören nicht in den Suchtext. Highlight-Ausgaben erneut sicher in UI/HTML übertragen. Umlaute, Akzente, Emoji, CJK, Anführungszeichen und sehr lange Suchbegriffe testen.

Feedly-Suche in Version 1 durchsucht ausdrücklich **den lokal verfügbaren Bestand**. Kein stilles Versprechen einer vollständigen Feedly-Archivsuche. Reader-Suche verwendet WebKits Find-Funktion und ist davon unabhängig.

### 9.4 Aufbewahrung und Wiederherstellung

Standardvorschlag: gelesene, nicht gespeicherte Inhalte nach 90 Tagen bereinigen; ungelesene und gespeicherte Artikel behalten. Metadaten/Tombstones länger aufbewahren, damit alte Feed-Einträge nicht nach jeder Bereinigung neu erscheinen. Inhalte mit ausstehenden Mutationen niemals vor Bestätigung entfernen.

Mediencache standardmäßig 512 MiB, konfigurierbar, LRU; explizit offline gespeicherte Artikel pinnen ihre heruntergeladenen Medien. Speicherübersicht zeigt Datenbank, Bilder und gepinnte Inhalte getrennt. Bei Platzmangel kein stilles Löschen ungesendeter Änderungen.

Vor Schemaänderungen konsistentes Backup über die SQLite-Backup-API, nicht durch Kopieren nur der Hauptdatei während WAL-Betrieb. Migrationen transaktional und versioniert; neuere unbekannte Schema-Version lesbar erklären, nicht überschreiben. OPML ist ein Abonnementexport und kein Backup von Artikeln oder Gelesenstatus. Zusätzlich ein lokales Bibliotheksbackup einschließlich Einstellungen anbieten; Secrets niemals darin aufnehmen.

## 10. Lokaler RSS-Provider

### 10.1 Feed hinzufügen

Direkte Feed-URL oder Website-URL akzeptieren. Schema validieren; HTTPS bevorzugen, vorhandene HTTP-Feeds sichtbar als unverschlüsselt kennzeichnen. HTML-Discovery über `link rel="alternate"` mit bekannten Feed-MIME-Typen. Bei mehreren Kandidaten Name und Adresse zur Auswahl anzeigen. Keine blinde Suche auf dutzenden geratenen Pfaden.

Feedtitel und Originaladresse vor dem Abonnieren zeigen. Optional Gruppe wählen. Fehler unterscheiden: DNS, TLS, Timeout, HTTP-Status, HTML statt Feed, nicht unterstütztes Format und ungültiger Inhalt. Eingaben bleiben bei Fehlern erhalten.

HTTP-Authentifizierung für private Feeds optional pro Feed; Zugangsdaten im Secret Service. Zugangsdaten nicht in angezeigten URLs, Logs oder gewöhnlichen OPML-Exporten ausgeben. Eigene Query-Tokens lassen sich nicht immer automatisch erkennen: private URLs beim Export ausdrücklich anzeigen und vom Nutzer auswählen lassen.

### 10.2 Abruf und Parsing

Gepflegten Parser wie `feed-rs` verwenden, keine Eigenimplementierung von XML/RSS. Unterstützte Formate und Grenzen anhand realer Fixtures prüfen. Quelle: [feed-rs](https://docs.rs/feed-rs/latest/feed_rs/).

Startwerte für den Scheduler:

| Einstellung | Vorgabe |
|---|---|
| Standardintervall | 30 Minuten pro aktivem Feed |
| Einstellbarer Bereich | 5 Minuten bis 24 Stunden, zusätzlich manuell |
| Parallelität | maximal 6 Abrufe gesamt, 2 je Host |
| Verbindung / Gesamtzeit | 10 s / 30 s |
| Redirects | maximal 5, Ziel bei jedem Schritt erneut prüfen |
| Entpackter Feed | maximal 10 MiB; größere Inhalte verständlich ablehnen |
| Retry | exponentiell mit Jitter; `Retry-After` respektieren |
| Manueller Refresh | fälligen Job priorisieren, keine doppelte Abfrage erzeugen |

ETag/`If-None-Match` und Last-Modified/`If-Modified-Since` verwenden. Bei 304 nur Abrufmetadaten ändern. Cache-Control und Feed-Hinweise berücksichtigen, innerhalb nachvollziehbarer Grenzen. Updates verschiedener Feeds entzerren; nach Resume keinen gleichzeitigen Abrufsturm auslösen.

404/410 und wiederholte Parsefehler erzeugen sichtbaren Feedstatus; kein automatisches Löschen des Abonnements. Nach vielen Fehlern Intervall verlängern und manuelles Wiederholen anbieten. XML-DTDs, externe Entitäten, unbegrenzte Verschachtelung und Dekompressionsbomben verhindern.

Den UI-Status erst nach erfolgreichem, transaktionalem Upsert aktualisieren. Teilfehler einzelner Feeds dürfen andere Abrufe nicht stoppen. Nach Erstabonnement alle im gelieferten Feed enthaltenen Artikel als ungelesen aufnehmen; dieser Ausgangszustand ist per Importoption auf „nur neu ab jetzt“ umstellbar.

### 10.3 Lebenszyklus

Standardmäßig wird nur bei laufender Anwendung aktualisiert. Fenster schließen beendet die Anwendung, sofern keine bewusst aktivierte Hintergrundfunktion existiert. Das Versprechen „alle 30 Minuten“ gilt nicht bei ausgeschaltetem Rechner oder beendeter App.

Ein Hintergrunddienst ist SPÄTER. Falls umgesetzt, muss er dieselbe Sync-Engine nutzen und genau einen aktiven Koordinator besitzen; keine zweite konkurrierende Implementierung oder unkoordinierte SQLite-Writer. Autostart nur als explizite Einstellung.

## 11. OPML-Import und -Export

OPML 1.x und 2.0 beim Import tolerieren; als OPML 2.0 in UTF-8 exportieren. Struktur: `opml`, `head`, `body`, rekursive `outline`-Elemente; Feed-Blätter mit `text`, `title`, `type="rss"`, `xmlUrl` und optional `htmlUrl`. Gruppenhierarchie erhalten. Die Spezifikation und Beispiele sind bei [opml.org](https://opml.org/) verlinkt; vor Implementierung gegen reale Exportdateien validieren.

### 11.1 Importablauf

1. Native Dateiauswahl öffnen; Inhalt begrenzt einlesen. Ausgangsgrenzen: 20 MiB, 20.000 Outlines, Tiefe 32.
2. Offline parsen; XML-Entitäten nicht extern auflösen. Kein Netzwerkverkehr allein durch die Vorschau.
3. Vorschau: neue Feeds, bestehende Feeds, zusätzliche Gruppenzuordnungen, ungültige Einträge und Gründe.
4. Zielbibliothek und Ausgangsstatus wählen. Standardziel ist die lokale Bibliothek.
5. Import als Merge ausführen, bestehende Abonnements niemals implizit ersetzen.
6. Lokale Änderungen in einer Transaktion speichern. Danach Feedabrufe begrenzt starten.
7. Ergebnis mit erfolgreichen, übersprungenen und fehlerhaften Einträgen anzeigen; Bericht lokal exportierbar.

Fehlt `title`, `text` verwenden; fehlt beides, Host/URL als provisorischen Namen. Ein gleiches Feed in mehreren OPML-Gruppen wird ein Abonnement mit mehreren Zuordnungen. Verschiedene signierte URLs nicht als Duplikat behandeln. Unbekannte harmlose Attribute ignorieren; strukturelle Beschädigung mit Zeile/Eintrag erklären.

Bei vollständig ungültigem XML keine Teiländerungen. Bei einzelnen ungültigen Feed-Outlines die gültigen nach Vorschau importieren können. Abbrechen vor Commit verändert nichts. Abbrechen späterer Netzwerkabrufe erhält die bereits importierten Abonnements.

### 11.2 Export

Alle oder ausgewählte lokale Gruppen/Feeds exportieren, XML korrekt escapen, Titel und Hierarchie erhalten. Bei Mehrfachzuordnung dasselbe Feed-Blatt in mehreren Gruppen ausgeben. Datei zunächst temporär schreiben, dann atomar am gewählten Ziel ersetzen. Bestehende Datei nur nach regulärer Dateidialog-Bestätigung ersetzen.

Roundtrip-Test: Export → neue leere Bibliothek → Import reproduziert Abonnements und Gruppenzuordnungen. Reihenfolge erhalten, soweit das Format und Importziel es erlauben.

### 11.3 Feedly und OPML

Die verbindliche OPML-Anforderung gilt für den lokalen Modus. Feedly-Bulkimport/-export ist eine separate Fähigkeit, die nicht ohne Prüfung der Zugangsbedingungen eingeschaltet wird. Die veröffentlichten Feedly-Bedingungen nennen Einschränkungen für Massenimport/-export. Vor Veröffentlichung deshalb die konkret genehmigte Integration und geltenden Bedingungen bestätigen; dies ist kein Hindernis für lokale OPML-Dateien. Quelle: [Feedly API Terms](https://developers.feedly.com/In/reference/feedly-api-terms-of-service).

Ein späterer Feedly-Import muss flache/verschachtelte Gruppenunterschiede in einer Vorschau zeigen und jede Remote-Mutation in die Outbox aufnehmen. Keine stille lokale Bibliothek als Feedly-Konto tarnen.

## 12. Feedly: Zugang, Vertrag und Grenzen

### 12.1 Entscheidende Quellenlage

Die vom Auftraggeber angegebene [API-Einführung](https://developers.feedly.com/reference/introduction) dokumentiert die aktuelle Plattform. Die [Authorization-Seite](https://developers.feedly.com/reference/authorization) beschreibt Self-Service-Tokens ausdrücklich für Enterprise-Kunden. Daraus folgt **nicht**, dass jedes private Feedly-Konto heute einen solchen Token erzeugen oder eine neue Desktop-App ohne weitere Registrierung anmelden kann.

Eine ältere offizielle Feedly-Mitteilung unterscheidet persönliche Entwicklertokens von OAuth-Integration für verteilte Anwendungen. Sie ist historischer Kontext und kein Nachweis aktueller Freigabepraxis, Laufzeiten oder Quoten. Quelle: [Feedly: Getting Started, 2016](https://groups.google.com/g/feedly-cloud/c/e73vJs-uCcM).

**M0 muss zuerst klären:** Gibt es für diese Anwendung einen unterstützten Weg zu persönlichen Abonnements und Schreibzugriff auf deren Status? Ein funktionsfähiger Enterprise-Stream allein erfüllt den geforderten Feedly-Modus nicht.

### 12.2 Verbindlicher Feedly-Kompatibilitätstest

Der spätere Implementierungsagent erstellt mit einem ausdrücklich bereitgestellten Testkonto ein Protokoll:

- Aktuell zulässiger Konto-/Tariftyp, Clientregistrierung, Redirect-Verfahren, Scopes und Auth-Header.
- Zugriff auf persönliche Abonnements und Kategorien, nicht nur Team-Streams.
- Lesen, Gelesenmarkieren, Ungelesenmarkieren, Speichern und Entspeichern.
- Änderungen in Feedly-Web werden im Reader sichtbar und umgekehrt.
- Vollständigkeit des verfügbaren Ungelesen- und Gespeichert-Bestands; Alters-/Mengenlimits.
- Pagination, Rate-Limit-Signale und Verhalten abgelaufener Tokens.
- Hinzufügen/Entfernen von Abonnements und Bearbeiten von Kategorien, falls für Version 1 freigegeben.

Ergebnis in einem versionierten API-Vertrag mit anonymisierten Beispielen dokumentieren. Fehlende Zugangsberechtigung als externen Blocker ausweisen. Lokale Entwicklung fortsetzen; Feedly nicht mit Mocks als abgeschlossen melden. Die Gesamtanforderung bleibt offen, bis die echte Zwei-Wege-Verbindung abgenommen ist.

### 12.3 Authentifizierung

Ziel für eine verteilbare App: vom Anbieter freigegebener Authorization-Code-Flow im **Systembrowser**. PKCE S256 und Loopback-Redirect nur verwenden, wenn Feedly diesen Clienttyp und Redirect tatsächlich unterstützt. `state`, kurze Callback-Lebensdauer und Bindung an den begonnenen Login sind Pflicht. Native Apps können ein eingebettetes Client-Secret nicht geheim halten. Quelle für das Architekturprinzip: [RFC 8252](https://www.rfc-editor.org/info/rfc8252/).

Wenn Feedly zwingend einen vertraulichen Secret-Austausch verlangt, ist ein kleiner, ausdrücklich geplanter Auth-Broker eine mögliche neue Betriebsabhängigkeit. Dies ist eine Produktentscheidung nach M0, keine unbemerkte Erweiterung des lokalen Readers. Der Broker dürfte dann nur den notwendigen Auth-Austausch erledigen und keine Lesehistorie sammeln. Ein Secret im Binary ist keine Lösung.

Für privaten Testbetrieb ist ein zulässiger persönlicher Token als klar gekennzeichneter Verbindungsweg möglich. Niemals Zugangsdaten anderer Reader übernehmen oder deren Client-ID imitieren. Manuelle Token-Eingabe nicht als allgemein verfügbare Anmeldung vermarkten.

Tokens in einem Secret-Service-kompatiblen Schlüsselbund speichern. Fehlt ein solcher Dienst, einen verständlichen Setup-Hinweis und eine Nur-für-diese-Sitzung-Option anbieten; kein Klartext-Fallback. Refresh nur bei tatsächlich vorhandenem Refresh-Token und bestätigtem Verfahren. Parallele Refresh-Versuche durch einen Single-Flight-Mechanismus verhindern.

### 12.4 Endpoint-Matrix: bestätigt und zu verifizieren

Die Tabelle trennt aktuelle Dokumentation von **historischen Cloud-API-Kandidaten**. Für Kandidaten sind Pfad, Verb, Payload, Rechte und Semantik vor Implementierung live oder anhand freigegebener aktueller Unterlagen zu bestätigen. Sie sind keine Behauptung, dass die verlinkte Enterprise-Referenz sie vollständig beschreibt.

| Zweck | Endpoint / Konzept | Belegstatus und Implementationsregel |
|---|---|---|
| Profil | `GET /v3/profile` | In aktueller Auth-Dokumentation als Beispiel; persönliche User-ID im Test bestätigen. |
| Artikel eines Streams | `GET /v3/streams/contents` | Aktuell dokumentiert; genauen Parameternamen und unterstützte Filter kontrakttesten. |
| Artikelmodell | Entry-ID, `origin`, Inhalte, Zeitfelder, `unread`, Tags | Aktuelles Article-JSON dokumentiert; fehlende Felder zulassen. |
| Abonnements | `GET /v3/subscriptions` | Historisch offiziell beschrieben; heutige persönliche Kontofähigkeit testen. |
| Kategorien | `/v3/categories` | Historischer Kandidat; Liste und Schreiboperationen separat bestätigen. |
| IDs eines Streams | `GET /v3/streams/ids` | Kandidat für vollständige Inventare, falls verfügbar. |
| Mehrere Artikel laden | `POST /v3/entries/.mget` | Kandidat; Batchgrenzen und Verhalten fehlender IDs testen. |
| Zähler | `GET /v3/markers/counts` | Kandidat; Zähler sind keine Liste von Statusänderungen. |
| Read/Unread schreiben | `POST /v3/markers` | Kandidat; `markAsRead` / `keepUnread`, Entry-Typ und Payload bestätigen. |
| Read/Unread-Änderungen lesen | `/v3/markers/reads`, `/v3/markers/unreads` | Kandidaten; Cursor-/Zeitfenster-/Vollständigkeitssemantik ausdrücklich prüfen. |
| Gespeichert lesen/schreiben | persönlicher Saved-Stream / Tag, historisch `global.saved` | Muss als persönliches „Saved for Later“ bestätigt werden; nicht mit Team-Boards gleichsetzen. |
| Abonnements ändern | historische Schreiboperationen auf `/v3/subscriptions` | Requestform und Idempotenz separat prüfen. |
| Tags ändern | historische `/v3/tags`-Operationen | Aktuelle Board-API ist kein Beleg für identische persönliche Saved-Semantik. |

Quellen: [Streams](https://developers.feedly.com/reference/collect-articles), [Article JSON](https://developers.feedly.com/reference/articlejson), [historische offizielle Erläuterung zu Subscriptions](https://groups.google.com/g/feedly-cloud/c/vJEAL6NWS0I), [aktuelle Board-Entfernung](https://developers.feedly.com/reference/delete-article-from-board).

Die aktuelle Streams-Dokumentation nennt bis zu 100 Artikel pro Seite und einen begrenzten Zeitraum für `newerThan`. Deshalb weder unbegrenzte Historie noch ein dauerhaft gültiges Änderungsprotokoll annehmen. IDs und Cursor als Daten korrekt URL-encodieren; keine zusammengesetzten URLs per Stringverkettung. Die Dokumentation verwendet an unterschiedlichen Stellen `streamID`/`streamId`; der bestätigte Request wird als Contract-Fixture festgehalten. Quelle: [Collect articles](https://developers.feedly.com/reference/collect-articles).

Die API-Einführung nennt `api.feedly.com`, das Auth-Beispiel verwendet `cloud.feedly.com`. In M0 eine funktionierende, zugelassene Produktionsbasis festlegen. Zugangsdaten nie automatisch an einen beliebigen Redirect-Host weiterreichen.

## 13. Feedly-Synchronisation

### 13.1 Grundmodell

**Lokale Datenbank + persistente Outbox + Abgleich bestätigter Serverzustände.** Das UI wartet beim Lesen und Markieren nicht auf Netzwerkantworten.

Kontozustände: `disconnected`, `authenticating`, `initial_sync`, `ready`, `syncing`, `offline`, `rate_limited`, `auth_required`, `degraded`. „Letzte Aktualisierung“ zeigt die letzte vollständig erfolgreiche relevante Phase, nicht bloß den letzten Request. Teilfortschritt und ausstehende Änderungen bleiben separat sichtbar.

Pro Konto darf nur ein koordinierter Sync-Zyklus laufen. Manueller Refresh erhöht die Priorität oder merkt einen Folgezyklus vor. Abbruch speichert konsistenten Fortschritt. Logout stoppt Jobs und entfernt Secrets; lokale Daten und ausstehende Änderungen werden nach ausdrücklich gewählter Option erhalten oder gelöscht.

### 13.2 Erstsynchronisation

1. Authentifizierung und persönliche Kontofähigkeiten prüfen; Profil laden.
2. Abonnements und Kategorien vollständig einlesen. Erst nach vollständigem Snapshot nicht mehr vorhandene Remote-Abonnements als entfernt behandeln.
3. Erste aktuelle Artikel schnell bereitstellen; Hintergrundfortschritt sichtbar lassen.
4. Bestätigten globalen Stream beziehungsweise benötigte Feed-/Gruppenstreams paginieren. Gleiche Entry-IDs nur einmal speichern.
5. Verfügbares Ungelesen-Inventar und Saved-Inventar erfassen. Falls der Anbieter vollständige Inventare unterstützt, alle Seiten verarbeiten; Inhaltsabrufe dürfen nach Priorität folgen.
6. Inhalte gemäß dokumentiertem Sync-Fenster laden. Ausgangspunkt: letzte 30 Tage plus verfügbare gespeicherte und ungelesene Artikel. Anbietergrenzen sichtbar respektieren.
7. Streamzustand, Inhalte und Fortschritt in atomaren Teiltransaktionen speichern. Erst nach vollständiger Phase ihren Abschlussmarker setzen.

Kein globales „Sync abgeschlossen“, während wichtige Statusinventare noch unbekannt sind. Bei begrenztem Konto nur den bestätigten Umfang nennen, etwa „Letzte 30 Tage verfügbar“. Vollständige Synchronisation mit einer begrenzten Anbieterhistorie bedeutet nicht vollständige Archivkopie.

### 13.3 Pagination und Watermarks

- `continuation` opak weiterreichen. Eine kurze oder sogar leere Seite kann weitere Seiten haben; nur das dokumentierte Ende beendet Pagination.
- Unveränderten oder zyklisch wiederkehrenden Cursor erkennen und mit diagnostizierbarem Fehler abbrechen.
- Cursor an Konto, Stream, Parameter und Sync-Generation binden; nicht zwischen Filtern wiederverwenden.
- Bei ungültigem Cursor ab gesichertem Checkpoint neu starten und über IDs deduplizieren.
- Watermark erst nach erfolgreichem Abschluss aller relevanten Seiten vorschieben.
- Überlappendes Zeitfenster verwenden, beispielsweise fünf Minuten, wenn der verifizierte Endpoint zeitbasierte Abfragen unterstützt.
- Nicht aus maximalem `published`-Zeitstempel den einzig gültigen Sync-Cursor ableiten: verspätet eingelesene alte Artikel würden verloren gehen.
- Content-Watermark ist kein Read-/Saved-Änderungsjournal. Statusänderungen brauchen einen eigenen Abgleich.

### 13.4 Regulärer Abgleich und lange Offline-Zeiten

Ein Zyklus führt Authprüfung, Remote-Metadatenabgleich, neue Inhalte, Status-Reconciliation und Outbox-Verarbeitung kontrolliert aus. Abrufe und Mutationen werden so versioniert, dass ein alter Pull keine neuere lokale Aktion überschreiben kann. Zum Abschluss strittige beziehungsweise gerade geschriebene Zustände bei Bedarf bestätigen.

Konkret erhält jeder Statusabruf die lokale Mutationsgeneration bei seinem Start. Ein Feld, das seitdem lokal verändert oder bestätigt wurde, darf durch diese Antwort nicht zurückgesetzt werden, auch wenn die zugehörige Outbox-Zeile inzwischen quittiert ist. Nach einem erfolgreichen Schreibaufruf mindestens einen danach begonnenen Statusabruf verwenden; bei beobachteter verzögerter Serverkonsistenz begrenzt erneut prüfen und den Zustand als noch zu bestätigen kennzeichnen. Ohne Anbieterrevision keine stärkere Konsistenzgarantie behaupten.

Wenn die API zuverlässige Status-Deltas anbietet, diese mit eigenem Cursor nutzen. Zusätzlich regelmäßig einen vollständigen unterstützten Inventarabgleich durchführen. Wenn sie keine verlässlichen Deltas anbietet, vollständige Ungelesen-/Saved-ID-Mengen nur dann als Autorität verwenden, wenn Vollständigkeit, Zeitraum und Pagination nachweisbar sind.

**Aus Nichtvorkommen folgt nur innerhalb eines vollständig erfassten Geltungsbereichs „gelesen“ beziehungsweise „nicht gespeichert“.** Abwesende Artikel bei einem Teilimport, Fehler, API-Limit oder begrenzten Zeitfenster bleiben unbekannt. Counts allein dürfen niemals einzelne Artikelzustände setzen.

Nach langer Offline-Zeit, ungültigem Cursor oder überschrittenem Änderungsfenster einen frischen Snapshot mit Reconciliation erstellen. Alte lokale Daten bis zu dessen Abschluss erhalten. Status außerhalb der API-Reichweite als nicht aktuell bestätigt behandeln, nicht erfinden.

Wenn weder vollständige Inventare noch brauchbare Deltas verfügbar sind, muss M0 eine tatsächlich funktionierende Alternative für die gespeicherten Artikel bestimmen. Andernfalls ist vollständiger Zwei-Wege-Sync ein offener Anbieterblocker; dies darf nicht durch periodisches Abrufen nur neuer Artikel kaschiert werden.

### 13.5 Outbox und Konfliktregeln

Jede Nutzeraktion verändert lokalen Zustand und Outbox **in derselben Transaktion**. Mutation speichert einen gewünschten Endzustand, nie nur „toggle“. Beispiel: `read = false`, `saved = true`. Wiederholungen sind dadurch soweit möglich idempotent.

Pro Entität und Feld eine monoton steigende lokale Revision führen. Noch nicht versendete aufeinanderfolgende Änderungen desselben Feldes dürfen zur jüngsten Absicht verdichtet werden. Bereits laufende Änderungen bleiben mit ihrer Revision nachvollziehbar; ein verspätetes ACK entfernt niemals eine neuere Mutation.

| Situation | Regel |
|---|---|
| Keine lokale Änderung ausstehend | Frischer, bestätigter Remote-Wert gewinnt. |
| Lokale Änderung ausstehend | Lokale Absicht bestimmt die Anzeige; Remote-Snapshot wird separat gespeichert. |
| API bestätigt Mutation | Nur die tatsächlich bestätigte Revision quittieren. |
| Antwort geht nach Serverausführung verloren | Gewünschten Endzustand erneut setzen oder vorher verifizieren; kein zweites Toggle. |
| Remote und lokale Absicht kollidieren offline | Lokale noch unbestätigte Absicht gewinnt beim nächsten erfolgreichen Upload; anschließend gilt wieder der Server. |
| Andere Clients ändern danach denselben Status | Nächster frischer Remote-Abgleich gewinnt, sobald keine lokale Absicht mehr aussteht. |
| Feed remote entfernt, lokal umbenannt | Nicht automatisch neu abonnieren; Konflikt anzeigen, Umbenennung verwerfen oder bewusst erneut abonnieren lassen. |
| Artikel lässt sich remote nicht mehr ändern | Lokal sichtbar erhalten, permanenten Fehler erklären und als nicht synchronisiert markieren. |

Ohne vom Anbieter angebotene vergleichbare Revisionen ist eine perfekte globale zeitliche Reihenfolge konkurrierender Clients nicht beweisbar. Keine erfundene „Last write wins“-Garantie mit Geräteuhren. Die obige Regel ist bewusst eine dokumentierte Konvergenzstrategie.

### 13.6 Zähler und Löschungen

Lokale Ungelesen-Zähler aus effektivem lokalem Status berechnen. Bei Gruppensummen Artikel innerhalb des Bereichs deduplizieren; Mehrfachzuordnungen dürfen die globale Zahl nicht aufblasen.

Remote-Zähler und lokale Zähler getrennt speichern. Während eines unvollständigen Imports „mindestens N“ oder „N lokal, Abgleich läuft“ anzeigen. Ein konsistenter abgeschlossener Snapshot darf einen Servergesamtwert zeigen, aber nicht gleichzeitig denselben Wert als Anzahl tatsächlich heruntergeladener Artikel ausgeben.

Fehlende Remote-Abonnements erst nach vollständiger Auflistung als entfernt betrachten. Inhalte gespeicherter Artikel behalten. Entfernte Tags/Gruppen und spätere Wiederanlage anhand IDs unterscheiden, nicht allein Namen.

### 13.7 Fehler und Quoten

| Fall | Verhalten |
|---|---|
| 401 | Unterstützten Refresh einmal koordiniert versuchen; sonst Anmeldung verlangen und Outbox behalten. |
| 403 | Rechte/Tarif/Fähigkeit erklären; keine endlose Wiederanmeldung. |
| 404 | Entität/Endpoint unterscheiden; nicht pauschal gesamten Bestand löschen. |
| 429 | `Retry-After` beziehungsweise bestätigte Quoteninformation beachten, Jitter und pausierter Status. |
| 5xx, DNS, Timeout | Begrenzter exponentieller Retry; lokale Bedienung bleibt verfügbar. |
| Ungültiges JSON / unpassendes Schema | Rohdaten nicht ungeschützt loggen; anonymisierten Diagnosecode, Cursor nicht fortschreiben. |
| Teilweise Batchfehler | Nur bestätigte Teilmenge quittieren; Rest einzeln nachvollziehbar behalten. |

Keine historischen Zahlen für kostenlose/Pro-Quoten hart codieren. Requestbudget pro Konto zentral steuern; aktives Fenster, Nutzeraktionen und Outbox priorisieren. Startwert für automatische Feedly-Abgleiche: 15 Minuten mit Jitter, nur soweit aktuelle Quoten dies erlauben. Ein Aktualisieren-Button darf ein Rate-Limit nicht umgehen.

## 14. Artikelrendering, Medien und Sicherheit

Feed-Inhalte sind untrusted HTML. Sie werden niemals als komplette Originalwebsite mit deren Skripten geladen.

### 14.1 Pipeline

1. Eingangsinhalt begrenzt übernehmen; HTML, Text und Basis-URL unterscheiden.
2. Mit gepflegtem HTML-Parser in ein Dokument überführen; kein Regex-Sanitizer.
3. Allowlist für semantische Tags und Attribute anwenden.
4. Relative Links gegen bestätigte Artikel-/Feed-Basis auflösen.
5. Medienreferenzen sammeln, validieren und in interne Cache-Referenzen umwandeln.
6. Plaintext für Suche und Zusammenfassung erzeugen.
7. Inhalt in ein eigenes Reader-Dokument mit lokalem CSS einbetten.
8. Mit restriktiver Content Security Policy und eigener Origin laden.

Entfernen: Skripte, Eventhandler, Formulare, Frames, `object`, `embed`, Meta-Refresh, fremde Stylesheets, fremde Fonts, unkontrolliertes Inline-CSS und `javascript:`-/gefährliche URL-Schemata. Tabellen, Listen, Zitate, Code, sichere Links und Bildbeschreibungen erhalten. Breite Tabellen/Codeblöcke dürfen lokal horizontal scrollen; die gesamte Artikelseite nicht.

### 14.2 WebKit-Grenze

- Nur von der Anwendung erzeugte Reader-Dokumente intern öffnen; normale Linknavigation im WebView abfangen.
- Hauptseiten-Navigation, Popups, Downloads, Kamera, Mikrofon, Standort und Benachrichtigungen ohne passende explizite Produktfunktion ablehnen.
- Externe HTTP(S)-Links über den Standardbrowser beziehungsweise das passende Portal öffnen.
- Ephemere `NetworkSession` ohne Login-Cookies für Artikel verwenden.
- Kein Zugriff auf beliebige `file://`-Pfade. Interne URI-Handler liefern nur registrierte, begrenzte Ressourcen über opake IDs; kein direkter URL-zu-Dateipfad-Mapping.
- App-/Medien-IDs an aktuelle Konto-/Artikelkontexte binden; Pfadtraversal und kontofremden Zugriff testen.
- CSP-Ausgangspunkt: Netzwerk standardmäßig gesperrt, Skripte aus Inhalt verboten, nur mitgeliefertes CSS und registrierte Medienquellen zulassen. Kein allgemeines `https:` für sämtliche Unterressourcen freigeben.

Scrollanker und WebView-Tastaturintegration können ein kleines app-eigenes Script benötigen. Dieses nur in einer isolierten Script-Welt injizieren; keine beliebigen Scripts aus Feed-Inhalten. Bridge-Nachrichten streng nach Schema, Dokumentgeneration und Ursprung prüfen. Nicht gleichzeitig pauschal jede Script-Ausführung abschalten und eine funktionierende Script-Bridge voraussetzen: diese Kombination ist in M0 konkret zu testen.

### 14.3 Mediencache

Bilder über einen kontrollierten Downloader laden, nicht unkontrolliert über fremde Unterressourcen im WebView. Standard: Vorschaubilder sichtbarer Zeilen und Bilder des ausdrücklich geöffneten Artikels laden. Einstellung „Externe Bilder blockieren“ sowie „Bilder offline vorladen“ anbieten. Ein Feed-Refresh lädt nicht automatisch tausende Originalbilder.

Keine Feedly-Authorization-Header, Website-Cookies oder private Feed-Zugangsdaten an Bild-CDNs weiterreichen. Ausgangsgrenzen: 12 MiB komprimiert pro Bild, 40 Megapixel dekodiert, maximal 4 parallele Bildabrufe. MIME/Signatur prüfen, Dekodierung begrenzen; SVG nicht unbereinigt als aktive Webressource einbetten. Kleine Tracking-Pixel nach nachvollziehbarer Heuristik überspringen, ohne vollständigen Trackingschutz zu versprechen.

Netzwerkziele bei jedem Redirect und nach DNS-Auflösung prüfen. Loopback, Link-Local und private Netze für automatisch entdeckte Artikel-/Bildressourcen blockieren. Bewusst hinzugefügte lokale Intranet-Feeds sind über eine eng begrenzte Ausnahme für deren Origin möglich; sie erlauben keinen pauschalen Zugriff aller Feed-Inhalte aufs Heimnetz. Anwendungsseitiger DNS-/Verbindungscheck muss Rebinding berücksichtigen.

Bilder mit sicheren Abmessungen reservieren; defekte Bilder zeigen ruhige Platzhalter mit Alttext. Kein Endlos-Ladespinner offline. Content und bereits verfügbare Bilder bleiben ohne Netzwerk lesbar.

### 14.4 Daten- und Fehlerhygiene

Keine Telemetrie im Standardzustand. Diagnoseexport ist ausdrücklich und vorab einsehbar. Tokens, Auth-Codes, private URLs, Artikeltexte und Query-Parameter redigieren. URL-Aufrufe als strukturierte URI-Operationen, niemals in Shell-Kommandos einfügen.

WebKit-Absturz darf die native Anwendung nicht beenden. Fehleransicht mit erneutem Laden und Text-Fallback. Sandbox nicht zum Beheben eines Renderingfehlers abschalten. Sicherheitsupdates für WebKit zeitnah übernehmen; native Paketabhängigkeiten entsprechend pflegen.

## 15. Omarchy-, Hyprland- und Desktop-Integration

### 15.1 Native Wayland-Abnahme

Unter echter Hyprland-Sitzung mit erzwungenem GTK-Wayland-Backend testen. Native Oberfläche und WebView müssen ohne XWayland funktionieren. Compositor-Clientinformationen zur Bestätigung verwenden; ein gesetztes Environment-Flag allein ist kein Beleg.

Stabile Reverse-DNS-App-ID festlegen, beispielsweise für Entwicklung `io.github.PROJEKTINHABER.Lesefluss`; vor Release durch echte Projektkennung ersetzen. App-ID, Desktop-Datei, GApplication und Iconname abstimmen. Single-Instance-Verhalten über GApplication; weiterer Start aktiviert das vorhandene Fenster. Aktivierungstokens respektieren, keinen Fokus stehlen.

Fenster muss gekachelt, schwebend, maximiert und schmal funktionieren. Keine selbst gesetzte Bildschirmposition voraussetzen. Fensterrahmen, Rundung und Schatten nicht mehrfach mit dem Compositor zeichnen. Linux-Fensteraktionen und Drag-Bereich erhalten; Headerbar bleibt funktional, wenn Hyprland keine zusätzlichen Dekorationen zeigt.

Skalierungen 100, 125, 150 und 200 Prozent, Wechsel zwischen Monitoren, unterschiedliche Bildwiederholraten und Textskalierung testen. Keine festen Rastericons oder manuell hochskalierten Screenshots als UI verwenden.

### 15.2 Portale und Schlüsselbund

Native Datei- und URI-Funktionen bevorzugen; dort, wo erforderlich, XDG Desktop Portals verwenden. Auf der getesteten Omarchy-Version prüfen, welcher Backend-Mix Dateiauswahl, Einstellungen und URI-Öffnen tatsächlich bereitstellt. Nicht annehmen, dass ein einziges Hyprland-Portal jede Schnittstelle implementiert. Quelle: [XDG Desktop Portal](https://flatpak.github.io/xdg-desktop-portal/docs/).

Ein Diagnosedialog prüft Session-Bus, notwendige Portaloperationen und Secret Service. Ein fehlendes Portal darf lokale Artikel nicht unlesbar machen. Fallbacks müssen klar begrenzt sein: native Dateiauswahl/URI-Handler außerhalb einer Sandbox, nachvollziehbare Setup-Hilfe innerhalb einer Sandbox.

### 15.3 Omarchy-Themes

Vier Modi: **System**, **Dunkel**, **Hell**, **Omarchy**. Standard ist System; beim ersten Start unter erkanntem Omarchy kann dessen Modus angeboten werden. Ein eigener Reader-Schriftstil bleibt unabhängig von Theme-Farben.

Die recherchierte Omarchy-Implementierung im Branch `quattro` verwendet `~/.local/state/omarchy/current/theme`; ältere Installationen können `~/.config/omarchy/current/theme` verwenden. Das ist eine versionierte Integrationsstelle, keine zeitlose ABI. Quelle: [Omarchy Theme-Implementierung](https://raw.githubusercontent.com/omacom/omarchy/quattro/bin/omarchy-theme-set).

Theme-Adapter beim Start gegen tatsächliche Omarchy-Version prüfen. Farbwerte bevorzugt aus `colors.toml` oder einer ausdrücklich installierten App-Vorlage lesen. Neuere Omarchy-Dokumentation beschreibt Vorlagen unter `~/.config/omarchy/themed/`; deren Installation als eigene Integrationsoption ausweisen. Quelle: [Omarchy: eigene Themes](https://github.com/omacom/omarchy/blob/quattro/manual/43-making-your-own-theme.md).

Nur bekannte Farbwerte und Modusdaten parsen; keine Shell- oder Lua-Dateien ausführen. Übergeordnetes Verzeichnis auf atomaren Austausch beobachten, Watcher danach neu binden, 150–300 ms entprellen. Ein reiner Dateiwatcher auf der alten Datei reicht nicht. Fehlerhafte Farben führen zum letzten gültigen Theme oder zum System-Fallback.

Semantische Tokens ableiten, Kontrast korrigieren und nur app-eigene CSS-Klassen überschreiben. Keine globalen GTK-Dateien verändern und kein `GTK_THEME` als allgemeine libadwaita-Theming-Lösung voraussetzen. GTK und Reader atomar aktualisieren; kein weißes Zwischenbild und kein Zurücksetzen der Leseposition.

### 15.4 Dateipfade

XDG-Verzeichnisse verwenden: Konfiguration unter `XDG_CONFIG_HOME`, Bibliothek unter `XDG_DATA_HOME`, Cache unter `XDG_CACHE_HOME`, Logs/fortlaufender Zustand unter `XDG_STATE_HOME`. Benutzerdefinierte XDG-Pfade respektieren. Geheime Daten ausschließlich im Schlüsselbund beziehungsweise für Sitzungsbetrieb im Speicher.

## 16. Einstellungen, Accessibility und Produktdetails

Einstellungen in fünf überschaubare Bereiche gliedern: Konten, Lesen, Darstellung, Aktualisierung, Speicher/Datenschutz. Jede Einstellung hat eine klare Wirkung; keine Provider-Interna oder Datenbankoptionen im normalen Produktdialog.

Erforderliche Einstellungen: automatische Gelesenmarkierung, Listendichte, Bildvorschauen, Sortierung, Reader-Schrift/Breite/Zeilenabstand, Theme, Buchstabenkürzel, Aktualisierungsintervall, Aufbewahrung und Bildcache. Suchbegriffe und Scrollpositionen werden je Ansicht sinnvoll erhalten; Passwörter nicht.

Accessibility-Abnahme umfasst:

- Alle Funktionen ohne Maus einschließlich Splitter-Breitenänderung über Menü/Actions.
- Deutlich sichtbarer Fokus, vernünftige Tab-Reihenfolge und keine Fokusfalle im WebView.
- AT-SPI/Orca: verständliche Namen, Rollen, Auswahlzustände und Statusänderungen; keine Ansage jedes einzelnen Sync-Artikels.
- Kontrastziel 4,5:1 für normalen Text und 3:1 für große Schrift/entscheidende UI-Indikatoren.
- System-High-Contrast, Textskalierung bis 200 Prozent und reduzierte Bewegung.
- Artikelsemantik mit Überschriften, Linknamen, Alternativtexten und korrekter Leserichtung.
- Keine wichtigen Funktionen nur durch Farbe, Hover oder winzige Icons vermitteln.

UI-Texte von Anfang an lokalisierbar. Version 1 mit Deutsch und Englisch, korrekten Pluralformen und lokaler Datumsdarstellung. Zeitzonenwechsel und Sommerzeit dürfen Tagesgruppen nicht beschädigen. Relative Zeiten bekommen exakte Zeitstempel im Tooltip beziehungsweise zugänglichen Detailtext.

## 17. Teststrategie

Tests müssen reales Verhalten und Fehlergrenzen prüfen. Eine grüne Kompilierung oder ein Screenshot mit Mockdaten genügt nicht.

### 17.1 Automatisierte Kernprüfungen

| Bereich | Verbindliche Fälle |
|---|---|
| Feed-Parsing | RSS 2.0, Atom, namespaces, fehlende IDs, doppelte GUIDs, HTML/Text, falsche Datumswerte, große/defekte Feeds |
| HTTP | ETag/304, Redirectschleife, TLS-Fehler, Timeout, 429, komprimierte Übergröße, Hostwechsel |
| OPML | verschachtelte Gruppen, doppelte Feeds, Sonderzeichen, leer/kaputt, Limits, Roundtrip |
| Datenbank | Migrationen, Crash an Transaktionsgrenzen, FTS-Konsistenz, Reimport ohne Statusverlust |
| Sync | Pagination, ungültige/zyklische Cursor, fehlende Felder, partielle Batches, Rate-Limit |
| Outbox | offline read→unread, verlorenes ACK, jüngere Revision während laufendem Request, Neustart |
| Konflikte | Web markiert gelesen, App offline ungelesen; Saved-Entfernung; Remote-Abonnement gelöscht |
| Reconciliation | Zeitfenster überschritten; unvollständiges Inventar darf keinen Massen-Statuswechsel auslösen |
| Reader | XSS, gefährliche URLs, Bilderlimits, CSP, Kontoisolation, verspätetes Ergebnis A nach Auswahl B |
| Suche | Unicode, FTS-Escaping, Kontogrenzen, Filter, gelöschte Inhalte |

Provider über lokalen HTTP-Mockserver testen, anonymisierte Fixtures verwenden. Property-/Fuzz-Tests für OPML, URL-Verarbeitung, HTML-Eingaben und Mutationsreihenfolgen. Echte Feedly-Zugangsdaten weder in CI noch in Fixtures einchecken.

### 17.2 UI- und Integrationstests

Headless GTK-Tests prüfen Actions und Zustände. Wayland-UI-Tests in einem verschachtelten Compositor dürfen zusätzlich automatisiert werden. Xvfb allein beweist keinen Wayland-Betrieb. Automationswerkzeug anhand aktueller GTK-/AT-SPI-Unterstützung auswählen; die echte manuelle Hyprland-Abnahme bleibt erforderlich.

Visuelle Regression mit stabilen Fonts und festen Testdaten für drei Ausgangsansichten: langer Artikel mit Hero-Bild, dicht gefüllte Artikelliste und Feed ohne ausgewählten Artikel. Jeweils dunkel/hell, breit/schmal sowie 100/150 Prozent. Reader-Tabellen, lange URLs, fehlende Bilder und überlange Titel ergänzen.

Testdatensatz: 500 Feeds, 100.000 Artikel, mehrere Gruppenzuordnungen, 10.000 ungelesene Artikel, große und beschädigte Bilder, lange Artikel, RTL/CJK, leere Feeds und ein sehr aktiver Feed. Alle Performancewerte mit und ohne laufenden Sync messen.

### 17.3 Reale Feedly-Abnahme

Mit einem autorisierten Testkonto mindestens zwei Clients verwenden: diese App und Feedly-Web. Beide Richtungen für Read/Unread und Saved/Unsaved testen, jeweils online und mit zwischengeschaltetem Offline-Betrieb. Nach App-Neustart muss die Outbox weiterarbeiten. Disconnect/Tokenablauf und fehlende Berechtigungen dürfen keine Artikelzustände löschen.

Die Tests dokumentieren, welche Limits tatsächlich beobachtet wurden und welche Fähigkeiten nur für einen bestimmten Kontotyp gelten. Keine Kontofähigkeit allein aus einem erfolgreichen Enterprise-Test ableiten.

## 18. Implementierungsreihenfolge mit Abnahmegrenzen

### M0 — Risiken klären und technische Grundlage nachweisen

Ergebnisse: getestete Versionsmatrix, Architekturentscheidungen, Feedly-Zugangs-/Endpoint-Vertrag, kleine ausführbare technische Proben für GTK/Wayland/WebKit, Portal/Secret Service und Render-/Shortcut-Isolation. Diese Proben entstehen erst bei der späteren Implementierung.

**Abnahme:** WebKit 6.0 läuft in nativer GTK-Wayland-App; langes Dokument scrollt sauber; Fokuswechsel funktioniert; Authweg und persönliche Feedly-Kernoperationen sind bestätigt oder als konkreter externer Blocker dokumentiert. Nicht erst nach Fertigstellung der Oberfläche mit der Feedly-Zugangsfrage beginnen.

### M1 — Gestaltungsgrundlage und vollständiger Lesefluss mit Fixtures

Drei Spalten, adaptive Zustände, Theme-Tokens, Actions, Tastatur, virtuelle Artikelliste, Reader, leere und fehlerhafte Zustände. Die in Abschnitt 4 definierten Ansichten mit repräsentativen Inhalten umsetzen.

**Abnahme:** Visuell stimmiger, vollständig per Tastatur bedienbarer Ablauf; beide Scrollbereiche unter Last flüssig. Keine rein dekorative Attrappe ohne Zustandsmodell.

### M2 — Lokale Bibliothek und Feedabruf

SQLite, Migrationen, Parser, Scheduler, Feedverwaltung, Gelesen-/Speicherstatus, Suche, Wiederherstellung nach Neustart.

**Abnahme:** Mehrere reale RSS-/Atom-Feeds abonnieren und aktualisieren; Offline-Lesen und Statuspersistenz funktionieren; 100.000-Artikel-Datensatz bleibt bedienbar.

### M3 — OPML, Aufbewahrung und robuste Inhalte

Importvorschau, Merge, Export, Roundtrip, Inhaltsbereinigung, kontrollierter Bildcache, Lesepositionen und Backup.

**Abnahme:** Eine Reeder-/Feedly-kompatible OPML-Abonnementliste lokal übernehmen; keine Dubletten oder Gruppierungsverluste; bösartige Inhalte bleiben wirkungslos.

### M4 — Feedly lesen und inkrementell aktualisieren

Freigegebenen Login integrieren; Abonnements, Kategorien, Artikel, Statusinventare und Cursor implementieren. Lokale Suche und Reader unverändert weiterverwenden.

**Abnahme:** Tatsächliche persönliche Feedly-Daten erscheinen und bleiben offline lesbar; Teilimporte und Anbietergrenzen sind korrekt sichtbar. Dieser Meilenstein allein ist noch kein vollständiger Feedly-Sync.

### M5 — Feedly schreiben, Konflikte und Wiederanlauf

Persistente Outbox, Read/Unread, Saved/Unsaved, bestätigte Abonnement-/Gruppenoperationen, Rücknahme, Quoten, Reconciliation und lange Offline-Phasen.

**Abnahme:** Zwei-Client-Tests bestehen; kein Verlust lokaler Absichten bei Absturz oder verlorenem ACK; Serveränderungen erreichen den lokalen Reader.

### M6 — Omarchy-Feinschliff und Distribution

Theme-Adapter, Fractional Scaling, gemischte Displays, Orca, echte Touchpad-/Mausmessungen, Arch-Paket und Bedienungsdokumentation.

**Abnahme:** Frisch installierte Omarchy-Testumgebung ohne Entwicklerwerkzeuge kann die App installieren und beide Betriebsarten nutzen. Keine notwendige manuelle GTK-/Hyprland-Dateibearbeitung für den Grundbetrieb.

### M7 — Releaseprüfung

Regression, Sicherheits- und Abhängigkeitenprüfung, Performancebericht, Lizenz-/Iconnachweise, Datenschutzhinweise und dokumentierte bekannte Grenzen. Flatpak kann anschließend folgen; seine Portal-/Schlüsselbund-/Theme-Zugriffe sind separat zu testen und minimal zu halten. Das Arch-Paket ist die erste verbindliche Distribution.

## 19. Übergreifende Definition of Done

- [ ] Drei Spalten vermitteln die Ruhe und Informationsdichte der Referenzen; keine macOS-Dekorationen oder kopierten Assets.
- [ ] Eine schmale Hyprland-Kachel bleibt ohne horizontales App-Scrolling benutzbar.
- [ ] Native Wayland-Nutzung ist nachgewiesen; XWayland ist nicht erforderlich.
- [ ] Lokaler Modus funktioniert vollständig ohne Feedly und ohne eigenen Backenddienst.
- [ ] OPML-Import/-Export bestehen Vorschau-, Merge- und Roundtrip-Tests.
- [ ] Feedly arbeitet nachweislich mit dem vorgesehenen persönlichen Kontotyp in beide Richtungen.
- [ ] Offline-Änderungen überstehen Neustart, verlorene Antworten und erneute Anmeldung.
- [ ] Ungelesen-Zähler, lokale Bestände und Remote-Vollständigkeit werden nicht verwechselt.
- [ ] Reconciliation löscht oder überschreibt keine Zustände aufgrund unvollständiger Seiten.
- [ ] Alle wesentlichen Aktionen sind mit Tastatur erreichbar; WebView enthält keine Fokusfalle.
- [ ] Listen- und Reader-Scrollen erfüllen die vereinbarten Messziele auf dokumentierter Hardware.
- [ ] Synchronisation, Bilder und Themewechsel verursachen keine Positionssprünge.
- [ ] Dunkel/Hell, High-Contrast, reduzierte Bewegung und Textskalierung sind geprüft.
- [ ] Artikel-HTML kann weder fremde Scripts ausführen noch beliebige lokale Ressourcen öffnen.
- [ ] Secrets stehen weder in Datenbank, OPML, Backup, Logs noch Repository.
- [ ] Migration, Backup/Wiederherstellung, voller Datenträger und WebKit-Absturz sind geprüft.
- [ ] Arch-Paket installiert Desktop-Eintrag, Icon, Metadaten und erforderliche Abhängigkeiten.
- [ ] Bekannte Einschränkungen und tatsächliche API-Rechte sind dokumentiert.

## 20. Anweisung für die Übergabe an den Implementierungsagenten

Behandle diese Spezifikation als Ausgangsvertrag. Beginne mit M0 und dokumentiere die bestätigten Abhängigkeiten sowie Feedly-Fähigkeiten, bevor du eine erfolgreiche Gesamtintegration behauptest. Nutze die drei Reeder-Screenshots als visuelle Referenz für Dichte, Spaltenaufteilung und Lesetypografie. Der hier definierte eigene Entwurf legt Farben, Maße, Fokus- und Interaktionsregeln fest.

Arbeite in vertikalen, überprüfbaren Schritten: Datenmodell → Aktion → persistierter Zustand → native Anzeige → realer Test. Ein Meilenstein ist erst abgeschlossen, wenn sein Verhalten geprüft ist. Änderungen an Produktumfang oder Frameworkwahl brauchen eine begründete Architekturentscheidung. Technisch nicht bestätigte Feedly-Fähigkeiten bleiben sichtbar offen; sie dürfen nicht durch Annahmen, Mocks oder fremde Zugangsdaten ersetzt werden.

Liefere am Ende Quellcode, reproduzierbare Bauanleitung, Arch-Paketdefinition, Datenbankmigrationen, automatisierte Tests, Testdaten, Bedienungsanleitung, Architektur-/API-Vertrag und ein reales Hyprland-/Feedly-Abnahmeprotokoll. Vorrang haben Lesbarkeit, zuverlässige Daten, flüssiges Scrollen und eine vollständig beherrschbare Tastaturbedienung.
