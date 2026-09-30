use html5ever::tendril::TendrilSink;
use html5ever::{local_name, Attribute};
use markup5ever_rcdom::{Handle, NodeData, RcDom, SerializableHandle};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;

pub struct CleanResult {
    pub html: String,
    pub images: Vec<String>,
}

const TAGS: &[&str] = &[
    "p",
    "div",
    "span",
    "br",
    "hr",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "dl",
    "dt",
    "dd",
    "blockquote",
    "pre",
    "code",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "th",
    "td",
    "caption",
    "figure",
    "figcaption",
    "img",
    "a",
    "em",
    "strong",
    "b",
    "i",
    "u",
    "s",
    "sub",
    "sup",
    "small",
    "mark",
    "abbr",
    "q",
    "cite",
    "kbd",
    "samp",
    "var",
    "time",
    "wbr",
    "ruby",
    "rt",
    "rp",
    "bdi",
    "bdo",
];

fn tag_attributes() -> HashMap<&'static str, HashSet<&'static str>> {
    let mut m: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
    m.insert("a", HashSet::from(["href", "title"]));
    m.insert(
        "img",
        HashSet::from(["src", "alt", "title", "width", "height"]),
    );
    m.insert("td", HashSet::from(["colspan", "rowspan"]));
    m.insert("th", HashSet::from(["colspan", "rowspan"]));
    m.insert("time", HashSet::from(["datetime"]));
    m.insert("abbr", HashSet::from(["title"]));
    m.insert("q", HashSet::from(["cite"]));
    m
}

fn resolve(base: Option<&url::Url>, value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }
    match base {
        Some(b) => b.join(v).ok().map(|u| u.to_string()),
        None => url::Url::parse(v).ok().map(|u| u.to_string()),
    }
}

fn walk(node: &Handle, base: Option<&url::Url>) {
    if let NodeData::Element { name, attrs, .. } = &node.data {
        let mut keep: Vec<Attribute> = Vec::new();
        for attr in attrs.borrow().iter() {
            let an = attr.name.local.as_ref();
            match (name.local.as_ref(), an) {
                ("a", "href") | ("img", "src") => {
                    if let Some(abs) = resolve(base, &attr.value) {
                        let scheme = abs.split(':').next().unwrap_or("").to_lowercase();
                        if scheme == "http" || scheme == "https" {
                            keep.push(Attribute {
                                name: attr.name.clone(),
                                value: abs.into(),
                            });
                        }
                    }
                }
                _ => keep.push(attr.clone()),
            }
        }
        *attrs.borrow_mut() = keep;
    }
    for child in node.children.borrow().iter() {
        walk(child, base);
    }
}

fn serialize_children(node: &Handle, out: &mut String) {
    let mut buf = Vec::new();
    let ser = SerializableHandle::from(node.clone());
    if html5ever::serialize(&mut buf, &ser, Default::default()).is_ok() {
        out.push_str(&String::from_utf8_lossy(&buf));
    }
}

fn find_body(node: &Handle) -> Option<Handle> {
    if let NodeData::Element { name, .. } = &node.data {
        if name.local == local_name!("body") {
            return Some(node.clone());
        }
    }
    for child in node.children.borrow().iter() {
        if let Some(b) = find_body(child) {
            return Some(b);
        }
    }
    None
}

pub fn sanitize(raw: &str, base_url: Option<&str>) -> CleanResult {
    let base = base_url.and_then(|b| url::Url::parse(b).ok());
    let dom: RcDom = html5ever::parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut Cursor::new(raw))
        .unwrap_or_default();
    walk(&dom.document, base.as_ref());
    let mut serialized = String::new();
    match find_body(&dom.document) {
        Some(body) => serialize_children(&body, &mut serialized),
        None => serialize_children(&dom.document, &mut serialized),
    }

    let mut builder = ammonia::Builder::new();
    builder
        .tags(TAGS.iter().copied().collect())
        .tag_attributes(tag_attributes())
        .generic_attributes(HashSet::new())
        .url_schemes(HashSet::from(["http", "https"]))
        .link_rel(Some("noopener noreferrer nofollow"));
    let cleaned = builder.clean(&serialized).to_string();

    let mut images = Vec::new();
    if let Ok(sel) = scraper::Selector::parse("img[src]") {
        let doc = scraper::Html::parse_fragment(&cleaned);
        for el in doc.select(&sel) {
            if let Some(src) = el.value().attr("src") {
                let s = src.to_string();
                if !images.contains(&s) {
                    images.push(s);
                }
            }
        }
    }
    CleanResult {
        html: cleaned,
        images,
    }
}

pub fn image_sources(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(sel) = scraper::Selector::parse("img[src]") {
        let doc = scraper::Html::parse_fragment(html);
        for el in doc.select(&sel) {
            if let Some(src) = el.value().attr("src") {
                let s = src.to_string();
                if (s.starts_with("http://") || s.starts_with("https://")) && !out.contains(&s) {
                    out.push(s);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_scripts_and_handlers() {
        let raw = r#"<div><script>alert(1)</script><p onclick="evil()">Text</p>
<iframe src="https://x.example"></iframe><img src="https://x.example/i.png" onerror="evil()">
<a href="javascript:alert(1)">click</a><meta http-equiv="refresh" content="0;url=x">
<style>body{display:none}</style><object data="x"></object><embed src="x">
<p style="background:url(javascript:evil)">ok</p></div>"#;
        let r = sanitize(raw, Some("https://x.example/feed"));
        assert!(!r.html.contains("script"), "{}", r.html);
        assert!(!r.html.contains("onclick"));
        assert!(!r.html.contains("onerror"));
        assert!(!r.html.contains("iframe"));
        assert!(!r.html.contains("javascript:"));
        assert!(!r.html.contains("<style"));
        assert!(!r.html.contains("<object"));
        assert!(!r.html.contains("<embed"));
        assert!(!r.html.contains("style="));
        assert!(r.html.contains("Text"));
        assert!(r.html.contains("ok"));
        assert!(r.images.contains(&"https://x.example/i.png".to_string()));
    }

    #[test]
    fn resolves_relative_urls() {
        let raw = r#"<img src="/img/a.png"><a href="../b.html">b</a><img src="c.png">"#;
        let r = sanitize(raw, Some("https://blog.example/x/feed.xml"));
        assert!(r
            .images
            .contains(&"https://blog.example/img/a.png".to_string()));
        assert!(r
            .images
            .contains(&"https://blog.example/x/c.png".to_string()));
        assert!(r.html.contains("https://blog.example/b.html"));
    }

    #[test]
    fn keeps_structure() {
        let raw = r#"<h2>T</h2><ul><li>eins</li></ul><blockquote>zitat</blockquote>
<pre><code>code</code></pre><table><tr><th>a</th><td>b</td></tr></table>"#;
        let r = sanitize(raw, None);
        for needle in [
            "<h2>",
            "<ul>",
            "<li>",
            "<blockquote>",
            "<pre>",
            "<code>",
            "<table>",
            "<th>",
            "<td>",
        ] {
            assert!(r.html.contains(needle), "fehlt {needle} in {}", r.html);
        }
    }

    #[test]
    fn link_rel_hardening() {
        let r = sanitize(r#"<a href="https://a.example">a</a>"#, None);
        assert!(
            r.html.contains("rel=\"noopener noreferrer nofollow\""),
            "{}",
            r.html
        );
    }
}

/// Ersetzt **nur** das `src`-Attribut innerhalb von `<img>`-Tags durch einen
/// Platzhalter und merkt die Originaladresse in `data-lf-src`.
/// Text, Linkziele und andere Attribute bleiben unangetastet, weil nur der
/// Bereich zwischen `<img` und dem zugehörigen `>` verändert wird.
pub fn rewrite_images(html: &str, placeholder: &str) -> String {
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let rest = &html[i..];
        if !rest
            .get(..4)
            .map(|t| t.eq_ignore_ascii_case("<img"))
            .unwrap_or(false)
        {
            let ch_len = rest.chars().next().map(char::len_utf8).unwrap_or(1);
            out.push_str(&rest[..ch_len]);
            i += ch_len;
            continue;
        }
        let tag_end = match rest.find('>') {
            Some(e) => i + e,
            None => {
                out.push_str(rest);
                break;
            }
        };
        let tag = &html[i..tag_end];
        // `span` umfasst das **vollständige** Attribut inklusive `="…"`; bliebe
        // nur der Name übergangen, stünde hinter dem neuen `src` der Wert des
        // Originals als unparsbares Attributrest im Tag.
        match attr_span(tag, "src") {
            Some((start, end))
                if !attr_value(tag, "src").is_some_and(|v| v.starts_with("data:")) =>
            {
                let value = attr_value(tag, "src").unwrap_or_default();
                let mut replaced = String::with_capacity(tag.len() + value.len() + 24);
                replaced.push_str(&tag[..start]);
                replaced.push_str(&format!(
                    "data-lf-src=\"{value}\" src=\"{}\"",
                    escape_attr(placeholder)
                ));
                replaced.push_str(&tag[end..]);
                out.push_str(&replaced);
            }
            _ => out.push_str(tag),
        }
        i = tag_end;
    }
    out
}

fn tag_lower_find(tag_lower: &str, attr: &str) -> Option<usize> {
    let needle = format!(" {attr}");
    tag_lower.find(&needle).map(|i| i + 1)
}

/// Byte-Bereich des vollständigen Attributs `name="…"` im Tag: Beginn am
/// Attributnamen, Ende hinter dem schließenden Anführungszeichen.
fn attr_span(tag: &str, name: &str) -> Option<(usize, usize)> {
    let start = tag_lower_find(&tag.to_ascii_lowercase(), name)?;
    let mut i = start + name.len();
    i += tag[i..].len() - tag[i..].trim_start().len();
    if !tag[i..].starts_with('=') {
        return None;
    }
    i += 1;
    i += tag[i..].len() - tag[i..].trim_start().len();
    let quote = tag[i..].chars().next()?;
    if quote == '"' || quote == '\'' {
        let rel = tag[i + 1..].find(quote)?;
        Some((start, i + 1 + rel + 1))
    } else {
        let rel = tag[i..]
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(tag.len() - i);
        Some((start, i + rel))
    }
}

/// Attributwert **so, wie er im HTML steht** – also noch zeichenreferenz-
/// kodiert. Ein erneutes `escape_attr` würde `&amp;` zu `&amp;amp;` verdoppeln.
fn attr_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let (start, _) = attr_span(tag, name)?;
    let after = &tag[start + name.len()..];
    let after = after.trim_start().strip_prefix('=')?.trim_start();
    let quote = after.chars().next()?;
    if quote == '"' || quote == '\'' {
        let rel = after[1..].find(quote)?;
        Some(&after[1..1 + rel])
    } else {
        Some(after)
    }
}

fn escape_attr(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;")
}

/// Setzt für Bilder mit passendem `data-lf-src` die Datenquelle;
/// berührt ausschließlich das `src`-Attribut von `<img>`.
pub fn replace_marker(html: &str, url: &str, data_uri: &str) -> String {
    rewrite_images_one(html, url, data_uri)
}

fn rewrite_images_one(html: &str, url: &str, data_uri: &str) -> String {
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let rest = &html[i..];
        if !rest
            .get(..4)
            .map(|t| t.eq_ignore_ascii_case("<img"))
            .unwrap_or(false)
        {
            let ch_len = rest.chars().next().map(char::len_utf8).unwrap_or(1);
            out.push_str(&rest[..ch_len]);
            i += ch_len;
            continue;
        }
        let tag_end = match rest.find('>') {
            Some(e) => i + e,
            None => {
                out.push_str(rest);
                break;
            }
        };
        let tag = &html[i..tag_end];
        // `data-lf-src` steht im HTML zeichenreferenz-kodiert, die gesuchte
        // Adresse kommt aus dem DOM (dekodiert). Der Vergleich muss daher beide
        // Seiten in dieselbe Form bringen, sonst trifft er URLs mit `&` nie.
        let hit = attr_value(tag, "data-lf-src") == Some(escape_attr(url).as_str());
        match (hit, attr_span(tag, "src")) {
            (true, Some((start, end))) => {
                let mut rebuilt = String::with_capacity(tag.len() + data_uri.len());
                rebuilt.push_str(&tag[..start]);
                rebuilt.push_str(&format!("src=\"{}\"", escape_attr(data_uri)));
                rebuilt.push_str(&tag[end..]);
                out.push_str(&rebuilt);
            }
            _ => out.push_str(tag),
        }
        i = tag_end;
    }
    out
}

pub fn image_alt_texts(html: &str) -> Vec<(String, String)> {
    let Ok(selector) = scraper::Selector::parse("img[data-lf-src]") else {
        return Vec::new();
    };
    scraper::Html::parse_fragment(html)
        .select(&selector)
        .filter_map(|el| {
            el.value().attr("data-lf-src").map(|src| {
                (
                    src.to_string(),
                    el.value().attr("alt").unwrap_or("Bild").to_string(),
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod image_tests {
    use super::*;

    #[test]
    fn only_img_src_is_replaced() {
        let html = r#"<p>Text mit https://cdn.example/a.png als Linktext</p>
            <a href="https://cdn.example/a.png">Link</a>
            <img src="https://cdn.example/a.png" alt="Bild">"#;
        let out = rewrite_images(html, "data:image/gif;base64,R0lGODlhAQABAAAAACw=");
        assert!(!out.contains("Linktext mit https://cdn.example/a.png"));
        assert!(
            out.contains(r#"<a href="https://cdn.example/a.png">"#),
            "Linkziel bleibt"
        );
        assert!(out.contains(r#"data-lf-src="https://cdn.example/a.png""#));
        assert!(out.contains(r#"src="data:image/gif;base64,R0lGODlhAQABAAAAACw=""#));
        let alts = image_alt_texts(&out);
        assert_eq!(
            alts,
            vec![("https://cdn.example/a.png".to_string(), "Bild".to_string())]
        );
    }

    /// Der Attributwert des Originals darf nicht als Reste-Attribut hinter dem
    /// neuen `src` stehen bleiben: `src="…"="…"` ist unparsbar und machte das
    /// Bild unsichtbar, obwohl der Test oben nur das Teil-Substring sah.
    #[test]
    fn rewrite_leaves_no_attribute_remains() {
        let html = r#"<div><img width="720" height="425" src="https://cdn.example/a.png" alt="Bild"></div>"#;
        let out = rewrite_images(html, "PH");
        assert!(!out.contains(r#""="https://cdn.example/a.png""#), "{out}");
        assert!(
            out.contains(r#"<img width="720" height="425" data-lf-src="https://cdn.example/a.png" src="PH" alt="Bild">"#),
            "{out}"
        );
    }

    /// `data-lf-src` muss genau der Wert sein, den das DOM später liefert,
    /// sonst findet der Medien-Nachlader das Bild nicht. Das HTML aus dem Feed
    /// ist bereits zeichenreferenz-kodiert; ein erneutes Kodieren erzeugte
    /// `&amp;amp;` und damit eine Adresse, die nirgends vorkommt.
    #[test]
    fn lf_src_matches_the_dom_value() {
        let dom = "https://cdn.example/a.png?x=1&y=2";
        let raw = r#"<img src="https://cdn.example/a.png?x=1&amp;y=2" alt="q">"#;
        let out = rewrite_images(raw, "PH");
        assert!(!out.contains("&amp;amp;"), "keine Doppelkodierung: {out}");
        let alts = image_alt_texts(&out);
        assert_eq!(alts, vec![(dom.to_string(), "q".to_string())]);
    }

    #[test]
    fn replace_marker_matches_ampersand_urls() {
        let dom = "https://cdn.example/a.png?x=1&y=2";
        let raw = r#"<img src="https://cdn.example/a.png?x=1&amp;y=2" alt="q">"#;
        let out = replace_marker(&rewrite_images(raw, "PH"), dom, "data:image/png;base64,AAA");
        assert!(out.contains(r#"src="data:image/png;base64,AAA""#), "{out}");
        assert!(!out.contains(r#""="https://cdn.example/a.png"#), "{out}");
    }

    #[test]
    fn marker_is_replaced_only_for_the_matching_image() {
        let html = r#"<img data-lf-src="https://a.example/1.png" src="PH" alt="eins">
            <img data-lf-src="https://a.example/2.png" src="PH" alt="zwei">"#;
        let out = replace_marker(html, "https://a.example/1.png", "data:image/png;base64,AAA");
        assert!(out.contains(r#"src="data:image/png;base64,AAA""#));
        assert!(
            out.contains(r#"data-lf-src="https://a.example/2.png" src="PH""#),
            "zweites Bild bleibt"
        );
    }

    #[test]
    fn inline_data_images_are_left_alone() {
        let html = r#"<img src="data:image/png;base64,iVBORw0KGgo=" alt="x">"#;
        let out = rewrite_images(html, "data:image/gif;base64,R0lGODlhAQABAAAAACw=");
        assert!(out.contains("data:image/png;base64"));
        assert!(!out.contains("data-lf-src"));
    }
}
