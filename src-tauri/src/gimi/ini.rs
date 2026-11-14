// Minimal INI section model that preserves the original text byte-for-byte.
#[derive(Clone)]
pub struct Section {
  pub name: String,
  pub raw: String,
}

pub struct Document {
  pub preamble: String,
  pub sections: Vec<Section>,
}

fn section_name(line: &str) -> Option<String> {
  let trimmed = line.trim_start();
  if !trimmed.starts_with('[') {
    return None;
  }
  let end = trimmed.find(']')?;
  Some(trimmed[1..end].trim().to_string())
}

pub fn parse(text: &str) -> Document {
  let mut preamble = String::new();
  let mut sections: Vec<Section> = Vec::new();
  for line in text.split_inclusive('\n') {
    match section_name(line) {
      Some(name) => sections.push(Section { name, raw: line.to_string() }),
      None => match sections.last_mut() {
        Some(section) => section.raw.push_str(line),
        None => preamble.push_str(line),
      },
    }
  }
  Document { preamble, sections }
}

impl Document {
  pub fn render(&self) -> String {
    let mut text = self.preamble.clone();
    for section in &self.sections {
      text.push_str(&section.raw);
    }
    text
  }

  pub fn section(&self, name: &str) -> Option<&Section> {
    self.sections.iter().find(|section| section.name.eq_ignore_ascii_case(name))
  }
}

pub fn split_header(raw: &str) -> (&str, &str) {
  match raw.find('\n') {
    Some(index) => (&raw[..=index], &raw[index + 1..]),
    None => (raw, ""),
  }
}

pub fn body(raw: &str) -> &str {
  split_header(raw).1
}

fn line_ending(line: &str) -> &'static str {
  if line.ends_with("\r\n") { "\r\n" } else if line.ends_with('\n') { "\n" } else { "" }
}

fn leading_whitespace(line: &str) -> &str {
  let trimmed = line.trim_start_matches([' ', '\t']);
  &line[..line.len() - trimmed.len()]
}

/// Returns the leading indentation when the line assigns the key.
fn option_prefix<'a>(line: &'a str, key: &str) -> Option<&'a str> {
  let trimmed = line.trim_start();
  if trimmed.starts_with(';') {
    return None;
  }
  let equals = trimmed.find('=')?;
  if !trimmed[..equals].trim().eq_ignore_ascii_case(key) {
    return None;
  }
  Some(leading_whitespace(line))
}

pub fn set_option(text: &str, section: &str, key: &str, value: &str) -> String {
  let mut document = parse(text);
  match document.sections.iter().position(|item| item.name.eq_ignore_ascii_case(section)) {
    Some(index) => {
      let raw = document.sections[index].raw.clone();
      let (header, body_text) = split_header(&raw);
      let mut lines = String::new();
      let mut replaced = false;
      for line in body_text.split_inclusive('\n') {
        if let Some(prefix) = option_prefix(line, key) {
          lines.push_str(prefix);
          lines.push_str(key);
          lines.push_str(" = ");
          lines.push_str(value);
          lines.push_str(line_ending(line));
          replaced = true;
        } else {
          lines.push_str(line);
        }
      }
      if !replaced {
        if !lines.ends_with('\n') {
          lines.push('\n');
        }
        lines.push_str(key);
        lines.push_str(" = ");
        lines.push_str(value);
        lines.push('\n');
      }
      document.sections[index].raw = format!("{}{}", header, lines);
    }
    None => {
      if let Some(last) = document.sections.last_mut() {
        if !last.raw.ends_with('\n') { last.raw.push('\n'); }
      }
      if !document.preamble.is_empty() && !document.preamble.ends_with('\n') {
        document.preamble.push('\n');
      }
      document.sections.push(Section {
        name: section.to_string(),
        raw: format!("[{}]\n{} = {}\n\n", section, key, value),
      });
    }
  }
  document.render()
}

pub fn get_option(text: &str, section: &str, key: &str) -> Option<String> {
  let document = parse(text);
  let item = document.section(section)?;
  for line in body(&item.raw).split_inclusive('\n') {
    if option_prefix(line, key).is_some() {
      let equals = line.find('=')?;
      let value = line[equals + 1..].split(';').next().unwrap_or("").trim();
      return Some(value.to_string());
    }
  }
  None
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn replaces_and_appends_options() {
    let text = "[Rendering]\r\n  export_shaders = 1\r\n; keep\r\n";
    let updated = set_option(text, "Rendering", "export_shaders", "0");
    assert!(updated.contains("  export_shaders = 0\r\n"));
    let added = set_option(&updated, "Rendering", "dump_usage", "0");
    assert!(added.contains("dump_usage = 0"));
    let section = set_option(&added, "Loader", "target", "C:\\game.exe");
    assert!(section.contains("[Loader]"));
    assert_eq!(get_option(&section, "Loader", "target").as_deref(), Some("C:\\game.exe"));
    let no_newline = set_option("[Existing]\nvalue=1", "Loader", "target", "game.exe");
    assert!(no_newline.contains("value=1\n[Loader]"));
  }
}
