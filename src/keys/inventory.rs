use super::{public_path, Inventory, Key};
use anyhow::{Context, Result};
use ssh_key::{HashAlg, PrivateKey, PublicKey};
use std::{collections::HashSet, fs, path::Path};
use walkdir::WalkDir;

pub fn inventory(dir: &Path) -> Result<Inventory> {
    let mut result = Inventory {
        keys: Vec::new(),
        warnings: Vec::new(),
    };
    if !dir.exists() {
        return Ok(result);
    }
    for entry in WalkDir::new(dir)
        .max_depth(1)
        .min_depth(1)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry.with_context(|| {
            format!(
                "Could not read {}. Check directory permissions.",
                dir.display()
            )
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.len() > 1_048_576 {
            continue;
        }
        let bytes = fs::read(path).with_context(|| {
            format!(
                "Could not read key file: {}. Check file permissions.",
                path.display()
            )
        })?;
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let public = if text.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----") {
            match PrivateKey::from_openssh(text) {
                Ok(key) => Some((key.public_key().clone(), true)),
                Err(e) => {
                    result.warnings.push(format!(
                        "Could not parse {}: {e}. Check the key with ssh-keygen -l -f.",
                        path.display()
                    ));
                    None
                }
            }
        } else if path.extension().is_some_and(|ext| ext == "pub") {
            match PublicKey::from_openssh(text.trim()) {
                Ok(key) => Some((key, false)),
                Err(e) => {
                    result.warnings.push(format!(
                        "Could not parse {}: {e}. Check the public key format.",
                        path.display()
                    ));
                    None
                }
            }
        } else if text.starts_with("-----BEGIN RSA PRIVATE KEY-----")
            || text.starts_with("-----BEGIN EC PRIVATE KEY-----")
            || text.starts_with("-----BEGIN PRIVATE KEY-----")
            || text.starts_with("-----BEGIN ENCRYPTED PRIVATE KEY-----")
        {
            result.warnings.push(format!(
                "Legacy PEM key found: {}. Convert it with ssh-keygen -p -o -f <filename>, then refresh the key list.",
                path.display()
            ));
            None
        } else {
            None
        };
        if let Some((public, private)) = public {
            result.keys.push(Key {
                path: path.into(),
                algorithm: public.algorithm().to_string(),
                fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
                private,
            });
        }
    }
    let private_paths: HashSet<_> = result
        .keys
        .iter()
        .filter(|k| k.private)
        .map(|k| public_path(&k.path))
        .collect();
    result
        .keys
        .retain(|k| k.private || !private_paths.contains(&k.path));
    Ok(result)
}
