use super::{Config, Entry, Host, FIELDS};

// Keep the source untouched; only tokenize directives needed to locate and edit host blocks.
pub(super) fn directive(line: &str) -> Option<(String, Vec<String>)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let end = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());
    let key = line[..end].to_ascii_lowercase();
    let rest = line[end..]
        .trim_start()
        .strip_prefix('=')
        .unwrap_or(line[end..].trim_start())
        .trim_start();
    let mut quoted = false;
    let mut value = String::new();
    let mut values = Vec::new();
    for c in rest.chars() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted => break,
            c if c.is_whitespace() && !quoted => {
                if !value.is_empty() {
                    values.push(std::mem::take(&mut value));
                }
            }
            c => value.push(c),
        }
    }
    if quoted {
        return Some((key, Vec::new()));
    }
    if !value.is_empty() {
        values.push(value);
    }
    Some((key, values))
}

impl Config {
    pub(super) fn parse(source: Option<String>) -> Self {
        let text = source.as_deref().unwrap_or_default();
        let mut boundaries = Vec::new();
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            if let Some((key, values)) = directive(line) {
                if key == "host" || key == "match" {
                    boundaries.push((offset, key, values));
                }
            }
            offset += line.len();
        }
        let mut entries = Vec::new();
        for (i, (start, key, aliases)) in boundaries.iter().enumerate() {
            if key != "host" {
                continue;
            }
            let end = boundaries.get(i + 1).map(|b| b.0).unwrap_or(text.len());
            let mut values = [String::new(), String::new(), String::new(), String::new()];
            let mut seen = [false; 4];
            let mut read_only =
                aliases.len() != 1 || aliases.iter().any(|a| a.chars().any(|c| "*?!".contains(c)));
            for line in text[*start..end].lines().skip(1) {
                if let Some((key, args)) = directive(line) {
                    if let Some(field) = FIELDS
                        .iter()
                        .position(|field| field.eq_ignore_ascii_case(&key))
                    {
                        if seen[field] || args.len() != 1 {
                            read_only = true;
                        }
                        if !seen[field] {
                            values[field] = args.first().cloned().unwrap_or_default();
                        }
                        seen[field] = true;
                    }
                }
            }
            let [hostname, user, port, identity_file] = values;
            entries.push(Entry {
                host: Host {
                    alias: aliases.join(" "),
                    hostname,
                    user,
                    port,
                    identity_file,
                },
                read_only,
                range: *start..end,
            });
        }
        Self { source, entries }
    }
}

pub(super) fn comment(line: &str) -> Option<&str> {
    let mut quoted = false;
    for (index, c) in line.char_indices() {
        if c == '"' {
            quoted = !quoted;
        }
        if c == '#' && !quoted {
            return Some(line[index..].trim_end());
        }
    }
    None
}
