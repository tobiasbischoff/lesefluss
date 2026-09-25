pub mod sanitize;
pub mod tokens;

use tokens::Tokens;

pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub struct ReaderDocument<'a> {
    pub kicker: &'a str,
    pub title: &'a str,
    pub author: Option<&'a str>,
    pub source: &'a str,
    pub published: &'a str,
    pub content_html: &'a str,
    /// Kennung des Dokuments; verhindert, dass verspätete Ergebnisse die
    /// Leseposition eines anderen Artikels überschreiben.
    pub generation: u64,
}

pub struct ReaderStyle {
    pub font_size: f64,
    pub measure_ch: u32,
    pub line_height: f64,
}

impl Default for ReaderStyle {
    fn default() -> Self {
        Self {
            font_size: 18.0,
            measure_ch: 68,
            line_height: 1.6,
        }
    }
}

pub fn render_document(doc: &ReaderDocument, tokens: &Tokens, style: &ReaderStyle) -> String {
    let mut meta = String::new();
    if !doc.source.is_empty() {
        meta.push_str(&format!("<span>{}</span>", escape_html(doc.source)));
    }
    if let Some(a) = doc.author.filter(|a| !a.is_empty()) {
        meta.push_str(&format!("<span>{}</span>", escape_html(a)));
    }
    if !doc.published.is_empty() {
        meta.push_str(&format!("<span>{}</span>", escape_html(doc.published)));
    }
    format!(
        r#"<!doctype html>
<html lang="de">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:;">
<meta name="lf-doc" content="{generation}">
<style>{css}</style>
</head>
<body>
<div class="lf-wrap">
<header class="lf-head">
<p class="lf-kicker">{kicker}</p>
<h1 class="lf-title">{title}</h1>
<p class="lf-meta">{meta}</p>
</header>
<article class="lf-body">
{content}
</article>
</div>
</body>
</html>"#,
        css = tokens.reader_css(style.font_size, style.measure_ch, style.line_height),
        kicker = escape_html(doc.kicker),
        title = escape_html(doc.title),
        meta = meta,
        content = doc.content_html,
        generation = doc.generation,
    )
}
