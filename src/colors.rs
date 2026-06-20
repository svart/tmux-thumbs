use regex::Regex;
use std::fmt;
use termion::color;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorSpec {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    Default,
    Rgb(u8, u8, u8),
}

impl ColorSpec {
    pub fn parse(color_name: &str) -> Result<ColorSpec, ColorParseError> {
        lazy_static! {
            static ref RGB: Regex =
                Regex::new(r"#([[:xdigit:]]{2})([[:xdigit:]]{2})([[:xdigit:]]{2})").unwrap();
        }

        if let Some(captures) = RGB.captures(color_name) {
            let r = u8::from_str_radix(captures.get(1).unwrap().as_str(), 16).unwrap();
            let g = u8::from_str_radix(captures.get(2).unwrap().as_str(), 16).unwrap();
            let b = u8::from_str_radix(captures.get(3).unwrap().as_str(), 16).unwrap();

            return Ok(ColorSpec::Rgb(r, g, b));
        }

        match color_name {
            "black" => Ok(ColorSpec::Black),
            "red" => Ok(ColorSpec::Red),
            "green" => Ok(ColorSpec::Green),
            "yellow" => Ok(ColorSpec::Yellow),
            "blue" => Ok(ColorSpec::Blue),
            "magenta" => Ok(ColorSpec::Magenta),
            "cyan" => Ok(ColorSpec::Cyan),
            "white" => Ok(ColorSpec::White),
            "default" => Ok(ColorSpec::Default),
            _ => Err(ColorParseError {
                value: color_name.to_string(),
            }),
        }
    }

    pub fn to_color(&self) -> Box<dyn color::Color> {
        match self {
            ColorSpec::Black => Box::new(color::Black),
            ColorSpec::Red => Box::new(color::Red),
            ColorSpec::Green => Box::new(color::Green),
            ColorSpec::Yellow => Box::new(color::Yellow),
            ColorSpec::Blue => Box::new(color::Blue),
            ColorSpec::Magenta => Box::new(color::Magenta),
            ColorSpec::Cyan => Box::new(color::Cyan),
            ColorSpec::White => Box::new(color::White),
            ColorSpec::Default => Box::new(color::Reset),
            ColorSpec::Rgb(r, g, b) => Box::new(color::Rgb(*r, *g, *b)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorParseError {
    value: String,
}

impl fmt::Display for ColorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unknown color: {}", self.value)
    }
}

impl std::error::Error for ColorParseError {}

pub fn get_color(color_name: &str) -> Box<dyn color::Color> {
    ColorSpec::parse(color_name)
        .unwrap_or_else(|error| panic!("{}", error))
        .to_color()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_color() {
        let text1 = format!("{}foo", color::Fg(&*get_color("green")));
        let text2 = format!("{}foo", color::Fg(color::Green));

        assert_eq!(text1, text2);
    }

    #[test]
    fn parse_rgb() {
        let text1 = format!("{}foo", color::Fg(&*get_color("#1b1cbf")));
        let text2 = format!("{}foo", color::Fg(color::Rgb(27, 28, 191)));

        assert_eq!(text1, text2);
    }

    #[test]
    fn parse_color_spec() {
        assert_eq!(ColorSpec::parse("green").unwrap(), ColorSpec::Green);
        assert_eq!(
            ColorSpec::parse("#1b1cbf").unwrap(),
            ColorSpec::Rgb(27, 28, 191)
        );
    }

    #[test]
    fn reject_invalid_color_spec() {
        let error = ColorSpec::parse("wat").unwrap_err();

        assert_eq!(error.to_string(), "Unknown color: wat");
    }

    #[test]
    #[should_panic]
    fn parse_invalid_rgb() {
        let _ = get_color("#1b1cbj");
    }

    #[test]
    #[should_panic]
    fn no_match_color() {
        let _ = get_color("wat");
    }
}
