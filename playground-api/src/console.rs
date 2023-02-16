use std::fmt::Display;

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const UNDERLINE: &str = "\x1b[4m";

pub trait Colorize {
  fn rgb(&self, r: u8, g: u8, b: u8) -> String
  where
    Self: Display,
  {
    format!("\x1b[38;2;{r};{g};{b}m{self}{RESET}")
  }
  fn on_rgb(&self, r: u8, g: u8, b: u8) -> String
  where
    Self: Display,
  {
    format!("\x1b[48;2;{r};{g};{b}m{self}{RESET}")
  }
  fn bold(&self) -> String
  where
    Self: Display,
  {
    format!("{}{self}{}", BOLD, RESET)
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
  fn success(&self) -> String
  where
    Self: Display,
  {
    self.bold().underline().rgb(0, 255, 94)
  }
  fn info(&self) -> String
  where
    Self: Display,
  {
    self.bold().underline().rgb(240, 105, 255)
  }
  fn log(&self) -> String
  where
    Self: Display,
  {
    self.rgb(255, 253, 194)
  }
}

impl Colorize for String {}
impl<'a> Colorize for &'a str {}

#[macro_export]
macro_rules! log {
  ( $($fn: ident).* @ $( $x: expr ),* ) => {
    {
      println!("{}", format!($($x),*).$($fn()).*);
    }
  };
  ( $( $x: expr ),* ) => {
    {
      println!("{}", format!($($x),*).log());
    }
  };
}
