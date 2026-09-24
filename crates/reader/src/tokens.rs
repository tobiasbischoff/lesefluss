#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    const fn hex(v: u32) -> Self {
        Self { r: ((v >> 16) & 0xFF) as u8, g: ((v >> 8) & 0xFF) as u8, b: (v & 0xFF) as u8 }
    }
    pub fn css(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
    pub fn rgba(self, a: f64) -> String {
        format!("rgba({},{},{},{a:.3})", self.r, self.g, self.b)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    pub surface_reader: Color,
    pub surface_list: Color,
    pub surface_raised: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub separator: Color,
    pub accent: Color,
    pub selection: Color,
    pub focus_ring: Color,
    pub sidebar_top: Color,
    pub sidebar_bottom: Color,
}

impl Tokens {
    pub fn dark() -> Self {
        Self {
            dark: true,
            surface_reader: Color::hex(0x1C1D20),
            surface_list: Color::hex(0x202125),
            surface_raised: Color::hex(0x2A2C31),
            text_primary: Color::hex(0xF0F0F2),
            text_secondary: Color::hex(0xB8BAC2),
            separator: Color::hex(0x34363D),
            accent: Color::hex(0xB9AAFF),
            selection: Color::hex(0x3B374A),
            focus_ring: Color::hex(0xD3C8FF),
            sidebar_top: Color::hex(0x55283F),
            sidebar_bottom: Color::hex(0x2B2946),
        }
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            surface_reader: Color::hex(0xFAF9F6),
            surface_list: Color::hex(0xF2F1ED),
            surface_raised: Color::hex(0xFFFFFF),
            text_primary: Color::hex(0x222329),
            text_secondary: Color::hex(0x60636D),
            separator: Color::hex(0xD9DADE),
            accent: Color::hex(0x6042B8),
            selection: Color::hex(0xE8E0FA),
            focus_ring: Color::hex(0x6141BF),
            sidebar_top: Color::hex(0xE8E0FA),
            sidebar_bottom: Color::hex(0xF2F1ED),
        }
    }

    pub fn gtk_css(self, sidebar_gradient: bool) -> String {
        let sidebar_bg = if sidebar_gradient {
            format!(
                "linear-gradient(180deg, {} 0%, {} 100%)",
                self.sidebar_top.css(),
                self.sidebar_bottom.css()
            )
        } else {
            format!("none, {}", self.surface_list.css())
        };
        let sidebar_bg_decl = if sidebar_gradient {
            format!("background-image: {}; background-color: transparent;", sidebar_bg)
        } else {
            format!("background-color: {};", self.surface_list.css())
        };
        format!(
            r#"
.lf-window {{ background-color: {sr}; color: {tp}; }}
.lf-pane-bg {{ background-color: {sr}; }}
.lf-list-bg {{ background-color: {sl}; }}
.lf-sidebar {{ {sidebar_bg_decl} color: {tp}; }}
.lf-sidebar separator {{ background-color: {sep}; opacity: 0.4; }}
.lf-sidebar-row {{ padding: 6px 12px; min-height: 24px; border-radius: 8px; margin: 1px 8px; }}
.lf-sidebar-row:hover {{ background-color: {sel}; }}
.lf-sidebar-row:selected {{ background-color: {sel}; }}
.lf-sidebar-row:selected label {{ color: {tp}; font-weight: 600; }}
.lf-sidebar-section {{ font-size: 11px; font-weight: 700; color: {ts}; padding: 12px 20px 4px; letter-spacing: 0.04em; }}
.lf-badge {{ font-size: 11px; color: {ts}; font-variant-numeric: tabular-nums; }}
.lf-badge-unread {{ font-size: 11px; color: {tp}; font-weight: 700; font-variant-numeric: tabular-nums; }}
.lf-list {{ background-color: {sl}; }}
listview.lf-articles {{ background-color: transparent; }}
listview.lf-articles > row {{ background: transparent; padding: 0; margin: 0; }}
listview.lf-articles > row:selected {{ background-color: {sel}; border-radius: 8px; }}
listview.lf-articles > row:selected .lf-article-row {{ background-color: {sel}; border-radius: 8px; box-shadow: inset 3px 0 0 0 {ac}; }}
.lf-article-row {{ padding: 12px 16px; min-height: 92px; border-bottom: 1px solid {sep2}; }}
.lf-article-row:selected, listview.lf-articles > row:selected .lf-article-row {{ background-color: {sel}; border-radius: 8px; }}
.lf-article-title {{ font-size: 14px; font-weight: 400; }}
.lf-article-title-read {{ font-size: 14px; font-weight: 400; color: {ts2}; }}
.lf-article-meta-read {{ font-size: 11px; color: {ts2}; opacity: 0.85; }}
.lf-article-excerpt-read {{ font-size: 12px; color: {ts2}; opacity: 0.85; }}
.lf-article-title-unread {{ font-size: 14.5px; font-weight: 600; }}
.lf-article-meta {{ font-size: 11px; color: {ts}; }}
.lf-article-excerpt {{ font-size: 12px; color: {ts}; }}
.lf-thumb {{ min-width: 64px; min-height: 64px; border-radius: 6px; }}
.lf-day-header {{ font-size: 12px; font-weight: 700; color: {ts}; padding: 14px 16px 2px; background-color: {sl}; }}
.lf-status-icon {{ color: {ts}; opacity: 0.6; }}
.lf-separator {{ background-color: {sep}; min-width: 1px; }}
headerbar {{ box-shadow: none; background-color: transparent; }}
.lf-sidebar headerbar {{ background-color: transparent; }}
searchbar > revealer > box {{ background-color: {sl}; border-color: {sep}; box-shadow: none; }}
"#,
            sr = self.surface_reader.css(),
            sl = self.surface_list.css(),
            tp = self.text_primary.css(),
            ts = self.text_secondary.css(),
            ts2 = self.text_secondary.rgba(0.9),
                        sep = self.separator.css(),
            sep2 = self.separator.rgba(0.55),
            sel = self.selection.css(),
            sidebar_bg_decl = sidebar_bg_decl,
        )
    }

    pub fn reader_css(self, font_size: f64, measure_ch: u32, line_height: f64) -> String {
        format!(
            r#"
:root {{ color-scheme: {scheme}; }}
* {{ box-sizing: border-box; }}
html, body {{ margin: 0; padding: 0; }}
body {{
  background: {sr}; color: {tp};
  font-family: 'Cantarell', 'Inter', system-ui, sans-serif;
  font-size: {fs}px; line-height: {lh};
  -webkit-user-select: text; user-select: text;
}}
.lf-wrap {{ max-width: min({ch}ch, 760px); margin: 0 auto; padding: 40px 56px 96px; }}
@media (max-width: 640px) {{ .lf-wrap {{ padding: 24px 20px 72px; }} }}
header.lf-head {{ margin-bottom: 32px; }}
.lf-kicker {{ font-size: 12px; font-weight: 600; letter-spacing: 0.06em; text-transform: uppercase; color: {ac}; margin: 0 0 12px; }}
h1.lf-title {{ font-size: clamp(26px, 4vw, 36px); line-height: 1.15; font-weight: 700; margin: 0 0 12px; }}
.lf-meta {{ font-size: 13px; color: {ts}; margin: 0; }}
.lf-meta span + span::before {{ content: " · "; }}
article.lf-body h2 {{ font-size: 1.35em; margin: 1.6em 0 0.5em; }}
article.lf-body h3 {{ font-size: 1.15em; margin: 1.4em 0 0.4em; }}
article.lf-body p {{ margin: 0 0 1.1em; }}
article.lf-body a {{ color: {ac}; text-decoration: underline; text-underline-offset: 2px; }}
article.lf-body img {{ max-width: 100%; height: auto; border-radius: 6px; display: block; margin: 1.2em auto; }}
article.lf-body figure {{ margin: 1.4em 0; }}
article.lf-body figcaption {{ font-size: 0.8em; color: {ts}; margin-top: 8px; }}
article.lf-body blockquote {{ margin: 1.3em 0; padding: 4px 0 4px 20px; border-left: 3px solid {ac}; color: {ts}; font-style: italic; }}
article.lf-body pre {{ background: {rl}; border: 1px solid {sep}; border-radius: 8px; padding: 14px 16px; overflow-x: auto; font-size: 0.85em; line-height: 1.5; }}
article.lf-body code {{ font-family: 'JetBrains Mono', 'Fira Code', monospace; font-size: 0.9em; }}
article.lf-body :not(pre) > code {{ background: {rl}; border-radius: 4px; padding: 1px 5px; }}
article.lf-body ul, article.lf-body ol {{ padding-left: 1.5em; margin: 0 0 1.1em; }}
article.lf-body li {{ margin-bottom: 0.35em; }}
article.lf-body table {{ border-collapse: collapse; margin: 1.3em 0; display: block; overflow-x: auto; max-width: 100%; }}
article.lf-body th, article.lf-body td {{ border: 1px solid {sep}; padding: 7px 12px; text-align: left; font-size: 0.92em; }}
article.lf-body th {{ background: {rl}; font-weight: 600; }}
article.lf-body hr {{ border: none; border-top: 1px solid {sep}; margin: 2em 0; }}
::selection {{ background: {sel}; }}
.lf-rtl {{ direction: rtl; }}
"#,
            scheme = if self.dark { "dark" } else { "light" },
            sr = self.surface_reader.css(),
            rl = self.surface_list.css(),
            tp = self.text_primary.css(),
            ts = self.text_secondary.css(),
            ac = self.accent.css(),
            sep = self.separator.css(),
            sel = self.selection.css(),
            fs = font_size,
            lh = line_height,
            ch = measure_ch,
        )
    }
}
