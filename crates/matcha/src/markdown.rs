//! Just enough Markdown for `drift`: inline code spans, links, and heading
//! anchors, outside fenced code blocks and HTML comments.

use std::collections::BTreeSet;

/// An inline code span and the prose around it on its line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// 1-based.
    pub line: usize,
    pub code: String,
    /// The line's text before and after the span.
    pub before: String,
    pub after: String,
}

/// A link target, as written: `../a.md#b`, `#b`, `https://...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub line: usize,
    pub target: String,
}

#[derive(Debug, Default)]
pub struct Doc {
    pub spans: Vec<Span>,
    pub links: Vec<Link>,
    /// GitHub-style anchors for every heading, plus explicit `id`/`name` anchors.
    pub anchors: BTreeSet<String>,
    /// The doc asked for only its links to be checked.
    pub links_only: bool,
}

/// Marks the next non-blank line as not to be checked.
const SKIP: &str = "<!-- matcha: skip -->";
/// Marks a doc whose code spans aren't claims about this repo (a proposal, a
/// write-up of another repo). Its links are still checked.
const LINKS_ONLY: &str = "<!-- matcha: links-only -->";

pub fn parse(text: &str) -> Doc {
    let mut doc = Doc::default();
    let mut fence: Option<(char, usize)> = None;
    let mut in_comment = false;
    let mut skip_next = false;
    let mut slugs = Slugger::default();

    for (i, raw) in text.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = raw.trim_start();

        if let Some((ch, len)) = fence {
            let run = trimmed.chars().take_while(|&c| c == ch).count();
            if run >= len && trimmed[run * ch.len_utf8()..].trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if let Some(open) = fence_open(trimmed) {
            fence = Some(open);
            continue;
        }

        collect_html_anchors(raw, &mut doc.anchors);
        if raw.contains(LINKS_ONLY) {
            doc.links_only = true;
        }

        // Drop HTML comments, which may span lines.
        let mut line = String::new();
        let mut rest = raw;
        loop {
            if in_comment {
                match rest.find("-->") {
                    Some(end) => {
                        rest = &rest[end + 3..];
                        in_comment = false;
                    }
                    None => break,
                }
            } else {
                match rest.find("<!--") {
                    Some(start) => {
                        line.push_str(&rest[..start]);
                        rest = &rest[start + 4..];
                        in_comment = true;
                    }
                    None => {
                        line.push_str(rest);
                        break;
                    }
                }
            }
        }

        if raw.contains(SKIP) {
            skip_next = true;
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }

        if let Some(heading) = atx_heading(&line) {
            doc.anchors.insert(slugs.slug(heading));
        }
        if std::mem::take(&mut skip_next) {
            continue;
        }

        let (spans, prose) = code_spans(&line);
        doc.spans
            .extend(spans.into_iter().map(|(start, end, code)| Span {
                line: line_no,
                code,
                before: line[..start].to_owned(),
                after: line[end..].to_owned(),
            }));
        doc.links
            .extend(links(&prose).into_iter().map(|target| Link {
                line: line_no,
                target,
            }));
    }
    doc
}

/// The fence character and length if `line` opens a fenced code block.
fn fence_open(line: &str) -> Option<(char, usize)> {
    let ch = line.chars().next().filter(|&c| c == '`' || c == '~')?;
    let len = line.chars().take_while(|&c| c == ch).count();
    (len >= 3).then_some((ch, len))
}

fn atx_heading(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if line.len() - t.len() > 3 {
        return None;
    }
    let hashes = t.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &t[hashes..];
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    Some(rest.trim().trim_end_matches('#').trim_end())
}

/// Code spans as (start, end, content) byte offsets into `line`, and the line
/// with each span blanked out so links aren't found inside code.
fn code_spans(line: &str) -> (Vec<(usize, usize, String)>, String) {
    let bytes = line.as_bytes();
    let mut spans = Vec::new();
    let mut prose = String::with_capacity(line.len());
    let mut i = 0;
    let mut copied = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let open = run_len(bytes, i);
        let content_start = i + open;
        // Find a closing run of exactly the same length.
        let mut j = content_start;
        let mut close = None;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let n = run_len(bytes, j);
                if n == open {
                    close = Some(j);
                    break;
                }
                j += n;
            } else {
                j += 1;
            }
        }
        let Some(close) = close else {
            // No closer: the backticks are literal.
            i = content_start;
            continue;
        };
        let end = close + open;
        let content = &line[content_start..close];
        let content = match content.strip_prefix(' ').and_then(|c| c.strip_suffix(' ')) {
            Some(inner) if !inner.trim().is_empty() => inner,
            _ => content,
        };
        spans.push((i, end, content.to_owned()));
        prose.push_str(&line[copied..i]);
        prose.extend(std::iter::repeat_n(' ', end - i));
        copied = end;
        i = end;
    }
    prose.push_str(&line[copied.min(line.len())..]);
    (spans, prose)
}

fn run_len(bytes: &[u8], start: usize) -> usize {
    bytes[start..].iter().take_while(|&&b| b == b'`').count()
}

/// Inline link and image targets, and reference definitions.
fn links(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let t = line.trim_start();
    // `[label]: target`
    // `[^1]: text` is a footnote, not a link.
    if t.starts_with('[')
        && !t.starts_with("[^")
        && let Some(close) = t.find("]:")
        && !t[1..close].contains(']')
    {
        if let Some(target) = t[close + 2..].split_whitespace().next() {
            out.push(unangle(target));
        }
        return out;
    }
    // `[text](target "title")`
    let mut rest = line;
    while let Some(pos) = rest.find("](") {
        let after = &rest[pos + 2..];
        let Some(end) = matching_paren(after) else {
            break;
        };
        let inside = after[..end].trim();
        let target = if let Some(stripped) = inside.strip_prefix('<') {
            stripped.split('>').next().unwrap_or("").to_owned()
        } else {
            inside.split_whitespace().next().unwrap_or("").to_owned()
        };
        if !target.is_empty() {
            out.push(target);
        }
        rest = &after[end..];
    }
    out
}

fn unangle(s: &str) -> String {
    s.trim_start_matches('<').trim_end_matches('>').to_owned()
}

/// The index of the `)` that closes a link target, allowing balanced parens.
fn matching_paren(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(i),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// `id="x"` and `name="x"` attributes, which make anchors too.
fn collect_html_anchors(line: &str, anchors: &mut BTreeSet<String>) {
    for attr in ["id=\"", "name=\""] {
        let mut rest = line;
        while let Some(pos) = rest.find(attr) {
            let value = &rest[pos + attr.len()..];
            if let Some(end) = value.find('"') {
                anchors.insert(value[..end].to_owned());
            }
            rest = value;
        }
    }
}

/// GitHub's heading anchors: lowercase, punctuation dropped, spaces to
/// hyphens, and `-1`, `-2` on repeats.
#[derive(Debug, Default)]
struct Slugger {
    seen: std::collections::HashMap<String, usize>,
}

impl Slugger {
    fn slug(&mut self, heading: &str) -> String {
        let base = slug(heading);
        let n = self.seen.entry(base.clone()).or_insert(0);
        let out = if *n == 0 { base } else { format!("{base}-{n}") };
        *n += 1;
        out
    }
}

pub fn slug(heading: &str) -> String {
    let text = strip_link_targets(heading);
    text.trim()
        .chars()
        .flat_map(char::to_lowercase)
        .filter_map(|c| match c {
            ' ' => Some('-'),
            '-' | '_' => Some(c),
            c if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .collect()
}

/// `[text](url)` to `text`, so a heading's anchor comes from what it shows.
fn strip_link_targets(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find("](") {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];
        match matching_paren(after) {
            Some(end) => rest = &after[end + 1..],
            None => {
                rest = after;
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(doc: &Doc) -> Vec<&str> {
        doc.spans.iter().map(|s| s.code.as_str()).collect()
    }

    #[test]
    fn finds_code_spans_with_their_context() {
        let doc = parse("Use `ClientId(Uuid)`, not `Uuid`.\n");
        assert_eq!(codes(&doc), ["ClientId(Uuid)", "Uuid"]);
        assert_eq!(doc.spans[1].before, "Use `ClientId(Uuid)`, not ");
        assert_eq!(doc.spans[1].after, ".");
    }

    #[test]
    fn double_backticks_hold_single_ones() {
        let doc = parse("Run `` a`b `` and `c`.\n");
        assert_eq!(codes(&doc), ["a`b", "c"]);
    }

    #[test]
    fn skips_fenced_blocks_and_comments() {
        let doc =
            parse("`a`\n```rust\n`b`\n```\n<!-- `c`\n`d` -->`e`\n~~~~\n`f`\n~~~\n`g`\n~~~~\n");
        assert_eq!(codes(&doc), ["a", "e"]);
    }

    #[test]
    fn a_skip_comment_skips_the_next_line_only() {
        let doc = parse("<!-- matcha: skip -->\n\n`a` [x](y.md)\n`b`\n");
        assert_eq!(codes(&doc), ["b"]);
        assert!(doc.links.is_empty());
    }

    #[test]
    fn notes_links_only_docs() {
        assert!(
            parse(
                "# Plan
<!-- matcha: links-only -->
"
            )
            .links_only
        );
        assert!(
            !parse(
                "# Plan
"
            )
            .links_only
        );
    }

    #[test]
    fn finds_links_outside_code() {
        let doc = parse(
            "See [a](docs/a.md#top \"title\"), ![img](<b c.png>), `[no](x.md)`, [f(x)](f_(x).md).\n[ref]: ../r.md\n[^1]: To be sure.\n",
        );
        let targets: Vec<&str> = doc.links.iter().map(|l| l.target.as_str()).collect();
        assert_eq!(targets, ["docs/a.md#top", "b c.png", "f_(x).md", "../r.md"]);
        assert_eq!(doc.links[3].line, 2);
    }

    #[test]
    fn makes_github_anchors() {
        let doc = parse(
            "# Design: mochi-donut\n## `census`: how is this done here?\n## Notes\n## Notes\n### [Link](x.md) text\n<a id=\"custom\"></a>\n",
        );
        let anchors: Vec<&str> = doc.anchors.iter().map(String::as_str).collect();
        assert_eq!(
            anchors,
            [
                "census-how-is-this-done-here",
                "custom",
                "design-mochi-donut",
                "link-text",
                "notes",
                "notes-1"
            ]
        );
    }

    #[test]
    fn headings_need_a_space() {
        assert_eq!(atx_heading("#hashtag"), None);
        assert_eq!(atx_heading("## Two ##"), Some("Two"));
        assert_eq!(atx_heading("    # code"), None);
    }
}
