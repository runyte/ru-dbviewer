// SPDX-License-Identifier: MPL-2.0
use crate::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "backend", rename_all = "lowercase", deny_unknown_fields)]
pub enum Profile {
    Sqlite {
        name: String,
        path: String,
    },
    Postgres {
        name: String,
        host: String,
        port: u16,
        database: String,
        user: String,
        #[serde(default)]
        plaintext: bool,
        #[serde(default)]
        password_env: String,
        #[serde(default)]
        ca: String,
        #[serde(default)]
        certificate: String,
        #[serde(default)]
        key: String,
    },
}
impl Profile {
    pub fn name(&self) -> &str {
        match self {
            Self::Sqlite { name, .. } | Self::Postgres { name, .. } => name,
        }
    }
    pub fn postgres(&self) -> bool {
        matches!(self, Self::Postgres { .. })
    }
    pub fn validate(&self) -> Result<()> {
        let valid =
            |s: &str, max| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control);
        if !valid(self.name(), 64) {
            return Err("Profile name must contain 1–64 bytes without controls".into());
        }
        match self {
            Self::Sqlite { path, .. } if !valid(path, 4096) => {
                return Err("A valid SQLite path is required".into());
            }
            Self::Postgres {
                host,
                port,
                database,
                user,
                password_env,
                certificate,
                key,
                ..
            } => {
                if !valid(host, 255)
                    || host.contains('@')
                    || host.contains("://")
                    || *port == 0
                    || !valid(database, 255)
                    || !valid(user, 255)
                {
                    return Err("Invalid PostgreSQL connection fields".into());
                }
                if !password_env.is_empty()
                    && (!password_env
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'_')
                        || password_env.len() > 128)
                {
                    return Err("Invalid password environment variable name".into());
                }
                if certificate.is_empty() != key.is_empty() {
                    return Err("Client certificate and key must be supplied together".into());
                }
            }
            _ => {}
        }
        Ok(())
    }
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Saved {
    pub profiles: Vec<Profile>,
    pub uncertain: Vec<String>,
}
impl Saved {
    pub fn validate(&self) -> Result<()> {
        if self.profiles.len() > 32 || self.uncertain.len() > 32 {
            return Err("Too many saved profiles".into());
        }
        let mut names = std::collections::HashSet::new();
        for p in &self.profiles {
            p.validate()?;
            if !names.insert(p.name()) {
                return Err("Duplicate profile name".into());
            }
        }
        Ok(())
    }
}
