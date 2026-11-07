//! Minimal `.env` parser, compatible with the docker compose flavour:
//!
//! - Blank lines and lines starting with `#` are ignored.
//! - An optional `export ` prefix is allowed.
//! - Unquoted values are taken literally up to the end of the line (quotes
//!   inside them are kept as-is, so `KEY={"a":"b"}` works). A `#` preceded
//!   by whitespace starts an inline comment. Trailing whitespace is trimmed.
//! - Single-quoted values are fully literal and may span multiple lines.
//! - Double-quoted values support `\n`, `\r`, `\t`, `\"`, `\\` and `\$`
//!   escapes and may span multiple lines.
//! - `$VAR` and `${VAR}` are expanded in unquoted and double-quoted values,
//!   looking up previously defined keys first, then the process environment.

use std::collections::HashMap;

pub fn parse(
    input: &str,
    context: &HashMap<String, String>,
) -> Result<Vec<(String, String)>, String> {
    let mut parser = Parser {
        chars: input.chars().collect(),
        pos: 0,
        line: 1,
    };
    let mut scope = context.clone();
    let mut entries = Vec::new();

    while let Some((key, value)) = parser.next_entry(&scope)? {
        scope.insert(key.clone(), value.clone());
        entries.push((key, value));
    }

    Ok(entries)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    line: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn skip_inline_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.bump();
        }
    }

    fn skip_to_eol(&mut self) {
        while let Some(c) = self.bump() {
            if c == '\n' {
                break;
            }
        }
    }

    fn err(&self, line: usize, msg: &str) -> String {
        format!("line {line}: {msg}")
    }

    fn next_entry(
        &mut self,
        scope: &HashMap<String, String>,
    ) -> Result<Option<(String, String)>, String> {
        loop {
            self.skip_inline_whitespace();
            match self.peek() {
                None => return Ok(None),
                Some('\n' | '\r') => {
                    self.bump();
                }
                Some('#') => self.skip_to_eol(),
                Some(_) => break,
            }
        }

        let line = self.line;
        let mut key = self.read_key();

        if key == "export" && matches!(self.peek(), Some(' ' | '\t')) {
            self.skip_inline_whitespace();
            key = self.read_key();
        }

        if key.is_empty() {
            return Err(self.err(line, "expected a variable name"));
        }

        self.skip_inline_whitespace();
        if self.peek() != Some('=') {
            return Err(self.err(line, &format!("expected `=` after `{key}`")));
        }
        self.bump();
        self.skip_inline_whitespace();

        let value = match self.peek() {
            Some('\'') => {
                let v = self.read_single_quoted(line)?;
                self.finish_quoted_line(line)?;
                v
            }
            Some('"') => {
                let v = self.read_double_quoted(line, scope)?;
                self.finish_quoted_line(line)?;
                v
            }
            _ => self.read_unquoted(scope),
        };

        Ok(Some((key, value)))
    }

    fn read_key(&mut self) -> String {
        let mut key = String::new();
        while let Some(c) = self.peek() {
            let valid = if key.is_empty() {
                c.is_ascii_alphabetic() || c == '_'
            } else {
                c.is_ascii_alphanumeric() || c == '_' || c == '.'
            };
            if !valid {
                break;
            }
            key.push(c);
            self.bump();
        }
        key
    }

    fn read_single_quoted(&mut self, line: usize) -> Result<String, String> {
        self.bump(); // opening quote
        let mut value = String::new();
        loop {
            match self.bump() {
                None => return Err(self.err(line, "unterminated single-quoted value")),
                Some('\'') => return Ok(value),
                Some(c) => value.push(c),
            }
        }
    }

    fn read_double_quoted(
        &mut self,
        line: usize,
        scope: &HashMap<String, String>,
    ) -> Result<String, String> {
        self.bump(); // opening quote
        let mut value = String::new();
        loop {
            match self.bump() {
                None => return Err(self.err(line, "unterminated double-quoted value")),
                Some('"') => return Ok(value),
                Some('\\') => match self.bump() {
                    Some('n') => value.push('\n'),
                    Some('r') => value.push('\r'),
                    Some('t') => value.push('\t'),
                    Some(c @ ('"' | '\\' | '$')) => value.push(c),
                    Some(c) => {
                        value.push('\\');
                        value.push(c);
                    }
                    None => return Err(self.err(line, "unterminated double-quoted value")),
                },
                Some('$') => self.expand(&mut value, scope),
                Some(c) => value.push(c),
            }
        }
    }

    /// After a closing quote only whitespace or a comment may follow.
    fn finish_quoted_line(&mut self, line: usize) -> Result<(), String> {
        self.skip_inline_whitespace();
        match self.peek() {
            None => Ok(()),
            Some('\n') => {
                self.bump();
                Ok(())
            }
            Some('\r') if self.chars.get(self.pos + 1) == Some(&'\n') => {
                self.bump();
                self.bump();
                Ok(())
            }
            Some('#') => {
                self.skip_to_eol();
                Ok(())
            }
            Some(_) => Err(self.err(line, "unexpected characters after closing quote")),
        }
    }

    fn read_unquoted(&mut self, scope: &HashMap<String, String>) -> String {
        let mut value = String::new();
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            if c == '#' && (value.is_empty() || value.ends_with([' ', '\t'])) {
                // Inline comment; `value` is empty only for `KEY= #comment`
                // since leading whitespace was already skipped.
                self.skip_to_eol();
                return value.trim_end().to_string();
            }
            self.bump();
            if c == '$' {
                self.expand(&mut value, scope);
            } else {
                value.push(c);
            }
        }
        self.bump(); // newline, if any
        value.trim_end().to_string()
    }

    /// Expands `$VAR` or `${VAR}`; the `$` has already been consumed.
    fn expand(&mut self, value: &mut String, scope: &HashMap<String, String>) {
        let braced = self.peek() == Some('{');
        let start = self.pos;
        if braced {
            self.bump();
        }

        let name = self.read_key();

        if name.is_empty() || (braced && self.peek() != Some('}')) {
            // Not a valid reference; keep it literally.
            self.pos = start;
            value.push('$');
            return;
        }
        if braced {
            self.bump();
        }

        if let Some(v) = scope.get(&name) {
            value.push_str(v);
        } else if let Ok(v) = std::env::var(&name) {
            value.push_str(&v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(input: &str) -> Vec<(String, String)> {
        parse(input, &HashMap::new()).unwrap()
    }

    fn get(input: &str, key: &str) -> String {
        parse_ok(input)
            .into_iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
            .unwrap()
    }

    #[test]
    fn unquoted_json_is_kept_literally() {
        assert_eq!(get("SOME_ENV={\"a\":\"test\"}\n", "SOME_ENV"), r#"{"a":"test"}"#);
        assert_eq!(
            get("SOME_ENV={\"a\": \"test\", \"b\": [1, 2]}\n", "SOME_ENV"),
            r#"{"a": "test", "b": [1, 2]}"#
        );
    }

    #[test]
    fn single_quoted_json_is_unwrapped() {
        assert_eq!(get("SOME_ENV='{\"a\":\"test\"}'\n", "SOME_ENV"), r#"{"a":"test"}"#);
    }

    #[test]
    fn skips_blank_lines_and_comments() {
        let entries = parse_ok("\n# comment\n  # indented\nFOO=1\n\nBAR=2");
        assert_eq!(
            entries,
            vec![("FOO".into(), "1".into()), ("BAR".into(), "2".into())]
        );
    }

    #[test]
    fn supports_export_prefix_and_spaces_around_equals() {
        assert_eq!(get("export FOO = bar\n", "FOO"), "bar");
    }

    #[test]
    fn key_named_export_still_works() {
        assert_eq!(get("export=1\n", "export"), "1");
    }

    #[test]
    fn unquoted_strips_inline_comments_and_trailing_whitespace() {
        assert_eq!(get("FOO=bar # comment\n", "FOO"), "bar");
        assert_eq!(get("FOO=bar   \n", "FOO"), "bar");
        assert_eq!(get("FOO=a#b\n", "FOO"), "a#b");
        assert_eq!(get("FOO=\n", "FOO"), "");
        assert_eq!(get("FOO= # nothing\n", "FOO"), "");
    }

    #[test]
    fn handles_crlf_line_endings() {
        let entries = parse_ok("FOO=1\r\nBAR=\"2\"\r\nBAZ='3'\r\n");
        assert_eq!(
            entries,
            vec![
                ("FOO".into(), "1".into()),
                ("BAR".into(), "2".into()),
                ("BAZ".into(), "3".into()),
            ]
        );
    }

    #[test]
    fn single_quoted_is_literal_and_multiline() {
        assert_eq!(get("FOO='a $BAR \\n b'\n", "FOO"), "a $BAR \\n b");
        assert_eq!(get("FOO='line1\nline2'\n", "FOO"), "line1\nline2");
    }

    #[test]
    fn double_quoted_handles_escapes_and_multiline() {
        assert_eq!(get(r#"FOO="a\nb\t\"c\" \\ \$X""#, "FOO"), "a\nb\t\"c\" \\ $X");
        assert_eq!(get("FOO=\"line1\nline2\"\n", "FOO"), "line1\nline2");
        assert_eq!(get(r#"FOO="a # not a comment""#, "FOO"), "a # not a comment");
    }

    #[test]
    fn quoted_value_allows_trailing_comment() {
        assert_eq!(get("FOO=\"bar\" # comment\n", "FOO"), "bar");
        assert_eq!(get("FOO='bar'   # comment\n", "FOO"), "bar");
    }

    #[test]
    fn expands_variables_in_unquoted_and_double_quoted() {
        let input = "HOST=localhost\nA=http://$HOST:3000\nB=\"${HOST}/x\"\nC='$HOST'\n";
        assert_eq!(get(input, "A"), "http://localhost:3000");
        assert_eq!(get(input, "B"), "localhost/x");
        assert_eq!(get(input, "C"), "$HOST");
    }

    #[test]
    fn expands_from_context() {
        let mut ctx = HashMap::new();
        ctx.insert("BASE".to_string(), "/srv".to_string());
        let entries = parse("DIR=$BASE/app\n", &ctx).unwrap();
        assert_eq!(entries, vec![("DIR".into(), "/srv/app".into())]);
    }

    #[test]
    fn keeps_lone_or_invalid_dollar_literally() {
        assert_eq!(get("FOO=cost $5\n", "FOO"), "cost $5");
        assert_eq!(get("FOO=a$\n", "FOO"), "a$");
        assert_eq!(get("FOO=${unterminated\n", "FOO"), "${unterminated");
    }

    #[test]
    fn errors_on_invalid_lines() {
        assert!(parse("this is not valid\n", &HashMap::new()).is_err());
        assert!(parse("=value\n", &HashMap::new()).is_err());
        assert!(parse("FOO='unterminated\n", &HashMap::new()).is_err());
        assert!(parse("FOO=\"unterminated\n", &HashMap::new()).is_err());
        assert!(parse("FOO=\"a\"b\n", &HashMap::new()).is_err());
    }

    #[test]
    fn error_reports_line_number() {
        let err = parse("FOO=1\n\nnope\n", &HashMap::new()).unwrap_err();
        assert!(err.starts_with("line 3:"), "{err}");
    }
}
