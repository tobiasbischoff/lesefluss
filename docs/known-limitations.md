# Bekannte Grenzen

**Stand:** 25.09.2026 · Lesefluss 0.1.0

## Funktional

- **Volltext** stammt aus dem Feed-HTML. Feed-Artikel ohne vollständigen Inhalt werden als
  Kurzfassung mit Weiterlesen-Link angezeigt; Originalseiten werden bewusst nicht nachgeladen
  (kein Scraping, keine Umgehung von Paywalls).
- **Feedly-Schreibpfad** ist auf Marker (gelesen/gemerkt) und Delta-Lesen beschränkt. Abonnements,
  Ordner und Tags werden beim Verbinden importiert, aber nicht verändert; ein Delta-Lauf gleicht
  Abos nicht erneut ab. Eine öffentliche OAuth-Anmeldung gibt es nicht (siehe `m7-offen.md`, X1).
- **Feedly Saved-Inventar** benötigt die Berechtigung des Kontos; antwortet die API mit einem
  Fehler, bleibt der lokale Merk-Status unverändert (Sync meldet den Fehler als Toast).
- **Konflikte** werden übersprungen statt überschrieben: existiert für einen Artikel eine
  ausstehende lokale Änderung, gewinnt diese gegen den Remote-Stand.
- **Kein Hintergrunddienst:** Es findet kein Sync ohne laufendes Fenster statt. Beim Start läuft
  ein Delta-Sync, danach im eingestellten Intervall (Standard 15 min).
- **Wiederherstellung** aus einem Backup greift beim nächsten Start; das Fenster wird nicht
  während des Betriebs neu geladen.
- **Mediencache** lädt Bilder von Feed-CDNs; sehr große Bilder landen als Platzhalter. Die
  Cachegrenze wird erst beim nächsten Besuch eines Artikels durchgesetzt. Bilder gespeicherter
  Artikel sind gegen das Pruning geschützt; wer sie auslagern will, muss die Merkung lösen.
  Mit „Externe Bilder blockieren“ in den Einstellungen wird gar nichts nachgeladen.
- **Netzwerkpolicy und Proxys:** Weiterleitungen werden Hop für Hop geprüft und die
  Verbindung bindet nur an geprüfte Adressen. Läuft ein HTTP-Proxy in der Umgebung, kann
  dieser die Zielprüfung nicht beeinflussen, aber Verbindungen über ihn sind nicht
  abgesichert; Intranet-Ziele bleiben ohne ausdrückliche Freigabe gesperrt.
- **Hochskalierung/Textskalierung:** Messungen erfolgten bei Skalierung 1.667 und 60 Hz; 125 %/
  150 % und gemischte Monitorlayouts sind nicht Teil der Messung (siehe `perf-report.md`).
- **Scrollbudget bei Dauerbetätigung** wurde über einen Stresstest (Artikelnavigation) belegt, nicht
  über ein 60-sekündiges Endgerät-Scrollen mit `sysprof`.

## Distribution

- **App-ID ist ein Platzhalter** (`io.github.PROJEKTINHABER.Lesefluss`) und wird mit der
  Namensklärung vor der ersten Veröffentlichung ersetzt; Desktop-Datei, AppStream und Icon
  tragen dieselbe ID und müssen dann gemeinsam umbenannt werden.
- **PKGBUILD** ist auf ein lokales Quellverzeichnis ausgelegt (`source=("git+file://$srcdir/../")`)
  und vor einer echten Veröffentlichung auf ein echtes Repository umzuschreiben. `makepkg` wurde
  auf dieser Maschine noch nicht vollständig durchlaufen.
- **Flatpak** ist nicht enthalten; Portal-, Schlüsselbund- und Theme-Zugriffe wären dort separat
  zu prüfen und minimal zu halten.
- **Kein Installer/Updater**, keine Signatur, kein AUR-Eintrag.

## Datenmodell

- Volltextsuche nutzt FTS5 (nur `unicode61`, keine Wortstämmme/Synonyme) auf Titel, Autor,
  Feed-Titel und bereinigten Text.
- DieArtikel-Aktualisierungen übernehmen Inhalt, nie den Lesestatus. Re-Import eines bereits
  gelesenen Artikels macht ihn also nicht wieder ungelesen.
- Aufbewahrung löscht Inhalt älterer gelesener, nicht gemerkter Artikel; der Titel-Eintrag
  bleibt als Platzhalter erhalten, damit Zähler und Historie stimmen.
- `article_fts` speichert den Text zusätzlich zur Tabelle `articles` (kein `contentless`-Modus) —
  größerer Speicherbedarf, dafür direkte Integritätsprüfbarkeit.

## Sicherheitsrahmen

- Reader-HTML wird mit `ammonia` (Whitelist) bereinigt; `script`, `on*`, `file:`- und
  `data:`-Nicht-Bildquellen werden verworfen. Der WebView nutzt eine restriktive Content-Security-
  Policy (siehe `crates/reader/src/lib.rs`).
- Das Netzwerk läuft über `reqwest` mit aktivierter TLS-Verifikation. Feed-URLs werden
  normalisiert (Fragment weg, Host klein, Standardports weg) — eine nachträgliche Hochstufung von
  `http` auf `https` findet bewusst nicht statt, damit Feed- und Medien-URLs unverändert bleiben.
- Das Feedly-Token liegt im Secret Service; die Datei-Fallback-Lösung nutzt Modus 600.
- Ein Ausführungsaudit der Abhängigkeiten (`cargo audit`) steht aus — in dieser Umgebung war kein
  Installieren der Advisories-Datenbank möglich.
