use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Selection {
    pub(crate) text: String,
    pub(crate) upcase: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectionSet {
    selections: Vec<Selection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectionParseError(String);

impl fmt::Display for SelectionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Selection {
    fn parse_line(line: &str, line_number: usize) -> Result<Selection, SelectionParseError> {
        let Some((upcase, text)) = line.split_once(':') else {
            return Err(SelectionParseError(format!(
                "malformed selection line {}: expected `%U:%H`",
                line_number
            )));
        };

        let upcase = match upcase.trim_end() {
            "true" => true,
            "false" => false,
            value => {
                return Err(SelectionParseError(format!(
                    "invalid selection upcase flag on line {}: {}",
                    line_number, value
                )));
            }
        };

        Ok(Selection {
            text: text.to_string(),
            upcase,
        })
    }
}

impl SelectionSet {
    pub(crate) fn parse(content: &str) -> Result<SelectionSet, SelectionParseError> {
        let selections = content
            .lines()
            .enumerate()
            .map(|(index, line)| Selection::parse_line(line, index + 1))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(SelectionSet { selections })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.selections.is_empty()
    }

    pub(crate) fn is_multi(&self) -> bool {
        self.selections.len() > 1
    }

    pub(crate) fn single(&self) -> Option<&Selection> {
        self.selections
            .first()
            .filter(|_| self.selections.len() == 1)
    }

    pub(crate) fn multi_text(&self) -> String {
        self.selections
            .iter()
            .map(|selection| selection.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_selection_set_keeps_colons_in_text() {
        let selections = SelectionSet::parse("false:https://example.com/a:b").unwrap();
        let selection = selections.single().unwrap();

        assert_eq!(selection.text, "https://example.com/a:b");
        assert!(!selection.upcase);
    }

    #[test]
    fn parse_selection_set_joins_multi_selection_text() {
        let selections = SelectionSet::parse("false:first\ntrue:second").unwrap();

        assert!(selections.is_multi());
        assert_eq!(selections.multi_text(), "first second");
    }

    #[test]
    fn parse_selection_set_reports_malformed_line() {
        let error = SelectionSet::parse("false:first\nnot-a-selection").unwrap_err();

        assert_eq!(
            error.to_string(),
            "malformed selection line 2: expected `%U:%H`"
        );
    }
}
