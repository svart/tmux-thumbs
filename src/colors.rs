use regex::Regex;
use termion::color;

pub fn get_color(color_name: &str) -> Box<dyn color::Color> {
  lazy_static! {
    static ref RGB: Regex = Regex::new(r"#([[:xdigit:]]{2})([[:xdigit:]]{2})([[:xdigit:]]{2})").unwrap();
  }

  if let Some(captures) = RGB.captures(color_name) {
    let r = u8::from_str_radix(captures.get(1).unwrap().as_str(), 16).unwrap();
    let g = u8::from_str_radix(captures.get(2).unwrap().as_str(), 16).unwrap();
    let b = u8::from_str_radix(captures.get(3).unwrap().as_str(), 16).unwrap();

    return Box::new(color::Rgb(r, g, b));
  }

  match color_name {
    "black" => Box::new(color::Black),
    "red" => Box::new(color::Red),
    "green" => Box::new(color::Green),
    "yellow" => Box::new(color::Yellow),
    "blue" => Box::new(color::Blue),
    "magenta" => Box::new(color::Magenta),
    "cyan" => Box::new(color::Cyan),
    "white" => Box::new(color::White),
    "default" => Box::new(color::Reset),
    _ => panic!("Unknown color: {}", color_name),
  }
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
