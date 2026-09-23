use domain::{ArticleMeta, Feed, Group};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Library {
    pub groups: Vec<Group>,
    pub feeds: Vec<Feed>,
    pub articles: Vec<ArticleMeta>,
    pub contents: HashMap<String, String>,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn hours_ago(h: i64) -> i64 {
    now_ms() - h * 3_600_000
}

const HERO_SVG: &str = "data:image/svg+xml;utf8,%3Csvg xmlns='http://www.w3.org/2000/svg' width='960' height='400'%3E%3Cdefs%3E%3ClinearGradient id='g' x1='0' y1='0' x2='1' y2='1'%3E%3Cstop offset='0' stop-color='%2355283F'/%3E%3Cstop offset='1' stop-color='%232B2946'/%3E%3C/linearGradient%3E%3C/defs%3E%3Crect width='960' height='400' fill='url(%23g)'/%3E%3Ccircle cx='760' cy='120' r='140' fill='%23B9AAFF' opacity='0.25'/%3E%3Ccircle cx='200' cy='320' r='180' fill='%23F0F0F2' opacity='0.08'/%3E%3C/svg%3E";

fn short_paras(topic: &str, n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!(
            "<p>{topic} — Absatz {i}: Die Anwendung soll ruhig, dicht und lesbar bleiben. \
             Native Widgets übernehmen Listen und Menüs, während lange Artikel in einer \
             eingebetteten Web-Ansicht gerendert werden. Dieser Blindtext steht stellvertretend \
             für echte Feed-Inhalte, die in späteren Meilensteinen aus dem Netz geladen werden.</p>"
        ));
    }
    s
}

pub fn build() -> Library {
    let groups = vec![
        Group { id: "g-tech".into(), name: "Technik".into(), parent: None },
        Group { id: "g-news".into(), name: "Nachrichten".into(), parent: None },
        Group { id: "g-design".into(), name: "Design".into(), parent: None },
    ];
    let feeds = vec![
        Feed { id: "f-heise".into(), title: "heise online".into(), website: Some("https://www.heise.de".into()), accent: "#B94A2E".into(), groups: vec!["g-tech".into()] },
        Feed { id: "f-lobsters".into(), title: "Lobsters".into(), website: Some("https://lobste.rs".into()), accent: "#AC3A50".into(), groups: vec!["g-tech".into()] },
        Feed { id: "f-tagesschau".into(), title: "tagesschau.de".into(), website: Some("https://www.tagesschau.de".into()), accent: "#2E5FA3".into(), groups: vec!["g-news".into()] },
        Feed { id: "f-design".into(), title: "Formfollows Blog".into(), website: Some("https://example.org/blog".into()), accent: "#3F8F5F".into(), groups: vec!["g-design".into()] },
    ];


    let mut articles: Vec<ArticleMeta> = Vec::new();
    let mut contents: HashMap<String, String> = HashMap::new();

    let mut add = |id: &str,
                   feed: &str,
                   title: &str,
                   author: Option<&str>,
                   url: &str,
                   hours: i64,
                   excerpt: &str,
                   unread: bool,
                   saved: bool,
                   has_image: bool,
                   html: String| {
        articles.push(ArticleMeta {
            id: id.into(),
            feed_id: feed.into(),
            title: title.into(),
            author: author.map(str::to_string),
            url: Some(url.into()),
            published_at: hours_ago(hours),
            excerpt: excerpt.into(),
            unread,
            saved,
            has_image,
        });
        contents.insert(id.into(), html);
    };

    add(
        "a-webkit-60",
        "f-heise",
        "Was WebKitGTK 6.0 für native Linux-Reader bedeutet",
        Some("Marie Keller"),
        "https://example.com/webkit60",
        2,
        "Die API 6.0 von WebKitGTK erzwingt NetworkSession und Sandbox — ein Überblick über Migration, Sicherheitsgewinn und Scrollverhalten.",
        true,
        true,
        true,
        format!(
            r#"<figure><img src="{hero}" alt="Abstrakter Verlauf in Violett- und Dunkelrot-Tönen" width="960" height="400"><figcaption>WebKitGTK 6.0: neue API-Familie für GTK-4-Anwendungen.</figcaption></figure>
<p>Die API-Version 6.0 von WebKitGTK ist kein gewöhnliches Release, sondern der Schnitt für GTK 4: <code>NetworkSession</code> wird Pflicht, der Webprozess läuft standardmäßig in einer Bubblewrap-Sandbox, und alte <code>WebContext</code>-Pfade entfallen.</p>
<h2>Migration in drei Schritten</h2>
<ol>
<li><strong>Session statt Context:</strong> Cookies, Cache und TLS-Entscheidungen hängen an einer <code>NetworkSession</code>. Ephemere Sessions ersetzen den inkognito Modus.</li>
<li><strong>Sandbox akzeptieren:</strong> Der Renderer läuft isoliert; Dateizugriff erfolgt über registrierte URI-Handler statt <code>file://</code>.</li>
<li><strong>Find-Controller behalten:</strong> Die Textsuche im Dokument bleibt nativ verfügbar.</li>
</ol>
<blockquote><p>Ein Reader gewinnt doppelt: Feed-HTML bleibt ohne Skripte, und ein Absturz des Webprozesses reißt die Anwendung nicht mit.</p></blockquote>
<h2>Scrollen</h2>
<p>Der WebView bleibt der einzige vertikale Scrollbesitzer. Wer zusätzlich ein <code>GtkScrolledWindow</code> darumlegt, erhält doppelte Kinetik — genau das verbietet sich.</p>
<table>
<thead><tr><th>Eigenschaft</th><th>API 4.1 (GTK 3)</th><th>API 6.0 (GTK 4)</th></tr></thead>
<tbody>
<tr><td>Netzwerk-Isolation</td><td>WebContext</td><td>NetworkSession (Pflicht)</td></tr>
<tr><td>Sandbox</td><td>optional</td><td>Standard</td></tr>
<tr><td>Rendering</td><td>Coordinator fehlt</td><td>Frame Clock integriert</td></tr>
</tbody>
</table>
<h2>International</h2>
<p class="lf-rtl">auch rechtsläufige Absätze müssen sauber umgebrochen werden — Richtung und Zeilenhöhe bleiben erhalten.</p>
<pre><code>let session = webkit6::NetworkSession::new_ephemeral();
let view = webkit6::WebView::builder()
    .network_session(&amp;session)
    .build();</code></pre>
{rest}"#,
            hero = HERO_SVG,
            rest = short_paras("WebKit", 3)
        ),
    );

    add("a-heise-2", "f-heise", "Kernel 6.17 verbessert Scheduler für Hybrid-CPUs", None, "https://example.com/k617", 5, "Die neue Kern-Scheduler-Logik verteilt Lasten gezielter zwischen Performance- und Effizienz-Kernen.", true, false, false, short_paras("Kernel", 4));
    add("a-heise-3", "f-heise", "BSI warnt vor aktiver Ausnutzung einer Lücke in cups", Some("Jo Stein"), "https://example.com/cups", 9, "Drucker-Dienste sollten abgeschirmt werden; Patches stehen bereit.", true, false, false, short_paras("Sicherheit", 3));
    add("a-heise-4", "f-heise", "Kommentar: Der Desktop-Jahrgang 2026 ist ein guter", Some("Ada Lorenz"), "https://example.com/kommentar", 28, "Wayland, Portale und schnelle Toolkits greifen endlich ineinander.", false, false, false, short_paras("Kommentar", 5));
    add("a-heise-5", "f-heise", "Rust 1.98 stabilisiert Async-Closures", None, "https://example.com/rust198", 31, "Async-Closures und weitere Qualitätsverbesserungen sind stabil.", true, false, false, format!("<pre><code>let fetch = async |url: &amp;str| {{ reqwest::get(url).await }};</code></pre>{}", short_paras("Rust", 3)));

    add("a-lob-1", "f-lobsters", "Show: A tiny Wayland compositor in 900 lines of Rust", Some("pixelpusher"), "https://example.com/tinycomp", 3, "Ein Lehrstück über wlroots, damage tracking und Eingaberouting.", true, false, true, short_paras("Compositor", 4));
    add("a-lob-2", "f-lobsters", "SQLite FTS5: Tokenizer richtig wählen", Some("dbmaven"), "https://example.com/fts5", 7, "Unicode61 vs. Trigramm — Messwerte für Suche mit Umlauten und CJK.", true, false, false, format!("<pre><code>CREATE VIRTUAL TABLE article_fts USING fts5(title, body, tokenize='unicode61 remove_diacritics 2');</code></pre>{}", short_paras("Datenbank", 3)));
    add("a-lob-3", "f-lobsters", "The hidden cost of double buffering", Some("gfxguru"), "https://example.com/buffering", 12, "Warum Frame-Budgets kippen, wenn Present-Zeitpunkte wandern.", true, false, false, short_paras("Grafik", 5));
    add("a-lob-4", "f-lobsters", "Ask: Which RSS reader do you use on Linux in 2026?", Some("curiouscat"), "https://example.com/askrss", 20, "Große Umfrage mit überraschend vielen GTK-Nennungen.", false, false, false, short_paras("Umfrage", 2));
    add("a-lob-5", "f-lobsters", "Keyset pagination beats OFFSET at scale", Some("indexwiz"), "https://example.com/keyset", 34, "Stabile Fenster über (published_at, id) statt Seitensprüngen.", true, false, false, format!("<pre><code>WHERE (published_at, id) &lt; (?, ?) ORDER BY published_at DESC, id DESC LIMIT 100;</code></pre>{}", short_paras("Paginierung", 3)));
    add("a-lob-6", "f-lobsters", "GTK 4.22 released with SVG widget", None, "https://example.com/gtk422", 50, "Das Toolkit bekommt native SVG-Darstellung und Feinschliff an Listen.", false, false, false, short_paras("GTK", 3));

    add("a-ts-1", "f-tagesschau", "Haushaltsdebatte: Investitionen und Schuldenbremse", Some("Anja Vogel"), "https://example.com/haushalt", 1, "Die Fraktionen streiten über den Etat; Abstimmung am Freitag.", true, false, true, short_paras("Politik", 4));
    add("a-ts-2", "f-tagesschau", "Wirtschaft: Exporte legen im August zu", None, "https://example.com/exporte", 6, "Das Statistische Bundesamt meldet ein Plus von 2,1 Prozent.", true, false, false, "<table><thead><tr><th>Monat</th><th>Veränderung</th></tr></thead><tbody><tr><td>Juni</td><td>+0,4 %</td></tr><tr><td>Juli</td><td>-1,2 %</td></tr><tr><td>August</td><td>+2,1 %</td></tr></tbody></table><p>Die Zahlen sind vorläufig und saisonbereinigt.</p>".to_string());
    add("a-ts-3", "f-tagesschau", "Unwetterwarnung für den Süden aufgehoben", None, "https://example.com/unwetter", 10, "Der Deutsche Wetterdienst gibt Entwarnung für die Alpenregion.", true, false, false, short_paras("Wetter", 2));
    add("a-ts-4", "f-tagesschau", "Analyse: Was das neue Klimagesetz vorsieht", Some("Tom Berger"), "https://example.com/klima", 26, "Sektorziele, Berichtspflichten und der Streit um die Nachsteuerung.", false, false, false, short_paras("Analyse", 5));
    add("a-ts-5", "f-tagesschau", "Bahn kündigt Winter-Fahrplan an", None, "https://example.com/bahn", 30, "Mehr ICE-Sprinter, weniger Nachtbaustellen ab Dezember.", true, false, false, short_paras("Verkehr", 3));
    add("a-ts-6", "f-tagesschau", "Kulturerbe: Anträge für immaterielle Liste", None, "https://example.com/kultur", 55, "Zwölf neue Vorschläge stehen zur Entscheidung.", false, false, false, short_paras("Kultur", 2));

    add("a-des-1", "f-design", "Typografie-Skala für Lese-Apps", Some("Lina Marsh"), "https://example.com/type-scale", 4, "Von 14 px UI-Text bis 36 px Überschrift: eine konsistente Skala bauen.", true, false, false, short_paras("Typografie", 4));
    add("a-des-2", "f-design", "Dichte ohne Unruhe: Informationsdesign für Artikellisten", Some("Ole Brandt"), "https://example.com/dichte", 14, "Warum feste Zeilenhöhen und reservierte Bildflächen Ruhe erzeugen.", true, false, true, short_paras("Listen", 5));
    add("a-des-3", "f-design", "Farbtokens: Ein System für GTK-CSS und Reader", Some("Lina Marsh"), "https://example.com/tokens", 29, "Semantische Namen statt Hex-Literale — in beiden Renderern.", false, true, false, short_paras("Farbe", 4));
    add("a-des-4", "f-design", "Fokus sichtbar machen, ohne zu schreien", None, "https://example.com/fokus", 48, "Fokusringe, die Kontrastregeln erfüllen und trotzdem dezent bleiben.", true, false, false, short_paras("A11y", 3));
    add("a-des-5", "f-design", "Reduzierte Bewegung respektieren", None, "https://example.com/motion", 72, "Ein Schalter, der Übergänge abschaltet, ohne Funktionen zu verlieren.", false, false, false, short_paras("Motion", 2));

    articles.sort_by(|a, b| b.published_at.cmp(&a.published_at).then(a.id.cmp(&b.id)));
    Library { groups, feeds, articles, contents }
}
