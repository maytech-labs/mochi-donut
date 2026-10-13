//! Splitting a shell command line into the simple commands it would run.
//!
//! This is not a shell. It finds every program a line would start, with its
//! words after quote removal, so `check` can look at each one: commands joined
//! by `;`, `&&`, `||`, `|`, or newlines, in subshells and `{ }` groups, and
//! inside `$( )` and backticks, quoted or not. Variables aren't expanded,
//! redirections are dropped, and heredoc bodies are skipped as data.

/// The simple commands in `src`, each as its words, in the order they appear.
/// Command substitutions come before the command that uses them.
pub fn commands(src: &str) -> Vec<Vec<String>> {
    let mut p = Parser {
        chars: src.chars().collect(),
        i: 0,
        out: Vec::new(),
        heredocs: Vec::new(),
    };
    p.list(None);
    p.out
}

struct Parser {
    chars: Vec<char>,
    i: usize,
    out: Vec<Vec<String>>,
    /// Heredoc delimiters waiting for the end of the current line.
    heredocs: Vec<Heredoc>,
}

struct Heredoc {
    delimiter: String,
    /// `<<-` strips leading tabs from the body and the delimiter line.
    strip_tabs: bool,
}

/// The words of the command being read.
#[derive(Default)]
struct Words {
    done: Vec<String>,
    word: String,
    /// Whether a word has started, so `""` counts as an empty word.
    started: bool,
    /// The next word is a redirection target, not an argument.
    skip_next: bool,
}

impl Words {
    fn push_char(&mut self, c: char) {
        self.word.push(c);
        self.started = true;
    }

    fn end_word(&mut self) {
        if self.started {
            let word = std::mem::take(&mut self.word);
            if self.skip_next {
                self.skip_next = false;
            } else {
                self.done.push(word);
            }
        }
        self.started = false;
    }

    fn end_command(&mut self, out: &mut Vec<Vec<String>>) {
        self.end_word();
        self.skip_next = false;
        if !self.done.is_empty() {
            out.push(std::mem::take(&mut self.done));
        }
    }
}

impl Parser {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.i + ahead).copied()
    }

    /// Reads commands until `end` (`)` or a backtick) or the end of input.
    fn list(&mut self, end: Option<char>) {
        let mut w = Words::default();
        while let Some(c) = self.peek(0) {
            self.i += 1;
            match c {
                _ if Some(c) == end => break,
                '\n' => {
                    w.end_command(&mut self.out);
                    self.skip_heredoc_bodies();
                }
                ';' | '&' | '|' | '(' | ')' => w.end_command(&mut self.out),
                ' ' | '\t' | '\r' => w.end_word(),
                '#' if !w.started => {
                    while self.peek(0).is_some_and(|c| c != '\n') {
                        self.i += 1;
                    }
                }
                '\\' => {
                    if let Some(next) = self.peek(0) {
                        self.i += 1;
                        if next != '\n' {
                            w.push_char(next);
                        }
                    }
                }
                '\'' => {
                    w.started = true;
                    while let Some(q) = self.peek(0) {
                        self.i += 1;
                        if q == '\'' {
                            break;
                        }
                        w.word.push(q);
                    }
                }
                '"' => self.double_quoted(&mut w),
                '`' => {
                    self.list(Some('`'));
                    w.push_char('…');
                }
                '$' if self.peek(0) == Some('(') => {
                    self.i += 1;
                    self.list(Some(')'));
                    w.push_char('…');
                }
                '<' | '>' => self.redirection(c, &mut w),
                _ => w.push_char(c),
            }
        }
        w.end_command(&mut self.out);
    }

    fn double_quoted(&mut self, w: &mut Words) {
        w.started = true;
        while let Some(c) = self.peek(0) {
            self.i += 1;
            match c {
                '"' => return,
                '\\' => {
                    if let Some(next) = self.peek(0) {
                        self.i += 1;
                        if !matches!(next, '"' | '\\' | '$' | '`') {
                            w.word.push('\\');
                        }
                        if next != '\n' {
                            w.word.push(next);
                        }
                    }
                }
                '`' => {
                    self.list(Some('`'));
                    w.word.push('…');
                }
                '$' if self.peek(0) == Some('(') => {
                    self.i += 1;
                    self.list(Some(')'));
                    w.word.push('…');
                }
                _ => w.word.push(c),
            }
        }
    }

    /// Drops a redirection (`>`, `2>&1`, `<<EOF`) and the word it names.
    fn redirection(&mut self, first: char, w: &mut Words) {
        // A file descriptor number right before the operator belongs to it.
        if w.started && w.word.chars().all(|c| c.is_ascii_digit()) {
            w.word.clear();
            w.started = false;
        }
        w.end_word();
        if first == '<' && self.peek(0) == Some('<') && self.peek(1) != Some('<') {
            self.i += 1;
            self.heredoc_start();
            return;
        }
        let mut op = String::from(first);
        while let Some(c) = self.peek(0).filter(|c| matches!(c, '<' | '>' | '&' | '|')) {
            op.push(c);
            self.i += 1;
        }
        // `>&2` and `2>&1` name a descriptor, not a file.
        if op.ends_with('&') && self.peek(0).is_some_and(|c| c.is_ascii_digit() || c == '-') {
            while self.peek(0).is_some_and(|c| c.is_ascii_digit() || c == '-') {
                self.i += 1;
            }
            return;
        }
        w.skip_next = true;
    }

    /// Reads a heredoc's delimiter. Its body starts on the next line.
    fn heredoc_start(&mut self) {
        let strip_tabs = self.peek(0) == Some('-');
        if strip_tabs {
            self.i += 1;
        }
        while self.peek(0).is_some_and(|c| c == ' ' || c == '\t') {
            self.i += 1;
        }
        let mut delimiter = String::new();
        while let Some(c) = self.peek(0) {
            if c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')' | '<' | '>') {
                break;
            }
            self.i += 1;
            if !matches!(c, '\'' | '"' | '\\') {
                delimiter.push(c);
            }
        }
        if !delimiter.is_empty() {
            self.heredocs.push(Heredoc {
                delimiter,
                strip_tabs,
            });
        }
    }

    /// After a newline, skips the bodies of any heredocs started on that line.
    fn skip_heredoc_bodies(&mut self) {
        for doc in std::mem::take(&mut self.heredocs) {
            loop {
                let start = self.i;
                while self.peek(0).is_some_and(|c| c != '\n') {
                    self.i += 1;
                }
                let line: String = self.chars[start..self.i].iter().collect();
                if self.peek(0).is_some() {
                    self.i += 1;
                }
                let line = if doc.strip_tabs {
                    line.trim_start_matches('\t')
                } else {
                    line.as_str()
                };
                if line == doc.delimiter || self.peek(0).is_none() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds(src: &str) -> Vec<Vec<&str>> {
        // Leak for terse assertions; tests only.
        commands(src)
            .into_iter()
            .map(|c| {
                c.into_iter()
                    .map(|w| &*Box::leak(w.into_boxed_str()))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn splits_on_operators() {
        assert_eq!(
            cmds("cd a && kubectl get pods | grep x; echo done || true"),
            vec![
                vec!["cd", "a"],
                vec!["kubectl", "get", "pods"],
                vec!["grep", "x"],
                vec!["echo", "done"],
                vec!["true"],
            ]
        );
    }

    #[test]
    fn removes_quotes() {
        assert_eq!(
            cmds(r#"ssh samwise 'sudo k3s kubectl get pods' "a b" c\ d"#),
            vec![vec![
                "ssh",
                "samwise",
                "sudo k3s kubectl get pods",
                "a b",
                "c d"
            ]]
        );
        assert_eq!(cmds(r#"echo "" x"#), vec![vec!["echo", "", "x"]]);
    }

    #[test]
    fn finds_commands_inside_substitutions() {
        assert_eq!(
            cmds(r#"echo "pods: $(kubectl delete pod x)" `helm list`"#),
            vec![
                vec!["kubectl", "delete", "pod", "x"],
                vec!["helm", "list"],
                vec!["echo", "pods: …", "…"],
            ]
        );
    }

    #[test]
    fn subshells_and_groups() {
        assert_eq!(
            cmds("(cd x; git push) && { kubectl apply -f y; }"),
            vec![
                vec!["cd", "x"],
                vec!["git", "push"],
                vec!["{", "kubectl", "apply", "-f", "y"],
                vec!["}"],
            ]
        );
    }

    #[test]
    fn drops_redirections() {
        assert_eq!(
            cmds("kubectl get pods 2>&1 >out.txt < in && echo ok >&2"),
            vec![vec!["kubectl", "get", "pods"], vec!["echo", "ok"]]
        );
        assert_eq!(cmds("cat <<< 'x'"), vec![vec!["cat"]]);
    }

    #[test]
    fn skips_heredoc_bodies() {
        let src = "kubectl apply -f - <<'EOF'\nkind: Pod\nkubectl delete all\nEOF\ngit push";
        assert_eq!(
            cmds(src),
            vec![vec!["kubectl", "apply", "-f", "-"], vec!["git", "push"]]
        );
        let tabs = "cat <<-END\n\tssh prod\n\tEND\nls";
        assert_eq!(cmds(tabs), vec![vec!["cat"], vec!["ls"]]);
    }

    #[test]
    fn comments_and_line_continuations() {
        assert_eq!(
            cmds("kubectl \\\n  get pods # not: kubectl delete\nls"),
            vec![vec!["kubectl", "get", "pods"], vec!["ls"]]
        );
        assert_eq!(cmds("echo a#b"), vec![vec!["echo", "a#b"]]);
    }

    #[test]
    fn unterminated_input_still_returns_what_it_found() {
        assert_eq!(
            cmds("kubectl apply 'oops"),
            vec![vec!["kubectl", "apply", "oops"]]
        );
        assert_eq!(
            cmds("echo $(git push"),
            vec![vec!["git", "push"], vec!["echo", "…"]]
        );
    }
}
