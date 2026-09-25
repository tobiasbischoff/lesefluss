use quick_xml::events::Event;
use quick_xml::Reader;

pub const MAX_OPML_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_OUTLINES: usize = 20_000;
pub const MAX_DEPTH: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub struct OpmlFeed {
    pub title: String,
    pub xml_url: String,
    pub html_url: Option<String>,
    pub groups: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct OpmlDraft {
    pub feeds: Vec<OpmlFeed>,
    pub errors: Vec<String>,
}

fn attr(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    for a in e.attributes().flatten() {
        if a.key.local_name().as_ref() == name {
            return a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|v| v.to_string());
        }
    }
    None
}

pub fn parse_opml(xml: &str) -> Result<OpmlDraft, String> {
    if xml.len() > MAX_OPML_BYTES {
        return Err(format!(
            "Datei zu groß ({} Bytes, Limit {MAX_OPML_BYTES})",
            xml.len()
        ));
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut group_stack: Vec<String> = Vec::new();
    let mut open_groups: Vec<bool> = Vec::new();
    let mut draft = OpmlDraft::default();
    let mut outlines = 0usize;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) if e.local_name().as_ref() == "outline" => {
                outlines += 1;
                if outlines > MAX_OUTLINES {
                    return Err(format!("Mehr als {MAX_OUTLINES} Outlines"));
                }
                let text = attr(&e, "text")
                    .or_else(|| attr(&e, "title"))
                    .unwrap_or_default();
                match attr(&e, "xmlUrl") {
                    Some(url) if !url.trim().is_empty() => {
                        draft.feeds.push(make_feed(text, url, &e, &group_stack));
                        open_groups.push(false);
                    }
                    _ => {
                        if group_stack.len() >= MAX_DEPTH {
                            return Err(format!(
                                "Verschachtelung tiefer als {MAX_DEPTH} bei Outline {outlines}"
                            ));
                        }
                        if text.is_empty() {
                            draft.errors.push(format!("Outline {outlines} ohne Namen"));
                        }
                        group_stack.push(text);
                        open_groups.push(true);
                    }
                }
            }
            Ok(Event::Empty(e)) if e.local_name().as_ref() == "outline" => {
                outlines += 1;
                if outlines > MAX_OUTLINES {
                    return Err(format!("Mehr als {MAX_OUTLINES} Outlines"));
                }
                let text = attr(&e, "text")
                    .or_else(|| attr(&e, "title"))
                    .unwrap_or_default();
                if let Some(url) = attr(&e, "xmlUrl") {
                    if !url.trim().is_empty() {
                        draft.feeds.push(make_feed(text, url, &e, &group_stack));
                    }
                }
            }
            Ok(Event::End(e)) if e.local_name().as_ref() == "outline" => match open_groups.pop() {
                Some(true) => {
                    group_stack.pop();
                }
                Some(false) => {}
                None => {
                    return Err("Unbalancedes Outline: schließendes Element ohne Öffnung".into())
                }
            },
            Err(err) => return Err(format!("XML-Fehler: {err}")),
            _ => {}
        }
        buf.clear();
    }
    if !open_groups.is_empty() {
        return Err(format!(
            "Datei endet mit {} offenen Outline-Elementen",
            open_groups.len()
        ));
    }
    if draft.feeds.is_empty() && draft.errors.is_empty() {
        return Err("Keine Feed-Outlines gefunden".into());
    }
    Ok(draft)
}

fn make_feed(
    text: String,
    url: String,
    e: &quick_xml::events::BytesStart,
    groups: &[String],
) -> OpmlFeed {
    let title = if text.is_empty() {
        url::Url::parse(&url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_else(|| url.clone())
    } else {
        text
    };
    OpmlFeed {
        title,
        xml_url: url.trim().to_string(),
        html_url: attr(e, "htmlUrl"),
        groups: groups.to_vec(),
    }
}

pub fn build_opml(feeds: &[OpmlFeed]) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<opml version=\"2.0\">\n<head>\n<title>Lesefluss Abonnements</title>\n</head>\n<body>\n");
    let mut open_groups: Vec<String> = Vec::new();
    for f in feeds {
        let mut depth = 0usize;
        while depth < open_groups.len()
            && depth < f.groups.len()
            && open_groups[depth] == f.groups[depth]
        {
            depth += 1;
        }
        while open_groups.len() > depth {
            out.push_str("</outline>\n");
            open_groups.pop();
        }
        while open_groups.len() < f.groups.len() {
            let g = f.groups[open_groups.len()].clone();
            out.push_str(&format!(
                "<outline text=\"{}\" title=\"{}\">\n",
                escape(&g),
                escape(&g)
            ));
            open_groups.push(g);
        }
        out.push_str(&format!(
            "<outline text=\"{}\" title=\"{}\" type=\"rss\" xmlUrl=\"{}\"{}/>\n",
            escape(&f.title),
            escape(&f.title),
            escape(&f.xml_url),
            match &f.html_url {
                Some(h) => format!(" htmlUrl=\"{}\"", escape(h)),
                None => String::new(),
            }
        ));
    }
    while !open_groups.is_empty() {
        out.push_str("</outline>\n");
        open_groups.pop();
    }
    out.push_str("</body>\n</opml>\n");
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
<head><title>Meine Feeds</title></head>
<body>
<outline text="Technik">
  <outline text="heise" title="heise" type="rss" xmlUrl="https://www.heise.de/rss/feed.xml" htmlUrl="https://www.heise.de/"/>
  <outline text="Lobsters" type="rss" xmlUrl="https://lobste.rs/rss"/>
</outline>
<outline text="Einzelfeed" type="rss" xmlUrl="https://example.com/feed&amp;x=1"/>
<outline text="kaputt"/>
</body>
</opml>"#;

    #[test]
    fn parse_groups_and_feeds() {
        let d = parse_opml(SAMPLE).unwrap();
        assert_eq!(d.feeds.len(), 3);
        assert_eq!(d.feeds[0].groups, vec!["Technik".to_string()]);
        assert_eq!(d.feeds[0].xml_url, "https://www.heise.de/rss/feed.xml");
        assert_eq!(
            d.feeds[0].html_url.as_deref(),
            Some("https://www.heise.de/")
        );
        assert_eq!(d.feeds[2].groups.len(), 0);
        assert_eq!(d.feeds[2].xml_url, "https://example.com/feed&x=1");
    }

    #[test]
    fn roundtrip() {
        let d = parse_opml(SAMPLE).unwrap();
        let xml = build_opml(&d.feeds);
        let d2 = parse_opml(&xml).unwrap();
        assert_eq!(d.feeds, d2.feeds);
    }

    #[test]
    fn group_after_feed_outline_stays_open() {
        let xml = r#"<opml version="2.0"><body>
            <outline type="rss" text="Erster Feed" xmlUrl="https://a.example/feed.xml"></outline>
            <outline text="Gruppe">
                <outline type="rss" text="Feed in Gruppe" xmlUrl="https://b.example/feed.xml"></outline>
            </outline>
        </body></opml>"#;
        let draft = parse_opml(xml).expect("parse");
        assert_eq!(draft.feeds.len(), 2);
        assert!(
            draft.feeds[0].groups.is_empty(),
            "Feed vor der Gruppe bleibt ohne Gruppe"
        );
        assert_eq!(draft.feeds[1].groups, vec!["Gruppe".to_string()]);
    }

    #[test]
    fn self_closing_feed_outline_does_not_break_the_stack() {
        let xml = r#"<opml version="2.0"><body>
            <outline text="Leere Gruppe"/>
            <outline type="rss" text="Feed" xmlUrl="https://a.example/feed.xml"/>
            <outline text="Gruppe">
                <outline type="rss" text="Zweiter" xmlUrl="https://b.example/feed.xml"/>
            </outline>
        </body></opml>"#;
        let draft = parse_opml(xml).expect("parse");
        assert_eq!(draft.feeds.len(), 2);
        assert!(
            draft.feeds[0].groups.is_empty(),
            "leere Gruppe hat keine Kinder"
        );
        assert_eq!(draft.feeds[1].groups, vec!["Gruppe".to_string()]);
    }

    #[test]
    fn unbalanced_and_too_deep_documents_are_rejected() {
        let deep: String = (0..40)
            .map(|i| format!("<outline text=\"E{i}\">"))
            .collect::<String>()
            + "<outline type=\"rss\" text=\"F\" xmlUrl=\"https://x.example/f.xml\"/>"
            + &"</outline>".repeat(40);
        let xml = format!("<opml version=\"2.0\"><body>{deep}</body></opml>");
        let err = parse_opml(&xml).unwrap_err();
        assert!(err.contains("Verschachtelung"), "{err}");

        let broken = "<opml version=\"2.0\"><body><outline text=\"G\">";
        let err = parse_opml(broken).unwrap_err();
        assert!(
            err.contains("offenen") || err.contains("Unbalanced"),
            "{err}"
        );
    }

    #[test]
    fn feed_without_name_falls_back_to_host_and_unnamed_group_is_reported() {
        let xml = r#"<opml version="2.0"><body><outline xmlUrl="https://a.example/feed.xml"/></body></opml>"#;
        let draft = parse_opml(xml).expect("parse");
        assert_eq!(draft.feeds.len(), 1);
        assert_eq!(draft.feeds[0].title, "a.example");
        assert!(
            draft.errors.is_empty(),
            "Feed ohne Namen ist erlaubt: {:?}",
            draft.errors
        );

        let xml = r#"<opml version="2.0"><body><outline><outline type="rss" text="F" xmlUrl="https://a.example/f.xml"/></outline></body></opml>"#;
        let draft = parse_opml(xml).expect("parse");
        assert!(
            draft.errors.iter().any(|e| e.contains("ohne Namen")),
            "{:?}",
            draft.errors
        );
    }

    #[test]
    fn broken_xml() {
        let r = parse_opml("<opml><body><outline text=\"x\" xmlUrl=\"https://a/</opml>");
        assert!(r.is_err());
        let r2 = parse_opml("<opml><body></body></opml>");
        assert!(r2.is_err());
    }
}
