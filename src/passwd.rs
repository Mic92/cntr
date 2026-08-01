//! User lookups without libc's getpwnam (and thus without linking NSS).
//!
//! Lookup order for --effective-user:
//! 1. /etc/passwd (covers local users)
//! 2. `getent passwd <name>` (covers NSS sources like sssd/LDAP or
//!    systemd-userdb, resolved in a separate process)
//! 3. a numeric `uid[:gid]` spec as an escape hatch

use anyhow::{Context, bail};
use rustix::process::{Gid, Uid};
use std::path::PathBuf;
use std::process::Command;

use crate::result::Result;

/// A user entry from /etc/passwd
#[derive(Debug, Clone)]
pub(crate) struct User {
    pub(crate) uid: Uid,
    pub(crate) gid: Gid,
    /// Home directory
    pub(crate) dir: PathBuf,
}

/// Look up a user by name or numeric `uid[:gid]` spec.
///
/// Returns Ok(None) if the user does not exist.
pub(crate) fn lookup(spec: &str) -> Result<Option<User>> {
    let contents = std::fs::read_to_string("/etc/passwd").context("failed to read /etc/passwd")?;
    if let Some(user) = parse_passwd(&contents, spec)? {
        return Ok(Some(user));
    }
    if let Some(user) = getent(spec)? {
        return Ok(Some(user));
    }
    parse_numeric(spec)
}

/// Resolve a user via the `getent` binary, which performs the NSS lookup
/// (sssd/LDAP, systemd-userdb, ...) in its own process so we don't have to
/// link against libc/NSS ourselves.
fn getent(username: &str) -> Result<Option<User>> {
    let output = match Command::new("getent").args(["passwd", username]).output() {
        Ok(output) => output,
        // getent not installed - fall through to other lookup methods
        Err(_) => return Ok(None),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let stdout = String::from_utf8(output.stdout).context("getent returned invalid UTF-8")?;
    parse_passwd(&stdout, username)
}

/// Parse a numeric `uid[:gid]` spec. The home directory is unknown in this
/// case, so `/` is used.
fn parse_numeric(spec: &str) -> Result<Option<User>> {
    let (uid, gid) = match spec.split_once(':') {
        Some((uid, gid)) => (uid, gid),
        None => (spec, spec),
    };
    let (Ok(uid), Ok(gid)) = (uid.parse::<u32>(), gid.parse::<u32>()) else {
        return Ok(None);
    };
    if uid == u32::MAX || gid == u32::MAX {
        bail!("invalid uid/gid (-1): {}", spec);
    }
    Ok(Some(User {
        uid: Uid::from_raw(uid),
        gid: Gid::from_raw(gid),
        dir: PathBuf::from("/"),
    }))
}

fn parse_passwd(contents: &str, username: &str) -> Result<Option<User>> {
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // name:passwd:uid:gid:gecos:dir:shell
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 6 || fields[0] != username {
            continue;
        }
        let uid: u32 = fields[2].parse().with_context(|| {
            format!(
                "invalid uid '{}' in /etc/passwd for {}",
                fields[2], username
            )
        })?;
        let gid: u32 = fields[3].parse().with_context(|| {
            format!(
                "invalid gid '{}' in /etc/passwd for {}",
                fields[3], username
            )
        })?;
        if uid == u32::MAX || gid == u32::MAX {
            bail!("invalid uid/gid (-1) in /etc/passwd for {}", username);
        }
        return Ok(Some(User {
            uid: Uid::from_raw(uid),
            gid: Gid::from_raw(gid),
            dir: PathBuf::from(fields[5]),
        }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_passwd() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\n\
                      joerg:x:1000:100:Joerg:/home/joerg:/bin/zsh\n";
        let user = parse_passwd(passwd, "joerg").unwrap().unwrap();
        assert_eq!(user.uid.as_raw(), 1000);
        assert_eq!(user.gid.as_raw(), 100);
        assert_eq!(user.dir, PathBuf::from("/home/joerg"));
        assert!(parse_passwd(passwd, "nobody").unwrap().is_none());
        let root = parse_passwd(passwd, "root").unwrap().unwrap();
        assert!(root.uid.is_root());
    }

    #[test]
    fn test_parse_numeric() {
        let user = parse_numeric("1000:100").unwrap().unwrap();
        assert_eq!(user.uid.as_raw(), 1000);
        assert_eq!(user.gid.as_raw(), 100);
        assert_eq!(user.dir, PathBuf::from("/"));

        let user = parse_numeric("1000").unwrap().unwrap();
        assert_eq!(user.gid.as_raw(), 1000);

        assert!(parse_numeric("joerg").unwrap().is_none());
        assert!(parse_numeric("4294967295").is_err());
    }
}
