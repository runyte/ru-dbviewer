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
    if text.starts_with("http://") || text.starts_with("https://") {
        return Err("SQLite needs a local file; use PostgreSQL for a database server".into());
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
        let path = path.canonicalize().map_err(|_| "Cannot resolve file")?;
        if !path
            .to_str()
            .is_some_and(|text| text.len() <= 4096 && !text.chars().any(char::is_control))
        {
            return Err(
                "Resolved database path must be UTF-8, at most 4096 bytes and contain no controls"
                    .into(),
            );
        }
        return Ok(Destination::File(path));
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
        let ty = if ty.is_symlink() {
            match std::fs::metadata(entry.path()) {
                Ok(metadata) => metadata.file_type(),
                Err(_) => continue,
            }
        } else {
            ty
        };
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
    #[cfg(unix)]
    #[test]
    fn resolved_files_must_be_valid_saved_profile_paths() {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt, os::unix::fs::symlink};

        let t = tempfile::tempdir().unwrap();
        let cases: &[(&[u8], &str)] = &[
            (b"line\nbreak.sqlite", "newline-alias.sqlite"),
            // APFS rejects invalid UTF-8 filenames at creation, before resolution.
            #[cfg(target_os = "linux")]
            (b"invalid-\xff.sqlite", "utf8-alias.sqlite"),
        ];
        for &(target, alias) in cases {
            let target = OsStr::from_bytes(target);
            std::fs::write(t.path().join(target), b"SQLite format 3\0").unwrap();
            symlink(target, t.path().join(alias)).unwrap();
            assert_eq!(
                resolve(t.path(), alias).unwrap_err(),
                "Resolved database path must be UTF-8, at most 4096 bytes and contain no controls"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn completion_follows_usable_symlinks_and_skips_unusable_targets() {
        use std::{ffi::CString, os::unix::fs::symlink};

        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        std::fs::write(root.join("target.sqlite"), b"SQLite format 3\0").unwrap();
        std::fs::create_dir(root.join("target-directory")).unwrap();
        std::fs::write(root.join("target-directory/child.sqlite"), b"").unwrap();
        let fifo = CString::new(root.join("fifo").as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: fifo is a live, NUL-terminated path inside the temporary fixture.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        for (target, alias) in [
            ("target.sqlite", "alias-file.sqlite"),
            ("target-directory", "alias-directory"),
            ("missing", "alias-broken"),
            ("alias-loop", "alias-loop"),
            ("fifo", "alias-fifo"),
        ] {
            symlink(target, root.join(alias)).unwrap();
        }

        let Destination::Choices(choices) = resolve(root, "alias-").unwrap() else {
            panic!("Prefix must produce choices");
        };
        assert_eq!(
            choices,
            vec![root.join("alias-directory"), root.join("alias-file.sqlite")]
        );
        let Destination::File(path) = resolve(root, "alias-file.sqlite").unwrap() else {
            panic!("File symlink must resolve to a database");
        };
        assert_eq!(path, root.join("target.sqlite").canonicalize().unwrap());
        let Destination::Choices(choices) = resolve(root, "alias-directory").unwrap() else {
            panic!("Directory symlink must be traversable");
        };
        assert_eq!(choices, vec![root.join("alias-directory/child.sqlite")]);
    }

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
        for url in [
            "http://example.invalid/data.sqlite",
            "https://example.invalid/data.sqlite",
        ] {
            assert!(resolve(t.path(), url).unwrap_err().contains("local file"));
        }
        assert!(resolve(t.path(), "$(echo secret)").is_err());
        for i in 0..63 {
            std::fs::write(t.path().join(format!("many-{i}")), b"").unwrap();
        }
        assert!(resolve(t.path(), "many-").is_err());
    }
}
