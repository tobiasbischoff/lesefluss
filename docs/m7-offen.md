# Restarbeiten nach M7 — priorisierte Arbeitsliste

**Stand:** 24.09.2026 · Bezug: `Implementationsplan-RSS-Reader-Omarchy.md` (Stand 23.09.2026),
`docs/m7-status.md`, Commit `027814f`.

Ergebnis des Spec-Abgleichs: Kern, Datenmodell, Feedly-Sync, Sicherheitsrahmen und
Performanceziele sind erreicht und belegt. Offen sind vor allem **Bedienung**,
**Produktdetails** und **Abnahmen/Tests**.

Jede Zeile nennt die Fundstelle in der Spezifikation und ein überprüfbares Abnahmekriterium.
Reihenfolge innerhalb einer Stufe: Abhängigkeiten beachten, ein Punkt pro Commit.

---

## M7.1 — Muss-Funktionen (releasekritisch)

### 1.1 Feedverwaltung (§3.1, §5.1)
- [ ] Kontextmenü auf Feed- und Gruppenzeilen (Menütaste, `Shift+F10`, rechte Maustaste):
      Umbenennen, Gruppenzuordnung ändern, Verschieben, Abbestellen, Als gelesen markieren.
- [ ] Umbenennen-Dialog mit Tastaturbedienung; Speichern über `storage::update_feed_title`.
- [ ] Gruppenauswahl beim Hinzufügen (`storage::set_feed_groups`), verschachtelte lokale Gruppen
      anlegen, leer stehende Gruppen nach Entfernen aufräumen.
- [ ] Abbestellen mit Erklärtext: keine weiteren Abrufe; gespeicherte und ausdrücklich
      aufbewahrte Inhalte bleiben erhalten. Kein stilles Löschen.
- [ ] `storage::remove_feed` ergänzen (Feed + Status + Zuordnungen in einer Transaktion,
      Inhalte behalten, `feed_fetch_state` aufräumen), Test dafür.
- **Abnahme:** Umbenennen, Umgruppieren, Verschieben und Abbestellen sind je Feed per Maus und
      per Tastatur möglich; nach Abbestellen bleiben gespeicherte Artikel lesbar; DB bleibt
      konsistent (Test).

### 1.2 Sortierung (§3.1, §4.2, §16)
- [ ] Sortiermodell neueste zuerst / älteste zuerst, persistent in `prefs` (`sort_order`).
- [ ] Keyset-Cursor muss beide Richtungen bedienen (`published_at, id` auf/absteigend),
      stabiler Tie-Breaker bleibt.
- [ ] Einstellung in der Seite „Darstellung“; Auswahl je Konto merken (§5.2).
- **Abnahme:** Umschalten ohne Sprung in der Liste, Nachladen funktioniert in beiden
      Richtungen, Zähler bleiben korrekt, `lf-bench` zeigt gleiche p95.

### 1.3 Tastatur- und Aktions-Vollständigkeit (§6)
- [ ] `Ctrl+K` Befehlspalette (Quellen wechseln, Aktion suchen, Navigation) — zentraler
      Action-Router, keine eigenen Tastenlistener.
- [ ] `Ctrl+?` Shortcut-Übersicht als Fenster plus Menüeintrag; Liste aus `set_accels_for_action`.
- [ ] Reader-Menü: „Link kopieren“ (§5.3), Tooltips und zugängliche Namen für alle Icon-Buttons
      prüfen.
- [ ] Splitter-Breiten per Tastatur/Action bedienbar (§16).
- [ ] `Esc`-Kette prüfen: Popover → Suche → Dialog → eine Navigationsebene zurück.
- **Abnahme:** Jede Aktion aus §6 ist ausführbar; Palette findet alle `win.*`-Aktionen;
      Screenreader-Namen vorhanden; kein Buchstabenkonflikt mit Textfeldern (bestehender Router).

### 1.4 Zustand und Ansichten (§4.3, §4.5, §5.1, §5.2, §16)
- [ ] Spaltenbreiten und Fenstergröße/-position in `prefs` speichern und wiederherstellen
      (Standard 1440×900, Quellen 248 px, Liste 336 px).
- [ ] „Nur-Lesen“-Ansicht: blendet beide Listen auch auf großen Monitoren aus.
- [ ] Gruppen einklappbar (Zustand pro Ansicht speichern).
- [ ] Auswahl und Suchbegriff pro Konto/Ansicht erhalten; Passwörter nie (§16).
- [ ] Listen-Scrollposition als oberste sichtbare Artikel-ID plus relativer Offset (§7.2).
- [ ] „N neue Artikel“ außerhalb des Listenanfangs mit Scrollanker, kein automatischer Sprung.
- [ ] Früherer Layoutwechsel bei hoher Textskalierung (Schwellen anpassen oder
      Textskalierung berücksichtigen).
- **Abnahme:** Nach App-Neustart stehen Ansicht, Auswahl und Leseposition an derselben Stelle;
      Themewechsel und Sync erzeugen keinen Sprung.

### 1.5 Feedly-Kontopflege (§13.1, §13.7, §12.4)
- [ ] Abmelden: Jobs stoppen, Token aus dem Schlüsselbund entfernen, danach wählen lassen,
      ob lokale Daten und ausstehende Änderungen bleiben.
- [ ] Kontozustände sichtbar: disconnected, initial_sync, syncing, offline, rate_limited,
      auth_required, degraded; „letzte vollständig erfolgreiche Phase“ statt „letzter Request“.
- [ ] Request-Budget pro Konto: Priorisierung von Fensteraktionen, Pause nach 429 mit
      `Retry-After`/Jitter, sichtbarer Status statt stillem Warten.
- [ ] Abo-/Gruppen-Schreiben: nur umsetzen, wenn gegen den dokumentierten Endpunkt live
      verifiziert; bis dahin als Fähigkeit `can_edit_subscriptions=false` ausweisen.
- **Abnahme:** Abmelden löscht keine lokalen Artikel; Status ist nach jedem Fehlerfall
      verständlich; keine Endlos-Wiederholung nach 401/403.

### 1.6 Medien- und Speichereinstellungen (§9.4, §14.3)
- [ ] „Externe Bilder blockieren“ und „Bilder offline vorladen“ in der Seite
      „Speicher & Datenschutz“; blockierte Bilder zeigen den vorhandenen Platzhalter.
- [ ] Speicherübersicht: Datenbankgröße, Bildcache, geprüfte/gepinnte Inhalte getrennt.
- [ ] Cache-Schnittstelle bleibt: 512 MiB Vorgabe, LRU, Pinning gespeicherter Artikel.
- **Abnahme:** Beide Schalter wirken ohne Neustart; Übersicht stimmt mit der Dateigröße überein.

### 1.7 Hinzufügen und Importoptionen (§10.1, §10.2, §11.1)
- [ ] Add-Feed-Dialog: Gruppe wählen, HTTPS bevorzugen, HTTP-Feed sichtbar als unverschlüsselt
      kennzeichnen, Fehlerarten unterscheiden (DNS, TLS, Timeout, Status, HTML statt Feed,
      ungültiges Format), Eingaben bleiben erhalten.
- [ ] Erstabonnement: alle enthaltenen Artikel als ungelesen übernehmen, Option „nur neu ab jetzt“.
- [ ] OPML-Import: Zielbibliothek wählen, Ausgangsstatus wählen, Ergebnisbericht (erfolgreich,
      übersprungen, fehlerhaft) lokal exportierbar, Limits 20 MiB / 20 000 Outlines / Tiefe 32
      mit verständlicher Meldung.
- **Abnahme:** Roundtrip-Test bleibt grün; Limit-Verletzung bricht ohne Teiländerung ab.

---

## M7.2 — Tests und Abnahmen

### 2.1 Fehlende automatisierte Fälle (§17.1)
- [ ] HTTP: Redirectschleife, TLS-Fehler, Timeout, 429 mit `Retry-After`, komprimierte
      Übergröße, Hostwechsel, XML-Entity/DTD-Abwehr.
- [ ] OPML: verschachtelte Gruppen, Doppel-Feeds, Sonderzeichen, leer/kaputt, Limits.
- [ ] Feed-Parsing: RSS 2.0, Atom, Namespaces, fehlende IDs, doppelte GUIDs, HTML/Text,
      falsche Datumswerte, großes/defektes Feed.
- [ ] Sync: ungültige/zyklische Cursor, fehlende Felder, partielle Batches, Rate-Limit.
- [ ] Konflikte/Reconciliation: Web markiert gelesen, App offline ungelesen; Saved-Entfernung;
      entferntes Remote-Abonnement; unvollständiges Inventar darf keinen Massenwechsel auslösen.
- [ ] Reader: XSS, gefährliche URLs, Bilderlimits, CSP, Kontoisolation, verspätetes Ergebnis A
      nach Auswahl B.
- [ ] Suche: FTS-Escaping, Konto-/Feedgrenzen, gelöschte Inhalte.
- [ ] Property-/Fuzz-Tests für OPML, URL-Verarbeitung, HTML-Eingaben, Mutationsreihenfolgen.
- [ ] Headless-GTK-Tests für Actions und Zustand (§17.2).
- [ ] Zeitzonenwechsel und Sommerzeit dürfen Tagesgruppen nicht beschädigen; relative Zeiten mit
      exaktem Zeitstempel im Tooltip.
- **Abnahme:** Neue Tests rot vor der Änderung, grün danach; Suite wächst von 15 Suites merklich.

### 2.2 Messungen und visuelle Abnahme (§7.3, §15.1, §16)
- [ ] 60 s kontinuierlich scrollen parallel 500 Artikel importieren, `sysprof`/Frame-Traces.
- [ ] Skalierungen 100/125/150/200 %, Monitorwechsel, 60 Hz und 120 Hz getrennt protokollieren.
- [ ] Textskalierung 200 %, System-High-Contrast, reduzierte Bewegung.
- [ ] Orca/AT-SPI: Namen, Rollen, Auswahlzustände, Statusänderungen; keine Ansage jedes
      Sync-Artikels.
- [ ] Visuelle Regression für die drei Ausgangsansichten (langer Artikel mit Hero-Bild, dichte
      Liste, Feed ohne Auswahl) in dunkel/hell und breit/schmal.
- [ ] WebKit-Absturz und voller Datenträger: Fehleransicht mit erneutem Laden und Textansicht.
- **Abnahme:** `docs/perf-report.md` wird um 120-Hz-, Skalierungs- und Import-parallel-Messungen
      ergänzt; Abnahmeprotokoll als `docs/abnahme-protokoll.md`.

### 2.3 Verteilungsabnahme (§15, §18 M6)
- [ ] `makepkg -si` auf frischer Omarchy-Umgebung ohne Entwicklerwerkzeuge durchlaufen.
- [ ] Portale prüfen: Dateiauswahl, URI-Öffnen, Secret Service; fehlendes Portal darf Artikel
      nicht unlesbar machen.
- **Abnahme:** Installation, Start, beide Betriebsarten, Upgrade über ein bestehendes
      `library.db` ohne Datenverlust.

---

## M7.3 — Releaseformalien

- [ ] App-ID und Name finalisieren (aktuell Platzhalter `io.github.PROJEKTINHABER.Lesefluss`);
      `application_id`, Desktop-Datei, AppStream und Icon gemeinsam umbenennen; Namenskollision
      prüfen.
- [ ] `LICENSE-MIT` und `LICENSE-APACHE` ins Wurzelverzeichnis (AppStream verweist bereits darauf).
- [ ] Bedienungsanleitung und reproduzierbare Bauanleitung (README): Abhängigkeiten, Build,
      Test, Installation, Feedly-Verbindung, Datenorte, Wiederherstellung.
- [ ] `cargo audit`/`cargo deny` auf einer Maschine mit Netzzugang; Ergebnis in
      `docs/credits.md` oder `docs/known-limitations.md` nachziehen.
- [ ] Abweichungen von der Spec explizit bestätigen oder korrigieren: manueller Feedly-Token
      statt System-Browser-PKCE, vereinfachtes Schema, FTS ohne Stemming, `VACUUM INTO` als
      Backup, Farbpunkt statt Favicon, Backup/Restore ab Neustart.
- **Abnahme:** `docs/known-limitations.md` und `docs/credits.md` stimmen mit dem Standort überein.

---

## SPÄTER — ausdrücklich nicht für Version 1 (§3.2, §10.3, §12.4)

Hintergrunddienst/Autostart · Volltextextraktion von Originalseiten · Flatpak · weitere
Sync-Anbieter · KI-Zusammenfassung und Übersetzung · Annotationen, Newsletter-Import,
Podcastverwaltung · Favicons je Feed · HTTP-Authentifizierung für private Feeds ·
Feedly-Abo-/Gruppen-Schreiben (nur nach Live-Verifikation) · Suchsprache/Stemming.

---

## Was nur der Nutzer prüfen kann

- Physische Tastatur: `j/k` in Liste und Reader, Buchstaben im Suchfeld, `Ctrl+,`, `Ctrl+L`,
  `Ctrl+F`, `F6` aus dem WebView, `Alt+←`, `Esc`-Kette.
- Touchpad- und Mausgefühl beim Scrollen, Fenster im gekachelten, schwebenden, maximierten
  und sehr schmalen Zustand.
- Orca-Durchgang, Hochskalierung auf dem echten Monitor, Reduzierte-Bewegung-Einstellung.
- Feedly-Zwei-Client-Test mit dem Web-Client (beide Richtungen, offline mit Umweg).
