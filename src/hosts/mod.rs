mod parse;
mod storage;
#[cfg(test)]
mod tests;

use anyhow::{bail, Context, Result};
use parse::{comment, directive};
use std::{
    ops::Range,
    path::{Path, PathBuf},
};

const FIELDS: [&str; 4] = ["HostName", "User", "Port", "IdentityFile"];

pub fn validate_alias(alias: &str) -> Result<()> {
    if alias.is_empty()
        || alias.starts_with('-')
        || !alias
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        bail!("Invalid alias. Select or enter a single alias using letters, numbers, dots, underscores, or hyphens, such as work-server.");
    }
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Host {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: String,
    pub identity_file: String,
}

impl Host {
    fn values(&self) -> [&str; 4] {
        [&self.hostname, &self.user, &self.port, &self.identity_file]
    }

    pub fn validate(&self) -> Result<()> {
        validate_alias(&self.alias)?;
        if self.hostname.is_empty()
            || self.hostname.starts_with('-')
            || !self
                .hostname
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ".-_:[]%".contains(c))
        {
            bail!("Invalid server address. Enter a hostname or IP address without a username or port.");
        }
        if !self.user.is_empty()
            && (self.user.starts_with('-')
                || !self
                    .user
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)))
        {
            bail!("Invalid username. Enter a login name such as alice, without spaces or @.");
        }
        if !self.port.is_empty() && self.port.parse::<u16>().ok().filter(|p| *p > 0).is_none() {
            bail!("Invalid SSH port. Enter a number from 1 to 65535, or leave it blank to use your SSH settings.");
        }
        if self
            .identity_file
            .chars()
            .any(|c| c.is_control() || "\"\\".contains(c))
        {
            bail!("Invalid key path. Use a path without double quotes, backslashes, or control characters.");
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Entry {
    pub host: Host,
    pub read_only: bool,
    range: Range<usize>,
}

pub struct Config {
    source: Option<String>,
    pub entries: Vec<Entry>,
}

impl Config {
    pub fn save(&self, dir: &Path, index: Option<usize>, host: &Host) -> Result<Option<PathBuf>> {
        host.validate()?;
        if self.entries.iter().enumerate().any(|(i, entry)| {
            Some(i) != index
                && entry
                    .host
                    .alias
                    .split_whitespace()
                    .any(|alias| alias.eq_ignore_ascii_case(&host.alias))
        }) {
            bail!("Alias {} already exists in ~/.ssh/config. Edit that entry or choose another alias.", host.alias);
        }
        let source = self.source.as_deref().unwrap_or_default();
        let eol = if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let mut block = format!("Host {}{eol}", host.alias);
        for (label, value) in FIELDS.iter().zip(host.values()) {
            if !value.is_empty() {
                let value = if *label == "IdentityFile" {
                    format!("\"{value}\"")
                } else {
                    value.to_owned()
                };
                block.push_str(&format!("    {label} {value}{eol}"));
            }
        }
        let next = if let Some(index) = index {
            let entry = self.editable(index)?;
            // Preserve extra options and comments, including comments on replaced directives.
            for line in source[entry.range.clone()].split_inclusive('\n') {
                match directive(line) {
                    Some((key, _))
                        if key == "host"
                            || FIELDS.iter().any(|field| field.eq_ignore_ascii_case(&key)) =>
                    {
                        if let Some(comment) = comment(line) {
                            block.push_str(&format!("    {comment}{eol}"));
                        }
                    }
                    _ => block.push_str(line),
                }
            }
            format!(
                "{}{}{}",
                &source[..entry.range.start],
                block,
                &source[entry.range.end..]
            )
        } else {
            // Reset scope before original global directives so they still apply to every other host.
            let needs_reset = source
                .lines()
                .filter_map(directive)
                .next()
                .is_some_and(|(key, _)| key != "host" && key != "match");
            format!(
                "{block}{eol}{}{source}",
                if needs_reset {
                    format!("Host *{eol}")
                } else {
                    String::new()
                }
            )
        };
        self.commit(dir, &next)
    }

    pub fn delete(&self, dir: &Path, index: usize) -> Result<Option<PathBuf>> {
        let entry = self.editable(index)?;
        let source = self.source.as_deref().unwrap_or_default();
        let mut kept = String::new();
        for line in source[entry.range.clone()].split_inclusive('\n') {
            if line.trim().starts_with('#') || line.trim().is_empty() {
                kept.push_str(line);
            }
        }
        self.commit(
            dir,
            &format!(
                "{}{}{}",
                &source[..entry.range.start],
                kept,
                &source[entry.range.end..]
            ),
        )
    }

    fn editable(&self, index: usize) -> Result<&Entry> {
        let entry = self
            .entries
            .get(index)
            .context("The selected host no longer exists. Reopen Hosts and select it again.")?;
        if entry.read_only {
            bail!("This entry matches multiple hosts or has repeated settings. Edit it directly in ~/.ssh/config to avoid changing other connections.");
        }
        Ok(entry)
    }
}
