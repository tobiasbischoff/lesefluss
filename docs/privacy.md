# Datenschutzhinweise

**Stand:** 24.09.2026 · Lesefluss 0.1.0

## Grundsatz

Lesefluss ist ein lokaler Reader. Es gibt keine Telemetrie, keine Nutzungsstatistik, keine
Abkürzung, keine Werbung und keinen Hintergrunddienst. Alle Daten liegen in den
XDG-Verzeichnissen des angemeldeten Benutzers.

## Daten im Dateisystem

| Ort | Inhalt | Löschbar durch |
|---|---|---|
| `$XDG_DATA_HOME/lesefluss/library.db` (+ `-wal`, `-shm`) | Feeds, Artikel, Lesestatus, Volltextindex, Einstellungen, Outbox | Löschen der Dateien = vollständiger Reset |
| `$XDG_CACHE_HOME/lesefluss/media` | Bildcache (Cachegrenze einstellbar, Standard 512 MiB) | Wird automatisch nach Größe beschnitten |
| `$XDG_CONFIG_HOME/lesefluss/` | OPML-Entwürfe, ältere Token-Dateien (migriert) | Verzeichnis löschen |
| Schlüsselbund (Secret Service), Eintrag `lesefluss` / `feedly-token` | Feedly-Zugriffstoken | `secret-tool clear lesefluss feedly-token` |

Ein Backup (`lesefluss-backup-<zeitpunkt>.db`) landet im Download-Verzeichnis des Benutzers und
enthält dieselben Inhalte wie die Datenbank inklusive Volltext — er ist nicht verschlüsselt.

## Netzwerkzugriffe

- **Lokale Feeds:** HTTP(S)-Abruf genau der von dir eingetragenen Feed-URLs sowie der Adresse des
  Artikels, wenn du ihn öffnest. Medien (Bilder) werden nur von diesen Adressen geladen.
- **Feedly (nur wenn verbunden):** `cloud.feedly.com` für Delta-Sync, Marker-Schreibvorgänge und das
  Saved-Inventar. Der Token stammt aus dem Schlüsselbund, wird nie in die Datenbank, in OPML-Dateien,
  in Logs oder in das Repository geschrieben.
- **Sonst nichts.** Keine Analytics-Endpunkte, keine Crash-Reporter, keine Update-Abfrage.

## Feedly-Datenfluss im Detail

1. Lesen/Gemerkt-Ereignisse landen in einer lokalen Outbox und werden gebündelt an `markers`
   gesendet; Antworten werden quittiert, Fehler exponentiell wiederholt.
2. Beim Sync werden ungelesene IDs, gemerkte IDs und ein Überlappungsfenster der letzten fünf
   Minuten gelesen; daraus entstehen lokale Statusänderungen.
3. Das Saved-Inventar (`streams/ids`, `user/<id>/tag/global.saved`) gleicht die lokalen
   Merk-Markierungen ab. Fehlt ein Artikel remote, wird er lokal nicht mehr gemerkt gehalten.
4. Wird die Verbindung gekappt, bleiben alle Änderungen in der Outbox und gehen nach dem nächsten
   Start bzw. Sync erneut raus.

Der Client nutzt ausschließlich die Marker-Schnittstelle von Feedly. Andere Schreibpfade
(z. B. Tags) werden nicht verwendet.

## Voraussetzungen für Portale/Schlüsselbund

- Secret Service (GNOME Keyring/KWallet) für das Feedly-Token. Ohne erreichbaren Dienst legt
  Lesefluss das Token in `~/.config/lesefluss/feedly-token` mit Dateimodus 600 ab.
- Netzwerkzugriff ist für Feed-Abruf, Feedly und Bildcache nötig; ohne Netz bleibt die lokale
  Bibliothek vollständig lesbar.
