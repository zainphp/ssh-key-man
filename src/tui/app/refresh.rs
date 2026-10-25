use super::*;
use std::sync::mpsc::{self, Receiver, TryRecvError};

pub(super) struct Snapshot {
    pub(super) inventory: keys::Inventory,
    pub(super) agent: Option<Agent>,
}

fn read_keys(directory: &Path) -> Result<Snapshot> {
    let mut inventory = keys::inventory(directory)?;
    let agent = match agent::current(directory) {
        Ok(agent) => agent,
        Err(error) => {
            inventory.warnings.push(format!("{error:#}"));
            None
        }
    };
    Ok(Snapshot { inventory, agent })
}

impl App {
    pub(super) fn refresh_keys(&mut self) {
        if self.refresh.is_some() {
            return;
        }
        let directory = self.directory.clone();
        let (sender, receiver) = mpsc::channel();
        self.refresh = Some(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(read_keys(&directory));
        });
    }

    pub fn poll_refresh(&mut self) {
        let result = match self.refresh.as_ref().map(Receiver::try_recv) {
            Some(Ok(result)) => result,
            Some(Err(TryRecvError::Disconnected)) => Err(anyhow::anyhow!(
                "Could not finish checking keys. Reopen Keys to try again."
            )),
            _ => return,
        };
        self.refresh = None;
        match result {
            Ok(snapshot) => {
                self.keys = snapshot.inventory.keys;
                self.agent = snapshot.agent;
                if !snapshot.inventory.warnings.is_empty() {
                    let warning = snapshot.inventory.warnings.join("\n");
                    let text = if self.message.is_empty() {
                        warning
                    } else {
                        format!("{}\n{warning}", self.message.text)
                    };
                    let tone = if self.message.tone == Tone::Danger {
                        Tone::Danger
                    } else {
                        Tone::Warning
                    };
                    self.message.set(text, tone);
                }
                clamp_selection(&mut self.key_selection, self.keys.len());
            }
            Err(error) => {
                self.keys.clear();
                self.agent = None;
                self.message.set(format!("{error:#}"), Tone::Danger);
            }
        }
    }
}
