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
    "p", "div", "span", "br", "hr", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "dl",
    "dt", "dd", "blockquote", "pre", "code", "table", "thead", "tbody", "tfoot", "tr", "th", "td",
    "caption", "figure", "figcaption", "img", "a", "em", "strong", "b", "i", "u", "s", "sub",
    "sup", "small", "mark", "abbr", "q", "cite", "kbd", "samp", "var", "time", "wbr", "ruby",
    "rt", "rp", "bdi", "bdo",
];

fn tag_attributes() -> HashMap<&'static str, HashSet<&'static str>> {
    let mut m: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
    m.insert("a", HashSet::from(["href", "title"]));
    m.insert("img", HashSet::from(["src", "alt", "title", "width", "height"]));
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
    CleanResult { html: cleaned, images }
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
        assert!(r.images.contains(&"https://blog.example/img/a.png".to_string()));
        assert!(r.images.contains(&"https://blog.example/x/c.png".to_string()));
        assert!(r.html.contains("https://blog.example/b.html"));
    }

    #[test]
    fn keeps_structure() {
        let raw = r#"<h2>T</h2><ul><li>eins</li></ul><blockquote>zitat</blockquote>
<pre><code>code</code></pre><table><tr><th>a</th><td>b</td></tr></table>"#;
        let r = sanitize(raw, None);
        for needle in ["<h2>", "<ul>", "<li>", "<blockquote>", "<pre>", "<code>", "<table>", "<th>", "<td>"] {
            assert!(r.html.contains(needle), "fehlt {needle} in {}", r.html);
        }
    }

    #[test]
    fn link_rel_hardening() {
        let r = sanitize(r#"<a href="https://a.example">a</a>"#, None);
        assert!(r.html.contains("rel=\"noopener noreferrer nofollow\""), "{}", r.html);
    }
}
