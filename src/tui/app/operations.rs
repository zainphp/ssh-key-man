use super::App;
use crate::tui::{
    model::{Operation, RunState},
    wizard::KeyWizard,
};
use crate::{agent, keys, theme::Tone};
use anyhow::Result;

impl App {
    pub fn run_operation(&mut self, operation: Operation) -> Result<RunState> {
        let dir = &self.directory;
        let result = match operation {
            Operation::Connect(alias) => {
                let mut command = crate::process::command("ssh")?;
                command.arg("--").arg(&alias);
                crate::process::interactive(
                    &mut command,
                    &format!("connect to SSH alias {alias}"),
                )?;
                return Ok(RunState::Finished);
            }
            Operation::Generate { name, rsa } => {
                println!("OpenSSH will ask for an optional passphrase to protect the private key. Input is hidden.");
                keys::generate(dir, &name, rsa).map(|key| {
                    self.message.set(
                        format!(
                            "Created {}. Fingerprint: {}",
                            key.path.display(),
                            key.fingerprint
                        ),
                        Tone::Success,
                    );
                    self.wizard = Some(KeyWizard::register(key));
                })
            }
            Operation::Load(key) => {
                let mut agent = agent::ensure(dir)?;
                let loaded = agent::load(&mut agent, &key)?;
                println!(
                    "{}: {}",
                    key.path.display(),
                    if loaded {
                        "Loaded in agent"
                    } else {
                        "Already loaded"
                    }
                );
                return Ok(RunState::Finished);
            }
            Operation::Delete(key) => {
                self.wizard = None;
                keys::delete(dir, &key).map(|()| {
                    self.message.set(
                        format!("Deleted local key files for {}.", key.path.display()),
                        Tone::Success,
                    )
                })
            }
            Operation::Finish { key, remote } => {
                let registration = remote
                    .as_ref()
                    .map(|(remote, port)| keys::deploy(&key, remote, *port))
                    .transpose();
                println!("Step 3 of 3: loading the key into the SSH agent.");
                let loaded = agent::ensure(dir).and_then(|mut agent| agent::load(&mut agent, &key));
                let failed = registration.is_err() || loaded.is_err();
                let mut messages = vec![format!("Key saved: {}", key.path.display())];
                match registration {
                    Ok(Some(())) => messages.push("Public key added to the server.".into()),
                    Ok(None) => {}
                    Err(e) => messages.push(format!("Remote setup failed: {e:#}")),
                }
                match loaded {
                    Ok(_) => messages.push("Loaded in agent.".into()),
                    Err(e) => messages.push(format!(
                        "Agent loading failed: {e:#}. Retry with {}.",
                        keys::load_retry(&key)
                    )),
                }
                self.message.set(
                    messages.join("\n"),
                    if failed { Tone::Danger } else { Tone::Success },
                );
                self.wizard = None;
                Ok(())
            }
        };
        if let Err(e) = result {
            self.message.set(format!("{e:#}"), Tone::Danger);
        }
        self.refresh_keys();
        Ok(RunState::Continue)
    }
}
