//! Shows a reply the way it was written: headings, lists, bold, italics, inline code, links and code blocks.
//! `parse` and `inline` are plain text functions (tested below); `render` builds the widgets.

use gtk4 as gtk;
use gtk::prelude::*;

#[derive(Debug, PartialEq)]
pub enum Block {
    Para(String),
    Heading(u8, String),
    Code { lang: String, text: String },
    /// `number` is Some for an ordered list.
    Item { depth: usize, number: Option<String>, text: String },
    Quote(String),
    Rule,
}

fn bullet_of(line: &str) -> Option<(usize, Option<String>, String)> {
    let indent = line.len() - line.trim_start().len();
    let t = line.trim_start();
    for p in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(p) {
            return Some((indent / 2, None, rest.to_string()));
        }
    }
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() && digits.len() < 4 {
        if let Some(rest) = t[digits.len()..].strip_prefix(". ") {
            return Some((indent / 2, Some(format!("{digits}.")), rest.to_string()));
        }
    }
    None
}

pub fn parse(src: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let mut code: Option<(String, Vec<String>)> = None;
    fn flush(para: &mut Vec<String>, out: &mut Vec<Block>) {
        if !para.is_empty() {
            out.push(Block::Para(para.join(" ")));
            para.clear();
        }
    }
    for line in src.lines() {
        if let Some((lang, lines)) = code.as_mut() {
            if line.trim_start().starts_with("```") {
                out.push(Block::Code { lang: lang.clone(), text: lines.join("\n") });
                code = None;
            } else {
                lines.push(line.to_string());
            }
            continue;
        }
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("```") {
            flush(&mut para, &mut out);
            code = Some((rest.trim().to_string(), Vec::new()));
        } else if t.is_empty() {
            flush(&mut para, &mut out);
        } else if t.len() >= 3 && t.chars().all(|c| c == '-' || c == '*' || c == '_') && !t.contains(' ') && t.chars().all(|c| c == t.chars().next().unwrap()) {
            flush(&mut para, &mut out);
            out.push(Block::Rule);
        } else if t.starts_with('#') && t.trim_start_matches('#').starts_with(' ') && t.len() - t.trim_start_matches('#').len() <= 6 {
            flush(&mut para, &mut out);
            let level = (t.len() - t.trim_start_matches('#').len()) as u8;
            out.push(Block::Heading(level, t.trim_start_matches('#').trim().to_string()));
        } else if let Some(rest) = t.strip_prefix("> ") {
            flush(&mut para, &mut out);
            out.push(Block::Quote(rest.to_string()));
        } else if let Some((depth, number, text)) = bullet_of(line) {
            flush(&mut para, &mut out);
            out.push(Block::Item { depth, number, text });
        } else if let (Some(Block::Item { text, .. }), true, true) = (out.last_mut(), para.is_empty(), line.starts_with(' ')) {
            // an indented line that carries on the item above
            text.push(' ');
            text.push_str(t);
        } else {
            para.push(t.to_string());
        }
    }
    if let Some((lang, lines)) = code {
        out.push(Block::Code { lang, text: lines.join("\n") }); // a block left open at the end
    }
    flush(&mut para, &mut out);
    out
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Inline markdown to Pango markup: `code`, **bold**, *italic*, [text](url).
pub fn inline(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let find = |from: usize, pat: &[char]| -> Option<usize> { (from..c.len().saturating_sub(pat.len() - 1)).find(|&j| c[j..j + pat.len()] == *pat) };
    while i < c.len() {
        match c[i] {
            '\\' if i + 1 < c.len() => {
                out.push_str(&esc(&c[i + 1].to_string()));
                i += 2;
            }
            '`' => match find(i + 1, &['`']) {
                Some(j) if j > i + 1 => {
                    let code: String = c[i + 1..j].iter().collect();
                    out.push_str(&format!("<span font_family=\"monospace\" background=\"#2B2B2B\">{}</span>", esc(&code)));
                    i = j + 1;
                }
                _ => {
                    out.push('`');
                    i += 1;
                }
            },
            '*' if i + 1 < c.len() && c[i + 1] == '*' => match find(i + 2, &['*', '*']) {
                Some(j) if j > i + 2 => {
                    let inner: String = c[i + 2..j].iter().collect();
                    out.push_str(&format!("<b>{}</b>", inline(&inner)));
                    i = j + 2;
                }
                _ => {
                    out.push_str("**");
                    i += 2;
                }
            },
            '*' if i + 1 < c.len() && !c[i + 1].is_whitespace() => match find(i + 1, &['*']) {
                Some(j) if j > i + 1 && !c[j - 1].is_whitespace() => {
                    let inner: String = c[i + 1..j].iter().collect();
                    out.push_str(&format!("<i>{}</i>", inline(&inner)));
                    i = j + 1;
                }
                _ => {
                    out.push('*');
                    i += 1;
                }
            },
            '[' => {
                let link = find(i + 1, &[']', '(']).and_then(|m| find(m + 2, &[')']).map(|e| (m, e)));
                match link {
                    Some((m, e)) => {
                        let text: String = c[i + 1..m].iter().collect();
                        let url: String = c[m + 2..e].iter().collect();
                        if url.starts_with("http://") || url.starts_with("https://") {
                            out.push_str(&format!("<a href=\"{}\">{}</a>", esc(&url).replace('"', "%22"), inline(&text)));
                        } else {
                            out.push_str(&inline(&text));
                        }
                        i = e + 1;
                    }
                    None => {
                        out.push('[');
                        i += 1;
                    }
                }
            }
            ch => {
                out.push_str(&esc(&ch.to_string()));
                i += 1;
            }
        }
    }
    out
}

fn text_label(markup: &str) -> gtk::Label {
    let l = gtk::Label::new(None);
    l.set_markup(markup);
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l.set_xalign(0.0);
    l.set_selectable(true);
    l.set_hexpand(true);
    l.add_css_class("msg-ai");
    l
}

fn code_block(lang: &str, text: &str) -> gtk::Box {
    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("code-block");
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    head.add_css_class("code-head");
    let name = gtk::Label::new(Some(if lang.is_empty() { "code" } else { lang }));
    name.add_css_class("code-lang");
    name.set_hexpand(true);
    name.set_xalign(0.0);
    head.append(&name);
    let copy = gtk::Button::from_icon_name("edit-copy-symbolic");
    copy.set_has_frame(false);
    copy.set_tooltip_text(Some("Copy the code"));
    copy.add_css_class("icon-btn");
    let code = text.to_string();
    copy.connect_clicked(move |b| b.clipboard().set_text(&code));
    head.append(&copy);
    frame.append(&head);

    let body = gtk::Label::new(Some(text));
    body.add_css_class("code-body");
    body.set_selectable(true);
    body.set_xalign(0.0);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&body));
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
    scroll.set_propagate_natural_height(true);
    frame.append(&scroll);
    frame
}

/// The widgets for one reply.
pub fn render(text: &str) -> gtk::Box {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 6);
    for block in parse(text) {
        match block {
            Block::Para(t) => col.append(&text_label(&inline(&t))),
            Block::Heading(level, t) => {
                let size = match level {
                    1 => "x-large",
                    2 => "large",
                    _ => "medium",
                };
                let l = text_label(&format!("<span size=\"{size}\" weight=\"bold\">{}</span>", inline(&t)));
                l.set_margin_top(6);
                col.append(&l);
            }
            Block::Code { lang, text } => col.append(&code_block(&lang, &text)),
            Block::Item { depth, number, text } => {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                row.set_margin_start(6 + depth as i32 * 18);
                let mark = gtk::Label::new(Some(number.as_deref().unwrap_or("•")));
                mark.set_valign(gtk::Align::Start);
                mark.add_css_class("md-mark");
                row.append(&mark);
                let t = text_label(&inline(&text));
                t.add_css_class("md-item");
                row.append(&t);
                col.append(&row);
            }
            Block::Quote(t) => {
                let l = text_label(&inline(&t));
                l.add_css_class("md-quote");
                col.append(&l);
            }
            Block::Rule => col.append(&gtk::Separator::new(gtk::Orientation::Horizontal)),
        }
    }
    col
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_are_found() {
        let src = "# Title\n\nSome **bold** text\nsecond line.\n\n- one\n- two\n  more of two\n1. first\n\n```rust\nfn main() {}\n```\n> quoted\n---\n";
        let b = parse(src);
        assert_eq!(b[0], Block::Heading(1, "Title".into()));
        assert_eq!(b[1], Block::Para("Some **bold** text second line.".into()));
        assert_eq!(b[2], Block::Item { depth: 0, number: None, text: "one".into() });
        assert_eq!(b[3], Block::Item { depth: 0, number: None, text: "two more of two".into() });
        assert_eq!(b[4], Block::Item { depth: 0, number: Some("1.".into()), text: "first".into() });
        assert_eq!(b[5], Block::Code { lang: "rust".into(), text: "fn main() {}".into() });
        assert_eq!(b[6], Block::Quote("quoted".into()));
        assert_eq!(b[7], Block::Rule);
    }

    #[test]
    fn an_unfinished_code_block_is_still_shown() {
        assert_eq!(parse("```\nls -la"), vec![Block::Code { lang: "".into(), text: "ls -la".into() }]);
    }

    #[test]
    fn inline_styles_become_markup_and_text_is_escaped() {
        assert_eq!(inline("a **b** and *c*"), "a <b>b</b> and <i>c</i>");
        assert!(inline("run `ls <dir>`").contains("ls &lt;dir&gt;</span>"));
        assert_eq!(inline("1 < 2 & 3"), "1 &lt; 2 &amp; 3");
        assert_eq!(inline("[docs](https://example.com/a?b=1&c=2)"), "<a href=\"https://example.com/a?b=1&amp;c=2\">docs</a>");
        assert_eq!(inline("[x](javascript:evil)"), "x", "only web links stay links");
    }

    #[test]
    fn stray_markers_are_left_alone() {
        assert_eq!(inline("2 * 3 = 6"), "2 * 3 = 6");
        assert_eq!(inline("a ** b"), "a ** b");
        assert_eq!(inline("tick ` alone"), "tick ` alone");
        assert_eq!(inline("\\*not italic\\*"), "*not italic*");
    }
}
