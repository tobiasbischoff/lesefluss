# M0 — Versionsmatrix (Stand: 23. September 2026)

Referenzmaschine: Omarchy (Arch), Hyprland/Wayland, wird bei Perf-Messungen konkret ergänzt.

## Systempakete (Arch, getestet auf diesem Rechner)

| Paket | Version | Anmerkung |
|---|---|---|
| rust | 1.98.1 | Stable-Toolchain |
| gtk4 | 4.22.4 | natives Wayland-Backend bestätigt |
| libadwaita | 1.9.3 | |
| webkitgtk-6.0 | 2.52.6 | GTK-4-API, NetworkSession Pflicht |
| sqlite | 3.53.4 | FTS5 verfügbar (zu verifizieren in M2) |
| libsoup3 | 3.6.6 | via webkit6/soup3-Binding |
| glib2 | 2.88.3 | |

## Rust-Crates (eine kompatible Bindings-Familie)

| Crate | Version | Feature-Flags |
|---|---|---|
| gtk4 (`gtk`) | 0.11.5 | `v4_10` (von webkit6 gefordert) |
| libadwaita (`adw`) | 0.9.2 | `v1_4` (ToolbarView) |
| webkit6 | 0.6.1 | default |
| glib | 0.22.10 | |
| gio | 0.22.10 | |
| javascriptcore6 | 0.6.0 | transitiv über webkit6 |
| soup3 | 0.9.0 | transitiv über webkit6 |
| tokio | 1.53.1 | geplant (M2) |
| rusqlite | 0.40.2 | geplant, `bundled` (M2) |
| feed-rs | 2.4.0 | geplant (M2) |
| reqwest | 0.13.5 | geplant (M2) |

Mindestanforderung aus tatsächlich genutzten APIs: GTK ≥ 4.10 (Bindings-Feature),
libadwaita ≥ 1.4, WebKitGTK ≥ 6.0-API. System hat 4.22/1.9/2.52 — Puffer vorhanden.

## M0-Probe: bestätigt

- `cargo run --bin lesefluss-probe` startet `AdwApplicationWindow` unter Hyprland.
- GDK-Display-Typ: `GdkWaylandDisplay` → natives Wayland, kein XWayland.
- Hyprland-Client class `io.github.PROJEKTINHABER.Lesefluss.Probe` gemappt.
- WebKit6-`WebView` mit ephemeraler `NetworkSession`, langes HTML-Dokument (200 Absätze),
  CSP `default-src 'none'` im Dokument, nativer Scrollbesitzer = WebKit.
- Native `GtkListBox`-Sidebar (248 px) + `GtkPaned`, F6-Fokuswechsel-Controller eingebaut.

## M0-Probe: offene Prüfpunkte (manuell am laufenden Fenster)

- [ ] Touchpad-/Wheel-Scrollen im WebView flüssig, keine doppelte Kinetik.
- [ ] Fokuswechsel Sidebar ↔ WebView (Tab, F6), keine Fokusfalle im WebView.
- [ ] Link-Klick im WebView (Probe lädt HTML neu; in M1: Navigation abfangen → externer Browser).
- [ ] Webprozess-Sandbox aktiv: `ps aux | grep -i webkit` zeigt `WebKitWebProcess` mit Sandbox-Flags.

## Portale und Secret Service (Diagnose auf diesem Rechner)

- `xdg-desktop-portal` 1.22.1 mit Backends `desktop.hyprland` und `desktop.gtk` aktiv.
- `org.freedesktop.secrets` wird von gnome-keyring 50.0 bedient; `libsecret` 0.21.7 vorhanden.
- Konsequenz: Token-Ablage über Secret Service möglich; Datei-/URI-Portale über gtk-Backend erwartbar (in M3/M6 gegentesten).

## Feedly (Abschnitt 12.1) — geprüft am 23.09.2026

**Ergebnis: persönliches Standard-Konto funktioniert.** Vollständiges Protokoll in
`docs/feedly-api-vertrag.md`.

- Zugangsweg (privater Testbetrieb): Legacy-OAuth-Code-Flow mit `client_id=feedly` +
  PKCE S256, Redirect `https://feedly.com/i/login`; Token 7 Tage gültig.
- Alte Dev-Token-Seite `/v3/auth/dev/tokens` ist tot; Self-Service-Tokens laut Doku
  nur für Enterprise. Für die Distribution eigene Client-Registrierung bei Feedly
  nötig (NewsFlash-Modell: staff-ausgestellte `client_id`/`client_secret`).
- Bestätigt (live, Zwei-Wege-Roundtrips): profile, subscriptions (GET/POST/DELETE),
  categories, streams/contents, streams/ids, entries/.mget, markers/counts,
  markAsRead/keepUnread (Wirkung verifiziert), markers/reads-Delta,
  Saved via PUT+Body / DELETE (Wirkung verifiziert).
- Blocker-Reste: `markers/unreads` = 404 (kein Unread-Delta → Inventarabgleich),
  Saved-POST mit 200-ACK aber ohne Wirkung (Falle dokumentiert),
  Rate-Limits/Refresh/Kategorie-Writes offen (siehe Vertrag §4).

