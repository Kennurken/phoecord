//! The terms screen: the text of `TERMS.md` (or `TERMS.ru.md`), drawn from its Markdown, with
//! "I accept" and "Quit". Accepting stores the version in the data folder (1.4.7); a newer version
//! of the terms is shown again on the next start.

use crate::theme::{self, color};
use eframe::egui::{self, Color32, FontId, text::LayoutJob};

/// One block of the terms text.
#[derive(Debug, PartialEq, Eq)]
pub enum Block {
    Title(String),
    Heading(String),
    Para(String),
    Bullet(String),
}

/// Split the Markdown of the terms into blocks. Only what the terms use: `#`, `##`, `- ` items,
/// paragraphs (lines joined), `**bold**`, `` `code` `` and `[text](link)`.
pub fn blocks(md: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut para = String::new();
    let flush = |para: &mut String, out: &mut Vec<Block>| {
        if !para.trim().is_empty() {
            out.push(Block::Para(para.trim().to_string()));
        }
        para.clear();
    };
    for line in md.lines() {
        let l = line.trim_end();
        // The window shows the version itself.
        if l.starts_with("Version:") || l.starts_with("Версия:") {
            continue;
        }
        if let Some(h) = l.strip_prefix("# ") {
            flush(&mut para, &mut out);
            out.push(Block::Title(h.trim().into()));
        } else if let Some(h) = l.strip_prefix("## ") {
            flush(&mut para, &mut out);
            out.push(Block::Heading(h.trim().into()));
        } else if let Some(b) = l.strip_prefix("- ") {
            flush(&mut para, &mut out);
            out.push(Block::Bullet(b.trim().into()));
        } else if l.trim().is_empty() {
            flush(&mut para, &mut out);
        } else if let Some(Block::Bullet(b)) = out
            .last_mut()
            .filter(|_| para.is_empty() && line.starts_with("  "))
        {
            b.push(' ');
            b.push_str(l.trim());
        } else {
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(l.trim());
        }
    }
    flush(&mut para, &mut out);
    out
}

/// Inline Markdown as styled runs: `(text, bold, code)`.
pub fn inline(s: &str) -> Vec<(String, bool, bool)> {
    let mut out: Vec<(String, bool, bool)> = Vec::new();
    let mut cur = String::new();
    let (mut bold, mut code) = (false, false);
    let mut chars = s.chars().peekable();
    let push = |cur: &mut String, out: &mut Vec<(String, bool, bool)>, b, c| {
        if !cur.is_empty() {
            out.push((std::mem::take(cur), b, c));
        }
    };
    while let Some(ch) = chars.next() {
        match ch {
            '*' if !code && chars.peek() == Some(&'*') => {
                chars.next();
                push(&mut cur, &mut out, bold, code);
                bold = !bold;
            }
            '`' => {
                push(&mut cur, &mut out, bold, code);
                code = !code;
            }
            '[' if !code => {
                // [text](link) -> text
                let rest: String = chars.clone().collect();
                if let Some(close) = rest.find("](")
                    && let Some(end) = rest[close..].find(')')
                {
                    let label = &rest[..close];
                    for (t, b, c) in inline(label) {
                        push(&mut cur, &mut out, bold, code);
                        out.push((t, b || bold, c));
                    }
                    for _ in 0..rest[..close + end + 1].chars().count() {
                        chars.next();
                    }
                } else {
                    cur.push(ch);
                }
            }
            _ => cur.push(ch),
        }
    }
    push(&mut cur, &mut out, bold, code);
    out
}

fn job(s: &str, size: f32, base: Color32, width: f32, strong: bool) -> LayoutJob {
    let mut j = LayoutJob::default();
    j.wrap.max_width = width;
    for (t, b, c) in inline(s) {
        let font: FontId = if b || strong {
            theme::semibold(size)
        } else if c {
            theme::medium(size)
        } else {
            theme::regular(size)
        };
        let color = if b || strong { color::TEXT } else { base };
        let mut fmt = egui::TextFormat::simple(font, color);
        fmt.line_height = Some(size * 1.55);
        if c {
            fmt.background = color::RAISED;
        }
        j.append(&t, 0.0, fmt);
    }
    j
}

/// Draw the terms in `ui` (inside a scroll area).
pub fn show(ui: &mut egui::Ui, md: &str, w: f32) {
    for b in blocks(md) {
        match b {
            // The window draws its own title above the text.
            Block::Title(_) => {}
            Block::Heading(h) => {
                ui.add_space(theme::S4);
                ui.add(egui::Label::new(job(&h, theme::T_TITLE, color::TEXT, w, true)).wrap());
                ui.add_space(theme::S1);
            }
            Block::Para(p) => {
                ui.add(egui::Label::new(job(&p, theme::T_BODY, color::MUTED, w, false)).wrap());
                ui.add_space(theme::S2);
            }
            Block::Bullet(p) => {
                ui.horizontal_top(|ui| {
                    ui.add_space(theme::S1);
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 21.0), egui::Sense::hover());
                    ui.painter().circle_filled(
                        egui::pos2(r.center().x, r.top() + 11.0),
                        2.5,
                        color::ROSE,
                    );
                    ui.add_space(theme::S1);
                    ui.add(
                        egui::Label::new(job(&p, theme::T_BODY, color::MUTED, w - 30.0, false))
                            .wrap(),
                    );
                });
                ui.add_space(theme::S1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_terms_parse_into_headings_paragraphs_and_bullets() {
        for lang in ["en", "ru"] {
            let b = blocks(phoecord_installer::terms::text(lang));
            assert!(matches!(b[0], Block::Title(_)));
            assert!(b.iter().filter(|x| matches!(x, Block::Heading(_))).count() >= 5);
            assert!(b.iter().any(|x| matches!(x, Block::Bullet(_))));
            // No raw Markdown markers survive inline parsing.
            for x in &b {
                if let Block::Para(p) | Block::Bullet(p) = x {
                    let plain: String = inline(p).into_iter().map(|r| r.0).collect();
                    assert!(!plain.contains("**") && !plain.contains("]("), "{plain}");
                }
            }
        }
    }

    #[test]
    fn inline_styles() {
        assert_eq!(
            inline("a **b** `c` [d](x) e"),
            vec![
                ("a ".into(), false, false),
                ("b".into(), true, false),
                (" ".into(), false, false),
                ("c".into(), false, true),
                (" ".into(), false, false),
                ("d".into(), false, false),
                (" e".into(), false, false),
            ]
        );
    }
}
