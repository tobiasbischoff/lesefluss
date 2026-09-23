use reader::tokens::Tokens;

#[derive(Clone, Copy, Debug)]
pub struct ReaderStyleState {
    pub font_size: f64,
    pub measure_ch: u32,
    pub line_height: f64,
}

impl Default for ReaderStyleState {
    fn default() -> Self {
        Self { font_size: 18.0, measure_ch: 68, line_height: 1.6 }
    }
}

pub fn tokens_for(dark: bool) -> Tokens {
    if dark {
        Tokens::dark()
    } else {
        Tokens::light()
    }
}

pub fn gtk_css_for(tokens: &Tokens) -> String {
    tokens.gtk_css(tokens.dark)
}
