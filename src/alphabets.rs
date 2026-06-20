use std::fmt;

const ALPHABETS: [(&str, &str); 22] = [
    ("numeric", "1234567890"),
    ("abcd", "abcd"),
    ("qwerty", "asdfqwerzxcvjklmiuopghtybn"),
    ("qwerty-homerow", "asdfjklgh"),
    ("qwerty-left-hand", "asdfqwerzcxv"),
    ("qwerty-right-hand", "jkluiopmyhn"),
    ("azerty", "qsdfazerwxcvjklmuiopghtybn"),
    ("azerty-homerow", "qsdfjkmgh"),
    ("azerty-left-hand", "qsdfazerwxcv"),
    ("azerty-right-hand", "jklmuiophyn"),
    ("qwertz", "asdfqweryxcvjkluiopmghtzbn"),
    ("qwertz-homerow", "asdfghjkl"),
    ("qwertz-left-hand", "asdfqweryxcv"),
    ("qwertz-right-hand", "jkluiopmhzn"),
    ("dvorak", "aoeuqjkxpyhtnsgcrlmwvzfidb"),
    ("dvorak-homerow", "aoeuhtnsid"),
    ("dvorak-left-hand", "aoeupqjkyix"),
    ("dvorak-right-hand", "htnsgcrlmwvz"),
    ("colemak", "arstqwfpzxcvneioluymdhgjbk"),
    ("colemak-homerow", "arstneiodh"),
    ("colemak-left-hand", "arstqwfpzxcv"),
    ("colemak-right-hand", "neioluymjhk"),
];

pub struct Alphabet<'a> {
    letters: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlphabetParseError {
    name: String,
}

impl fmt::Display for AlphabetParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unknown alphabet: {}", self.name)
    }
}

impl std::error::Error for AlphabetParseError {}

impl<'a> Alphabet<'a> {
    fn new(letters: &'a str) -> Alphabet<'a> {
        Alphabet { letters }
    }

    pub fn hints(&self, matches: usize) -> Vec<String> {
        let letters: Vec<String> = self.letters.chars().map(|s| s.to_string()).collect();

        let mut expansion = letters.clone();
        let mut expanded: Vec<String> = Vec::new();

        loop {
            if expansion.len() + expanded.len() >= matches {
                break;
            }
            if expansion.is_empty() {
                break;
            }

            let prefix = expansion.pop().expect("Ouch!");
            let sub_expansion: Vec<String> = letters
                .iter()
                .take(matches - expansion.len() - expanded.len())
                .map(|s| prefix.clone() + s)
                .collect();

            expanded.splice(0..0, sub_expansion);
        }

        expansion = expansion
            .iter()
            .take(matches - expanded.len())
            .cloned()
            .collect();
        expansion.append(&mut expanded);
        expansion
    }
}

pub fn get_alphabet(alphabet_name: &str) -> Alphabet<'_> {
    let letters = alphabet_letters(alphabet_name).unwrap_or_else(|| {
        panic!(
            "{}",
            AlphabetParseError {
                name: alphabet_name.to_string()
            }
        )
    });

    Alphabet::new(letters)
}

pub fn validate_alphabet(alphabet_name: &str) -> Result<(), AlphabetParseError> {
    alphabet_letters(alphabet_name)
        .map(|_| ())
        .ok_or_else(|| AlphabetParseError {
            name: alphabet_name.to_string(),
        })
}

fn alphabet_letters(alphabet_name: &str) -> Option<&'static str> {
    ALPHABETS
        .iter()
        .find_map(|(name, letters)| (*name == alphabet_name).then_some(*letters))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_matches() {
        let alphabet = Alphabet::new("abcd");
        let hints = alphabet.hints(3);
        assert_eq!(hints, ["a", "b", "c"]);
    }

    #[test]
    fn composed_matches() {
        let alphabet = Alphabet::new("abcd");
        let hints = alphabet.hints(6);
        assert_eq!(hints, ["a", "b", "c", "da", "db", "dc"]);
    }

    #[test]
    fn composed_matches_multiple() {
        let alphabet = Alphabet::new("abcd");
        let hints = alphabet.hints(8);
        assert_eq!(hints, ["a", "b", "ca", "cb", "da", "db", "dc", "dd"]);
    }

    #[test]
    fn composed_matches_max() {
        let alphabet = Alphabet::new("ab");
        let hints = alphabet.hints(8);
        assert_eq!(hints, ["aa", "ab", "ba", "bb"]);
    }

    #[test]
    fn validate_known_alphabet() {
        assert!(validate_alphabet("qwerty").is_ok());
    }

    #[test]
    fn reject_unknown_alphabet() {
        let error = validate_alphabet("wat").unwrap_err();

        assert_eq!(error.to_string(), "Unknown alphabet: wat");
    }
}
