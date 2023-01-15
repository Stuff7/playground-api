#![allow(dead_code)]
use std::fmt::Display;

pub const ESC: &str = "\x1b";
pub const CLEAR: &str = "\x1bc";
pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const CURSOR_HIDE: &str = "\x1b[?25l";
pub const CURSOR_SHOW: &str = "\x1b[?25h";
pub const CURSOR_START: &str = "\x1b[1;1H";
pub const ITALIC: &str = "\x1b[3m";
pub const UNDERLINE: &str = "\x1b[4m";

pub mod fg {
  pub const BLACK: &str = "\x1b[30m";
  pub const RED: &str = "\x1b[31m";
  pub const GREEN: &str = "\x1b[32m";
  pub const YELLOW: &str = "\x1b[33m";
  pub const BLUE: &str = "\x1b[34m";
  pub const MAGENTA: &str = "\x1b[35m";
  pub const CYAN: &str = "\x1b[36m";
  pub const WHITE: &str = "\x1b[37m";
  pub const DEFAULT: &str = "\x1b[39m";
  pub const BRIGHT_BLACK: &str = "\x1b[90m";
  pub const BRIGHT_RED: &str = "\x1b[91m";
  pub const BRIGHT_GREEN: &str = "\x1b[92m";
  pub const BRIGHT_YELLOW: &str = "\x1b[93m";
  pub const BRIGHT_BLUE: &str = "\x1b[94m";
  pub const BRIGHT_MAGENTA: &str = "\x1b[95m";
  pub const BRIGHT_CYAN: &str = "\x1b[96m";
  pub const BRIGHT_WHITE: &str = "\x1b[97m";

  pub fn rgb(r: u8, g: u8, b: u8) -> String {
    format!("{}[38;2;{r};{g};{b}m", super::ESC)
  }
}

pub mod bg {
  pub const BLACK: &str = "\x1b[40m";
  pub const RED: &str = "\x1b[41m";
  pub const GREEN: &str = "\x1b[42m";
  pub const YELLOW: &str = "\x1b[43m";
  pub const BLUE: &str = "\x1b[44m";
  pub const MAGENTA: &str = "\x1b[45m";
  pub const CYAN: &str = "\x1b[46m";
  pub const WHITE: &str = "\x1b[47m";
  pub const DEFAULT: &str = "\x1b[49m";
  pub const BRIGHT_BLACK: &str = "\x1b[100m";
  pub const BRIGHT_RED: &str = "\x1b[101m";
  pub const BRIGHT_GREEN: &str = "\x1b[102m";
  pub const BRIGHT_YELLOW: &str = "\x1b[103m";
  pub const BRIGHT_BLUE: &str = "\x1b[104m";
  pub const BRIGHT_MAGENTA: &str = "\x1b[105m";
  pub const BRIGHT_CYAN: &str = "\x1b[106m";
  pub const BRIGHT_WHITE: &str = "\x1b[107m";

  pub fn rgb(r: u8, g: u8, b: u8) -> String {
    format!("{}[48;2;{r};{g};{b}m", super::ESC)
  }
}

pub trait Colorize {
  fn black(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::BLACK, RESET)
  }
  fn red(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::RED, RESET)
  }
  fn green(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::GREEN, RESET)
  }
  fn yellow(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::YELLOW, RESET)
  }
  fn blue(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::BLUE, RESET)
  }
  fn magenta(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::MAGENTA, RESET)
  }
  fn cyan(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::CYAN, RESET)
  }
  fn white(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::WHITE, RESET)
  }
  fn idcolor(&self, id: u8) -> String
  where
    Self: Display,
  {
    format!("\x1b[38;5;{id}m{self}{RESET}")
  }
  fn rgb(&self, r: u8, g: u8, b: u8) -> String
  where
    Self: Display,
  {
    format!("\x1b[38;2;{r};{g};{b}m{self}{RESET}")
  }
  fn default(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::DEFAULT, RESET)
  }
  fn bblack(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::BRIGHT_BLACK, RESET)
  }
  fn bred(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::BRIGHT_RED, RESET)
  }
  fn bgreen(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::BRIGHT_GREEN, RESET)
  }
  fn byellow(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::BRIGHT_YELLOW, RESET)
  }
  fn bblue(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::BRIGHT_BLUE, RESET)
  }
  fn bmagenta(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::BRIGHT_MAGENTA, RESET)
  }
  fn bcyan(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::BRIGHT_CYAN, RESET)
  }
  fn bwhite(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", fg::BRIGHT_WHITE, RESET)
  }
  fn on_black(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::BLACK, RESET)
  }
  fn on_red(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::RED, RESET)
  }
  fn on_green(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::GREEN, RESET)
  }
  fn on_yellow(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::YELLOW, RESET)
  }
  fn on_blue(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::BLUE, RESET)
  }
  fn on_magenta(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::MAGENTA, RESET)
  }
  fn on_cyan(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::CYAN, RESET)
  }
  fn on_white(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::WHITE, RESET)
  }
  fn on_default(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::DEFAULT, RESET)
  }
  fn on_bblack(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::BRIGHT_BLACK, RESET)
  }
  fn on_bred(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::BRIGHT_RED, RESET)
  }
  fn on_bgreen(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::BRIGHT_GREEN, RESET)
  }
  fn on_byellow(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::BRIGHT_YELLOW, RESET)
  }
  fn on_bblue(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::BRIGHT_BLUE, RESET)
  }
  fn on_bmagenta(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::BRIGHT_MAGENTA, RESET)
  }
  fn on_bcyan(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::BRIGHT_CYAN, RESET)
  }
  fn on_bwhite(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", bg::BRIGHT_WHITE, RESET)
  }
  fn bold(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", BOLD, RESET)
  }
  fn italic(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", ITALIC, RESET)
  }
  fn underline(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", UNDERLINE, RESET)
  }
  fn err(&self) -> String
  where
    Self: Display,
  {
    self.bold().underline().rgb(255, 75, 75)
  }
  fn info(&self) -> String
  where
    Self: Display,
  {
    self.idcolor(222)
  }
  fn success(&self) -> String
  where
    Self: Display,
  {
    self.bold().underline().bgreen()
  }
}

impl Colorize for String {}
impl<'a> Colorize for &'a str {}

pub fn goto(x: u16, y: u16) -> String {
  format!("{ESC}[{y};{x}H")
}
