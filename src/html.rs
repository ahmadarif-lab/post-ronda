//! A small HTML reader for the response preview: it keeps the structure that matters when
//! reading a page (headings, paragraphs, lists, links, code, simple tables) and drops what
//! needs a browser engine (CSS, scripts, images, layout).

/// A run of text with the inline styling it had in the page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading(u8, Vec<Span>),
    Paragraph(Vec<Span>),
    Quote(Vec<Span>),
    Item {
        depth: usize,
        marker: String,
        spans: Vec<Span>,
    },
    Pre(String),
    Rule,
}

/// Most blocks kept for one page, so a huge document cannot stall the viewer.
const MAX_BLOCKS: usize = 4000;

/// Whether a response should be offered as HTML: by its content type, or by what the body starts with.
pub fn looks_like_html(content_type: &str, body: &str) -> bool {
    if content_type.to_ascii_lowercase().contains("html") {
        return true;
    }
    let head: String = body
        .trim_start()
        .chars()
        .take(15)
        .collect::<String>()
        .to_ascii_lowercase();
    head.starts_with("<!doctype html") || head.starts_with("<html")
}

struct Reader {
    blocks: Vec<Block>,
    spans: Vec<Span>,
    bold: usize,
    italic: usize,
    code: usize,
    quote: usize,
    pre: usize,
    pre_text: String,
    links: Vec<Option<String>>,
    lists: Vec<(bool, usize)>,
    heading: Option<u8>,
    item: Option<(usize, String)>,
}

impl Reader {
    fn push_text(&mut self, text: &str) {
        if self.pre > 0 {
            self.pre_text.push_str(text);
            return;
        }
        for piece in text.split_inclusive(char::is_whitespace) {
            let word = piece.trim_end_matches(char::is_whitespace);
            let spaced = word.len() != piece.len();
            if !word.is_empty() {
                self.add(word);
            }
            if spaced {
                self.add(" ");
            }
        }
    }
    /// Appends to the open span unless the inline style changed; runs of spaces collapse.
    fn add(&mut self, text: &str) {
        let link = self.links.iter().rev().find_map(|l| l.clone());
        let (bold, italic, code) = (
            self.bold > 0 || self.heading.is_some(),
            self.italic > 0,
            self.code > 0,
        );
        if text == " "
            && self
                .spans
                .last()
                .is_none_or(|s| s.text.is_empty() || s.text.ends_with([' ', '\n']))
        {
            return;
        }
        match self.spans.last_mut() {
            Some(s) if s.bold == bold && s.italic == italic && s.code == code && s.link == link => {
                s.text.push_str(text)
            }
            _ => self.spans.push(Span {
                text: text.to_owned(),
                bold,
                italic,
                code,
                link,
            }),
        }
    }
    fn flush(&mut self) {
        if self.pre > 0 {
            return;
        }
        let mut spans = std::mem::take(&mut self.spans);
        if let Some(first) = spans.first_mut() {
            first.text = first.text.trim_start().to_owned();
        }
        if let Some(last) = spans.last_mut() {
            last.text = last.text.trim_end().to_owned();
        }
        spans.retain(|s| !s.text.is_empty());
        let heading = self.heading;
        let item = self.item.take();
        if spans.is_empty() || self.blocks.len() >= MAX_BLOCKS {
            return;
        }
        self.blocks.push(if let Some(level) = heading {
            Block::Heading(level, spans)
        } else if let Some((depth, marker)) = item {
            Block::Item {
                depth,
                marker,
                spans,
            }
        } else if self.quote > 0 {
            Block::Quote(spans)
        } else {
            Block::Paragraph(spans)
        });
    }
    fn end_pre(&mut self) {
        let text = std::mem::take(&mut self.pre_text);
        let text = text.trim_matches('\n').trim_end().to_owned();
        if !text.is_empty() && self.blocks.len() < MAX_BLOCKS {
            self.blocks.push(Block::Pre(text));
        }
    }
    fn open(&mut self, name: &str, attrs: &str) {
        match name {
            "b" | "strong" => self.bold += 1,
            "i" | "em" | "cite" | "dfn" => self.italic += 1,
            "code" | "kbd" | "samp" | "tt" => self.code += 1,
            "a" => self.links.push(attribute(attrs, "href")),
            "br" => {
                if self.pre > 0 {
                    self.pre_text.push('\n');
                } else {
                    self.add("\n");
                }
            }
            "hr" => {
                self.flush();
                self.blocks.push(Block::Rule);
            }
            "img" => {
                let alt = attribute(attrs, "alt").unwrap_or_default();
                self.italic += 1;
                self.push_text(&if alt.is_empty() {
                    "[image]".to_owned()
                } else {
                    format!("[image: {alt}]")
                });
                self.italic -= 1;
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush();
                self.heading = Some(name.as_bytes()[1] - b'0');
            }
            "pre" => {
                self.flush();
                self.pre += 1;
            }
            "blockquote" => {
                self.flush();
                self.quote += 1;
            }
            "ul" | "ol" => {
                self.flush();
                self.lists.push((name == "ol", 0));
            }
            "li" | "dd" | "dt" => {
                self.flush();
                self.item = Some(match self.lists.last_mut() {
                    Some((ordered, count)) => {
                        *count += 1;
                        let marker = if *ordered {
                            format!("{count}.")
                        } else {
                            "•".to_owned()
                        };
                        (self.lists.len() - 1, marker)
                    }
                    None => (0, "•".to_owned()),
                });
            }
            "td" | "th" => {
                if !self.spans.is_empty() {
                    self.add(" │ ");
                }
            }
            "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "nav"
            | "aside" | "form" | "table" | "tr" | "dl" | "figure" | "figcaption" | "details"
            | "summary" | "fieldset" | "body" | "html" | "address" | "center" | "caption"
            | "thead" | "tbody" | "tfoot" => self.flush(),
            _ => {}
        }
    }
    fn close(&mut self, name: &str) {
        match name {
            "b" | "strong" => self.bold = self.bold.saturating_sub(1),
            "i" | "em" | "cite" | "dfn" => self.italic = self.italic.saturating_sub(1),
            "code" | "kbd" | "samp" | "tt" => self.code = self.code.saturating_sub(1),
            "a" => {
                self.links.pop();
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush();
                self.heading = None;
            }
            "pre" => {
                if self.pre > 0 {
                    self.pre -= 1;
                    if self.pre == 0 {
                        self.end_pre();
                    }
                }
            }
            "blockquote" => {
                self.flush();
                self.quote = self.quote.saturating_sub(1);
            }
            "ul" | "ol" => {
                self.flush();
                self.lists.pop();
            }
            "li" | "dd" | "dt" | "p" | "div" | "section" | "article" | "header" | "footer"
            | "main" | "nav" | "aside" | "form" | "table" | "tr" | "dl" | "figure"
            | "figcaption" | "details" | "summary" | "fieldset" | "body" | "html" | "address"
            | "center" | "caption" | "thead" | "tbody" | "tfoot" => self.flush(),
            _ => {}
        }
    }
}

/// The value of attribute `name` in the text of a start tag.
fn attribute(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let start = from + found;
        from = start + name.len();
        let before_ok = start == 0 || lower.as_bytes()[start - 1].is_ascii_whitespace();
        let rest = attrs[from..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let value = rest[1..].trim_start();
        let value = match value.chars().next() {
            Some(q @ ('"' | '\'')) => value[1..].split(q).next().unwrap_or(""),
            _ => value
                .split(|c: char| c.is_whitespace() || c == '>')
                .next()
                .unwrap_or(""),
        };
        return Some(decode(value));
    }
    None
}

/// Replaces the character references a page commonly uses.
pub fn decode(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let replaced = rest[1..]
            .find(';')
            .filter(|&end| end <= 10)
            .and_then(|end| entity(&rest[1..1 + end]).map(|c| (c, end + 2)));
        match replaced {
            Some((c, used)) => {
                out.push(c);
                rest = &rest[used..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "bull" | "middot" => '•',
        "laquo" => '«',
        "raquo" => '»',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "rarr" => '→',
        "larr" => '←',
        "times" => '×',
        _ => return None,
    })
}

/// Reads `html` into blocks to show in order.
pub fn parse(html: &str) -> Vec<Block> {
    let mut reader = Reader {
        blocks: vec![],
        spans: vec![],
        bold: 0,
        italic: 0,
        code: 0,
        quote: 0,
        pre: 0,
        pre_text: String::new(),
        links: vec![],
        lists: vec![],
        heading: None,
        item: None,
    };
    let lower = html.to_ascii_lowercase();
    let mut at = 0;
    while at < html.len() && reader.blocks.len() < MAX_BLOCKS {
        let Some(open) = html[at..].find('<').map(|i| at + i) else {
            reader.push_text(&decode(&html[at..]));
            break;
        };
        if open > at {
            reader.push_text(&decode(&html[at..open]));
        }
        let tail = &html[open..];
        if tail.starts_with("<!--") {
            at = tail.find("-->").map_or(html.len(), |i| open + i + 3);
            continue;
        }
        // The end of the tag: the first `>` outside quotes.
        let mut quote = None;
        let mut end = None;
        for (i, c) in tail.char_indices().skip(1) {
            match (quote, c) {
                (None, '"' | '\'') => quote = Some(c),
                (Some(q), c) if c == q => quote = None,
                (None, '>') => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            reader.push_text(&decode(tail));
            break;
        };
        let inner = &tail[1..end];
        at = open + end + 1;
        if inner.starts_with('!') || inner.starts_with('?') {
            continue;
        }
        let closing = inner.starts_with('/');
        let inner = inner.trim_start_matches('/').trim_end_matches('/');
        let (name, attrs) = inner
            .split_once(|c: char| c.is_whitespace())
            .unwrap_or((inner, ""));
        let name = name.to_ascii_lowercase();
        if closing {
            reader.close(&name);
            continue;
        }
        // Content that is never shown: skip to the matching end tag.
        if matches!(
            name.as_str(),
            "script" | "style" | "head" | "noscript" | "template" | "svg" | "iframe" | "select"
        ) {
            let needle = format!("</{name}");
            at = lower[at..]
                .find(&needle)
                .and_then(|i| lower[at + i..].find('>').map(|j| at + i + j + 1))
                .unwrap_or(html.len());
            continue;
        }
        reader.open(&name, attrs);
    }
    reader.flush();
    if reader.pre > 0 {
        reader.end_pre();
    }
    reader.blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.text.as_str()).collect()
    }
    #[test]
    fn detects_html_by_type_or_start() {
        assert!(looks_like_html("text/html; charset=utf-8", "x"));
        assert!(looks_like_html("", "\n  <!DOCTYPE html><html>"));
        assert!(looks_like_html("", "<HTML lang=en>"));
        assert!(!looks_like_html("application/json", "{\"a\":1}"));
        assert!(!looks_like_html("", "<xml/>"));
    }
    #[test]
    fn reads_structure_and_skips_what_is_not_shown() {
        let blocks = parse(
            r#"<!DOCTYPE html><html><head><title>T</title><style>p{}</style></head>
            <body><!-- note --><script>if (a < b) {}</script>
            <h1>Hello &amp; welcome</h1>
            <p>Some <b>bold</b> and <a href="https://x.test/a?b=1&amp;c=2" class='k'>a link</a>.</p>
            <ul><li>one</li><li>two<ul><li>nested</li></ul></li></ul>
            <ol><li>first</li></ol>
            <pre>  a
  b</pre><hr></body></html>"#,
        );
        assert_eq!(blocks.len(), 8, "{blocks:#?}");
        let Block::Heading(1, spans) = &blocks[0] else {
            panic!("{blocks:#?}")
        };
        assert_eq!(text(spans), "Hello & welcome");
        let Block::Paragraph(spans) = &blocks[1] else {
            panic!()
        };
        assert_eq!(text(spans), "Some bold and a link.");
        assert!(spans[1].bold);
        assert_eq!(spans[3].link.as_deref(), Some("https://x.test/a?b=1&c=2"));
        let Block::Item { depth, marker, .. } = &blocks[3] else {
            panic!()
        };
        assert_eq!((*depth, marker.as_str()), (0, "•"));
        let Block::Item { depth, spans, .. } = &blocks[4] else {
            panic!()
        };
        assert_eq!((*depth, text(spans).as_str()), (1, "nested"));
        let Block::Item { marker, .. } = &blocks[5] else {
            panic!()
        };
        assert_eq!(marker, "1.");
        assert_eq!(blocks[6], Block::Pre("  a\n  b".into()));
        assert_eq!(blocks[7], Block::Rule);
    }
    #[test]
    fn broken_markup_never_panics() {
        for html in [
            "<",
            "<a",
            "<a href=\"x",
            "</",
            "<p><b>x",
            "&#xZZ; &#99999999; &",
            "<!--",
        ] {
            let _ = parse(html);
        }
        assert_eq!(
            decode("&lt;b&gt; &#65;&#x42; &nbsp;&bogus;"),
            "<b> AB  &bogus;"
        );
    }
}
