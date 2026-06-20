use regex::{Match as RegexMatch, Regex};
use std::collections::HashMap;
use std::fmt;

const EXCLUDE_PATTERN_DEFS: [(&str, &str); 1] =
    [("bash", r"[[:cntrl:]]\[([0-9]{1,2};)?([0-9]{1,2})?m")];

const BUILTIN_PATTERN_DEFS: [(&str, &str); 15] = [
    ("markdown_url", r"\[[^]]*\]\(([^)]+)\)"),
    (
        "url",
        r"(?P<match>(https?://|git@|git://|ssh://|ftp://|file:///)[^ ]+)",
    ),
    (
        "diff_summary",
        r"diff --git a/([.\w\-@~\[\]]+?/[.\w\-@\[\]]++) b/([.\w\-@~\[\]]+?/[.\w\-@\[\]]++)",
    ),
    ("diff_a", r"--- a/([^ ]+)"),
    ("diff_b", r"\+\+\+ b/([^ ]+)"),
    ("docker", r"sha256:([0-9a-f]{64})"),
    ("path", r"(?P<match>([.\w\-@$~\[\]]+)?(/[.\w\-@$\[\]]+)+)"),
    ("color", r"#[0-9a-fA-F]{6}"),
    (
        "uid",
        r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
    ),
    ("ipfs", r"Qm[0-9a-zA-Z]{44}"),
    ("sha", r"[0-9a-f]{7,40}"),
    ("ip", r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}"),
    ("ipv6", r"[A-f0-9:]+:+[A-f0-9:]+[%\w\d]+"),
    ("address", r"0x[0-9a-fA-F]+"),
    ("number", r"[0-9]{4,}"),
];

lazy_static! {
    static ref EXCLUDE_PATTERNS: Vec<Pattern> = compile_pattern_defs(&EXCLUDE_PATTERN_DEFS);
    static ref BUILTIN_PATTERNS: Vec<Pattern> = compile_pattern_defs(&BUILTIN_PATTERN_DEFS);
}

#[derive(Debug)]
pub struct StateError {
    message: String,
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for StateError {}

struct Pattern {
    name: &'static str,
    regex: Regex,
}

fn compile_pattern_defs(defs: &[(&'static str, &'static str)]) -> Vec<Pattern> {
    defs.iter()
        .map(|(name, pattern)| Pattern {
            name,
            regex: Regex::new(pattern).unwrap(),
        })
        .collect()
}

fn compile_custom_patterns(regexps: &[&str]) -> Result<Vec<Pattern>, StateError> {
    regexps
        .iter()
        .map(|regexp| {
            Regex::new(regexp)
                .map(|regex| Pattern {
                    name: "custom",
                    regex,
                })
                .map_err(|error| StateError {
                    message: format!("Invalid custom regexp `{}`: {}", regexp, error),
                })
        })
        .collect()
}

#[derive(Clone)]
pub struct Match<'a> {
    pub x: usize,
    pub y: usize,
    pub pattern: &'a str,
    pub text: &'a str,
    pub hint: Option<String>,
}

impl<'a> fmt::Debug for Match<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Match {{ x: {}, y: {}, pattern: {}, text: {}, hint: <{}> }}",
            self.x,
            self.y,
            self.pattern,
            self.text,
            self.hint.clone().unwrap_or("<undefined>".to_string())
        )
    }
}

impl<'a> PartialEq for Match<'a> {
    fn eq(&self, other: &Match) -> bool {
        self.x == other.x && self.y == other.y
    }
}

pub struct State<'a> {
    pub lines: &'a Vec<&'a str>,
    alphabet: &'a str,
    custom_patterns: Vec<Pattern>,
}

impl<'a> State<'a> {
    pub fn new(lines: &'a Vec<&'a str>, alphabet: &'a str, regexp: &'a Vec<&'a str>) -> State<'a> {
        State::try_new(lines, alphabet, regexp).unwrap_or_else(|error| panic!("{}", error))
    }

    pub fn try_new(
        lines: &'a Vec<&'a str>,
        alphabet: &'a str,
        regexp: &'a Vec<&'a str>,
    ) -> Result<State<'a>, StateError> {
        Ok(State {
            lines,
            alphabet,
            custom_patterns: compile_custom_patterns(regexp)?,
        })
    }

    pub fn matches(&self, reverse: bool, unique: bool) -> Vec<Match<'a>> {
        let mut matches = self.raw_matches();

        self.assign_hints(&mut matches, reverse, unique);

        matches
    }

    fn raw_matches(&self) -> Vec<Match<'a>> {
        let patterns = self.pattern_priority();

        self.lines
            .iter()
            .enumerate()
            .flat_map(|(index, line)| scan_line(index, line, &patterns))
            .collect()
    }

    fn pattern_priority(&self) -> Vec<&Pattern> {
        EXCLUDE_PATTERNS
            .iter()
            .chain(self.custom_patterns.iter())
            .chain(BUILTIN_PATTERNS.iter())
            .collect()
    }

    fn assign_hints(&self, matches: &mut Vec<Match<'a>>, reverse: bool, unique: bool) {
        let alphabet = super::alphabets::get_alphabet(self.alphabet);
        let mut hints = alphabet.hints(matches.len());

        // This looks wrong but we do a pop after
        if !reverse {
            hints.reverse();
        } else {
            matches.reverse();
            hints.reverse();
        }

        if unique {
            let mut previous: HashMap<&str, String> = HashMap::new();

            for mat in matches.iter_mut() {
                if let Some(previous_hint) = previous.get(mat.text) {
                    mat.hint = Some(previous_hint.clone());
                } else if let Some(hint) = hints.pop() {
                    mat.hint = Some(hint.clone());
                    previous.insert(mat.text, hint);
                }
            }
        } else {
            for mat in matches.iter_mut() {
                if let Some(hint) = hints.pop() {
                    mat.hint = Some(hint);
                }
            }
        }

        if reverse {
            matches.reverse();
        }
    }
}

fn scan_line<'a>(line_index: usize, line: &'a str, patterns: &[&Pattern]) -> Vec<Match<'a>> {
    let mut matches = Vec::new();
    let mut chunk = line;
    let mut offset = 0;

    while let Some((pattern, matching)) = first_pattern_match(chunk, patterns) {
        let text = matching.as_str();
        let captures = extract_captures(&pattern.regex, text);

        // Never hint or break bash color sequences, but process them.
        if pattern.name != "bash" {
            for (subtext, substart) in captures {
                matches.push(Match {
                    x: offset + matching.start() + substart,
                    y: line_index,
                    pattern: pattern.name,
                    text: subtext,
                    hint: None,
                });
            }
        }

        chunk = chunk.get(matching.end()..).expect("Unknown chunk");
        offset += matching.end();
    }

    matches
}

fn first_pattern_match<'a, 'p>(
    chunk: &'a str,
    patterns: &[&'p Pattern],
) -> Option<(&'p Pattern, RegexMatch<'a>)> {
    patterns
        .iter()
        .filter_map(|pattern| {
            pattern
                .regex
                .find(chunk)
                .map(|matching| (*pattern, matching))
        })
        .min_by(|x, y| x.1.start().cmp(&y.1.start()))
}

fn extract_captures<'a>(pattern: &Regex, text: &'a str) -> Vec<(&'a str, usize)> {
    let captures = pattern.captures(text).expect("No matching?");

    if let Some(capture) = captures.name("match") {
        return vec![(capture.as_str(), capture.start())];
    }

    if captures.len() > 1 {
        return captures
            .iter()
            .skip(1)
            .flatten()
            .map(|capture| (capture.as_str(), capture.start()))
            .collect();
    }

    vec![(text, 0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(output: &str) -> Vec<&str> {
        output.split('\n').collect::<Vec<&str>>()
    }

    #[test]
    fn match_reverse() {
        let lines = split("lorem 127.0.0.1 lorem 255.255.255.255 lorem 127.0.0.1 lorem");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 3);
        assert_eq!(results.first().unwrap().hint.clone().unwrap(), "a");
        assert_eq!(results.last().unwrap().hint.clone().unwrap(), "c");
    }

    #[test]
    fn match_unique() {
        let lines = split("lorem 127.0.0.1 lorem 255.255.255.255 lorem 127.0.0.1 lorem");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, true);

        assert_eq!(results.len(), 3);
        assert_eq!(results.first().unwrap().hint.clone().unwrap(), "a");
        assert_eq!(results.last().unwrap().hint.clone().unwrap(), "a");
    }

    #[test]
    fn match_coordinates_are_byte_indexes() {
        let lines = split("λ 127.0.0.1");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.first().unwrap().x, "λ ".len());
        assert_eq!(results.first().unwrap().y, 0);
    }

    #[test]
    fn try_new_rejects_invalid_custom_regexp() {
        let lines = split("anything");
        let custom = ["["].to_vec();
        let error = State::try_new(&lines, "abcd", &custom).err().unwrap();

        assert!(error.to_string().contains("Invalid custom regexp"));
    }

    #[test]
    fn custom_named_capture_uses_match_group() {
        let lines = split("prefix-123");
        let custom = [r"prefix-(?P<match>\d+)"].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 1);
        assert_eq!(results.first().unwrap().text, "123");
        assert_eq!(results.first().unwrap().x, "prefix-".len());
    }

    #[test]
    fn custom_multiple_capture_groups_become_matches() {
        let lines = split("left-123");
        let custom = [r"(left)-(\d+)"].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 2);
        assert_eq!(results.first().unwrap().text, "left");
        assert_eq!(results.get(1).unwrap().text, "123");
        assert_eq!(results.get(1).unwrap().x, "left-".len());
    }

    #[test]
    fn custom_regex_without_captures_uses_full_match() {
        let lines = split("CUSTOM-123");
        let custom = [r"CUSTOM-\d+"].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 1);
        assert_eq!(results.first().unwrap().text, "CUSTOM-123");
        assert_eq!(results.first().unwrap().x, 0);
    }

    #[test]
    fn custom_regex_wins_priority_tie_with_builtin() {
        let lines = split("http://foo.bar");
        let custom = [r"http://foo\.bar"].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 1);
        assert_eq!(results.first().unwrap().pattern, "custom");
        assert_eq!(results.first().unwrap().text, "http://foo.bar");
    }

    #[test]
    fn match_docker() {
        let lines = split("latest sha256:30557a29d5abc51e5f1d5b472e79b7e296f595abcf19fe6b9199dbbc809c6ff4 20 hours ago");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 1);
        assert_eq!(
            results.first().unwrap().text,
            "30557a29d5abc51e5f1d5b472e79b7e296f595abcf19fe6b9199dbbc809c6ff4"
        );
    }

    #[test]
    fn match_bash() {
        let lines = split("path: [32m/var/log/nginx.log[m\npath: [32mtest/log/nginx-2.log:32[mfolder/.nginx@4df2.log");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 3);
        assert_eq!(results.first().unwrap().text, "/var/log/nginx.log");
        assert_eq!(results.get(1).unwrap().text, "test/log/nginx-2.log");
        assert_eq!(results.get(2).unwrap().text, "folder/.nginx@4df2.log");
    }

    #[test]
    fn match_paths() {
        let lines = split("Lorem /tmp/foo/bar_lol, lorem\n Lorem /var/log/boot-strap.log lorem ../log/kern.log lorem");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 3);
        assert_eq!(results.first().unwrap().text, "/tmp/foo/bar_lol");
        assert_eq!(results.get(1).unwrap().text, "/var/log/boot-strap.log");
        assert_eq!(results.get(2).unwrap().text, "../log/kern.log");
    }

    #[test]
    fn match_routes() {
        let lines =
            split("Lorem /app/routes/$routeId/$objectId, lorem\n Lorem /app/routes/$sectionId");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 2);
        assert_eq!(
            results.first().unwrap().text,
            "/app/routes/$routeId/$objectId"
        );
        assert_eq!(results.get(1).unwrap().text, "/app/routes/$sectionId");
    }

    #[test]
    fn match_home() {
        let lines = split("Lorem ~/.gnu/.config.txt, lorem");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 1);
        assert_eq!(results.first().unwrap().text, "~/.gnu/.config.txt");
    }

    #[test]
    fn match_slugs() {
        let lines = split("Lorem dev/api/[slug]/foo, lorem");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 1);
        assert_eq!(results.first().unwrap().text, "dev/api/[slug]/foo");
    }

    #[test]
    fn match_uids() {
        let lines =
            split("Lorem ipsum 123e4567-e89b-12d3-a456-426655440000 lorem\n Lorem lorem lorem");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 1);
    }

    #[test]
    fn match_shas() {
        let lines = split("Lorem fd70b5695 5246ddf f924213 lorem\n Lorem 973113963b491874ab2e372ee60d4b4cb75f717c lorem");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 4);
        assert_eq!(results.first().unwrap().text, "fd70b5695");
        assert_eq!(results.get(1).unwrap().text, "5246ddf");
        assert_eq!(results.get(2).unwrap().text, "f924213");
        assert_eq!(
            results.get(3).unwrap().text,
            "973113963b491874ab2e372ee60d4b4cb75f717c"
        );
    }

    #[test]
    fn match_ips() {
        let lines =
            split("Lorem ipsum 127.0.0.1 lorem\n Lorem 255.255.10.255 lorem 127.0.0.1 lorem");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 3);
        assert_eq!(results.first().unwrap().text, "127.0.0.1");
        assert_eq!(results.get(1).unwrap().text, "255.255.10.255");
        assert_eq!(results.get(2).unwrap().text, "127.0.0.1");
    }

    #[test]
    fn match_ipv6s() {
        let lines = split("Lorem ipsum fe80::2:202:fe4 lorem\n Lorem 2001:67c:670:202:7ba8:5e41:1591:d723 lorem fe80::2:1 lorem ipsum fe80:22:312:fe::1%eth0");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 4);
        assert_eq!(results.first().unwrap().text, "fe80::2:202:fe4");
        assert_eq!(
            results.get(1).unwrap().text,
            "2001:67c:670:202:7ba8:5e41:1591:d723"
        );
        assert_eq!(results.get(2).unwrap().text, "fe80::2:1");
        assert_eq!(results.get(3).unwrap().text, "fe80:22:312:fe::1%eth0");
    }

    #[test]
    fn match_markdown_urls() {
        let lines = split(
            "Lorem ipsum [link](https://github.io?foo=bar) ![](http://cdn.com/img.jpg) lorem",
        );
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 2);
        assert_eq!(results.first().unwrap().pattern, "markdown_url");
        assert_eq!(results.first().unwrap().text, "https://github.io?foo=bar");
        assert_eq!(results.get(1).unwrap().pattern, "markdown_url");
        assert_eq!(results.get(1).unwrap().text, "http://cdn.com/img.jpg");
    }

    #[test]
    fn match_urls() {
        let lines = split("Lorem ipsum https://www.rust-lang.org/tools lorem\n Lorem ipsumhttps://crates.io lorem https://github.io?foo=bar lorem ssh://github.io");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 4);
        assert_eq!(
            results.first().unwrap().text,
            "https://www.rust-lang.org/tools"
        );
        assert_eq!(results.first().unwrap().pattern, "url");
        assert_eq!(results.get(1).unwrap().text, "https://crates.io");
        assert_eq!(results.get(1).unwrap().pattern, "url");
        assert_eq!(results.get(2).unwrap().text, "https://github.io?foo=bar");
        assert_eq!(results.get(2).unwrap().pattern, "url");
        assert_eq!(results.get(3).unwrap().text, "ssh://github.io");
        assert_eq!(results.get(3).unwrap().pattern, "url");
    }

    #[test]
    fn match_addresses() {
        let lines = split("Lorem 0xfd70b5695 0x5246ddf lorem\n Lorem 0x973113tlorem");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 3);
        assert_eq!(results.first().unwrap().text, "0xfd70b5695");
        assert_eq!(results.get(1).unwrap().text, "0x5246ddf");
        assert_eq!(results.get(2).unwrap().text, "0x973113");
    }

    #[test]
    fn match_hex_colors() {
        let lines =
            split("Lorem #fd7b56 lorem #FF00FF\n Lorem #00fF05 lorem #abcd00 lorem #afRR00");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 4);
        assert_eq!(results.first().unwrap().text, "#fd7b56");
        assert_eq!(results.get(1).unwrap().text, "#FF00FF");
        assert_eq!(results.get(2).unwrap().text, "#00fF05");
        assert_eq!(results.get(3).unwrap().text, "#abcd00");
    }

    #[test]
    fn match_ipfs() {
        let lines = split("Lorem QmRdbNSxDJBXmssAc9fvTtux4duptMvfSGiGuq6yHAQVKQ lorem Qmfoobar");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 1);
        assert_eq!(
            results.first().unwrap().text,
            "QmRdbNSxDJBXmssAc9fvTtux4duptMvfSGiGuq6yHAQVKQ"
        );
    }

    #[test]
    fn match_process_port() {
        let lines =
      split("Lorem 5695 52463 lorem\n Lorem 973113 lorem 99999 lorem 8888 lorem\n   23456 lorem 5432 lorem 23444");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 8);
    }

    #[test]
    fn match_diff_a() {
        let lines = split("Lorem lorem\n--- a/src/main.rs");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 1);
        assert_eq!(results.first().unwrap().text, "src/main.rs");
    }

    #[test]
    fn match_diff_b() {
        let lines = split("Lorem lorem\n+++ b/src/main.rs");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 1);
        assert_eq!(results.first().unwrap().text, "src/main.rs");
    }

    #[test]
    fn match_diff_summary() {
        let lines = split("diff --git a/samples/test1 b/samples/test2");
        let custom = [].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 2);
        assert_eq!(results.first().unwrap().text, "samples/test1");
        assert_eq!(results.get(1).unwrap().text, "samples/test2");
    }

    #[test]
    fn priority() {
        let lines = split("Lorem [link](http://foo.bar) ipsum CUSTOM-52463 lorem ISSUE-123 lorem\nLorem /var/fd70b569/9999.log 52463 lorem\n Lorem 973113 lorem 123e4567-e89b-12d3-a456-426655440000 lorem 8888 lorem\n  https://crates.io/23456/fd70b569 lorem");
        let custom = ["CUSTOM-[0-9]{4,}", "ISSUE-[0-9]{3}"].to_vec();
        let results = State::new(&lines, "abcd", &custom).matches(false, false);

        assert_eq!(results.len(), 9);
        assert_eq!(results.first().unwrap().text, "http://foo.bar");
        assert_eq!(results.get(1).unwrap().text, "CUSTOM-52463");
        assert_eq!(results.get(2).unwrap().text, "ISSUE-123");
        assert_eq!(results.get(3).unwrap().text, "/var/fd70b569/9999.log");
        assert_eq!(results.get(4).unwrap().text, "52463");
        assert_eq!(results.get(5).unwrap().text, "973113");
        assert_eq!(
            results.get(6).unwrap().text,
            "123e4567-e89b-12d3-a456-426655440000"
        );
        assert_eq!(results.get(7).unwrap().text, "8888");
        assert_eq!(
            results.get(8).unwrap().text,
            "https://crates.io/23456/fd70b569"
        );
    }
}
