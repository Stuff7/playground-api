#![allow(dead_code)]
use std::ops::Deref;

pub fn title(value: impl Deref<Target = str>) -> String {
  value
    .chars()
    .fold(String::new(), |mut a, b| {
      if b.is_uppercase() {
        a.push(' ');
      }
      a.push(b);
      a
    })
    .trim()
    .to_string()
}
