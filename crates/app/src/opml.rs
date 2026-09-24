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
            return a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.to_string());
        }
    }
    None
}

pub fn parse_opml(xml: &str) -> Result<OpmlDraft, String> {
    if xml.len() > MAX_OPML_BYTES {
        return Err(format!("Datei zu groß ({} Bytes, Limit {MAX_OPML_BYTES})", xml.len()));
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut group_stack: Vec<String> = Vec::new();
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
                let text = attr(&e, "text").or_else(|| attr(&e, "title")).unwrap_or_default();
                match attr(&e, "xmlUrl") {
                    Some(url) if !url.trim().is_empty() => {
                        draft.feeds.push(make_feed(text, url, &e, &group_stack));
                    }
                    _ => {
                        if !text.is_empty() && group_stack.len() < MAX_DEPTH {
                            group_stack.push(text);
                        } else if group_stack.len() >= MAX_DEPTH {
                            draft.errors.push(format!("Zu tiefe Verschachtelung bei Outline {outlines}"));
                        }
                    }
                }
            }
            Ok(Event::Empty(e)) if e.local_name().as_ref() == "outline" => {
                outlines += 1;
                if outlines > MAX_OUTLINES {
                    return Err(format!("Mehr als {MAX_OUTLINES} Outlines"));
                }
                let text = attr(&e, "text").or_else(|| attr(&e, "title")).unwrap_or_default();
                if let Some(url) = attr(&e, "xmlUrl") {
                    if !url.trim().is_empty() {
                        draft.feeds.push(make_feed(text, url, &e, &group_stack));
                    }
                }
            }
            Ok(Event::End(e)) if e.local_name().as_ref() == "outline" => {
                group_stack.pop();
            }
            Err(err) => return Err(format!("XML-Fehler: {err}")),
            _ => {}
        }
        buf.clear();
    }
    if draft.feeds.is_empty() && draft.errors.is_empty() {
        return Err("Keine Feed-Outlines gefunden".into());
    }
    Ok(draft)
}

fn make_feed(text: String, url: String, e: &quick_xml::events::BytesStart, groups: &[String]) -> OpmlFeed {
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
        while depth < open_groups.len() && depth < f.groups.len() && open_groups[depth] == f.groups[depth] {
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
        assert_eq!(d.feeds[0].html_url.as_deref(), Some("https://www.heise.de/"));
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
    fn broken_xml() {
        let r = parse_opml("<opml><body><outline text=\"x\" xmlUrl=\"https://a/</opml>");
        assert!(r.is_err());
        let r2 = parse_opml("<opml><body></body></opml>");
        assert!(r2.is_err());
    }
}
