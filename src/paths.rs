// SPDX-License-Identifier: MPL-2.0
//! Explicit filesystem completion, off the protocol loop; never shell expansion.
use crate::Result;
use std::path::{Path, PathBuf};
#[derive(Debug)]
pub enum Destination {
    File(PathBuf),
    Choices(Vec<PathBuf>),
}
pub fn resolve(root: &Path, text: &str) -> Result<Destination> {
    if text.is_empty() || text.len() > 4096 || text.chars().any(char::is_control) {
        return Err("Enter an existing database path or directory".into());
    }
    let path = Path::new(text);
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    };
    if path.is_file() {
        use std::io::Read;
        let mut file = std::fs::File::open(&path).map_err(|_| "Database file is unreadable")?;
        let size = file
            .metadata()
            .map_err(|_| "Database metadata unavailable")?
            .len();
        if size != 0 {
            let mut header = [0; 16];
            file.read_exact(&mut header)
                .map_err(|_| "Existing file is not SQLite")?;
            if &header != b"SQLite format 3\0" {
                return Err("Existing file is not SQLite".into());
            }
        }
        return path
            .canonicalize()
            .map(Destination::File)
            .map_err(|_| "Cannot resolve file".into());
    }
    let (directory, prefix) = if path.is_dir() {
        (path.clone(), String::new())
    } else {
        (
            path.parent().ok_or("Missing parent directory")?.to_owned(),
            path.file_name()
                .and_then(|s| s.to_str())
                .ok_or("Invalid filename")?
                .to_owned(),
        )
    };
    let entries = std::fs::read_dir(&directory).map_err(|_| "Directory is unavailable")?;
    let mut choices = Vec::new();
    for (seen, entry) in entries.enumerate() {
        if seen >= 4096 {
            return Err(
                "Directory has too many entries; enter a more specific existing path".into(),
            );
        }
        let entry = entry.map_err(|_| "Cannot read directory entry")?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(&prefix) || name.chars().any(char::is_control) {
            continue;
        }
        let ty = entry.file_type().map_err(|_| "Cannot read entry type")?;
        if ty.is_dir() || ty.is_file() {
            choices.push(entry.path());
        }
        if choices.len() > 62 {
            return Err("More than 62 matches; refine the path before submitting".into());
        }
    }
    choices.sort();
    if choices.is_empty() {
        return Err("No matching existing files or directories".into());
    }
    Ok(Destination::Choices(choices))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absolute_relative_prefix_and_bounds() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("ledger.sqlite"), b"SQLite format 3\0").unwrap();
        std::fs::create_dir(t.path().join("data")).unwrap();
        assert!(matches!(
            resolve(t.path(), "ledger.sqlite").unwrap(),
            Destination::File(_)
        ));
        assert!(matches!(
            resolve(t.path(), t.path().join("ledger.sqlite").to_str().unwrap()).unwrap(),
            Destination::File(_)
        ));
        assert!(matches!(resolve(t.path(),"led").unwrap(),Destination::Choices(c) if c.len()==1));
        assert!(resolve(t.path(), "missing").is_err());
        assert!(resolve(t.path(), "$(echo secret)").is_err());
        for i in 0..63 {
            std::fs::write(t.path().join(format!("many-{i}")), b"").unwrap();
        }
        assert!(resolve(t.path(), "many-").is_err());
    }
}
