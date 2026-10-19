//! High-entropy administrator credential; only a digest is stored on disk.
use crate::store::{self, Store};
use anyhow::{Result, ensure};
use fs2::FileExt;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn random() -> String {
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    digest: String,
    generation: String,
}
pub fn directory(s: &Store) -> std::path::PathBuf {
    s.state_dir.join("admin")
}
pub fn reset(s: &Store, force: bool) -> Result<Option<String>> {
    // Revocation must remain available while a node transaction owns Store::lock.
    store::private_dir(&s.state_dir)?;
    let dir = directory(s);
    store::private_dir(&dir)?;
    let lock_path = dir.join("credential.lock");
    if lock_path.exists() {
        store::check_private(&lock_path, false)?;
    }
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(lock_path)?;
    lock.try_lock_exclusive()?;
    let path = dir.join("credential.json");
    if path.exists() {
        store::check_private(&path, false)?;
        if !force {
            return Ok(None);
        }
    }
    let token = random();
    store::atomic_write(
        &path,
        &serde_json::to_vec(&Credential {
            digest: store::digest(token.as_bytes()),
            generation: random(),
        })?,
    )?;
    Ok(Some(token))
}
pub fn generation(s: &Store) -> Result<String> {
    Ok(read(s)?.generation)
}
fn read(s: &Store) -> Result<Credential> {
    store::check_private(&s.state_dir, true)?;
    store::check_private(&directory(s), true)?;
    let p = directory(s).join("credential.json");
    store::check_private(&p, false)?;
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
pub fn verify(s: &Store, token: &str) -> bool {
    let digest = store::digest(token.as_bytes());
    read(s).is_ok_and(|c| {
        token.len() == 64 && bool::from(c.digest.as_bytes().ct_eq(digest.as_bytes()))
    })
}
pub struct Session {
    pub csrf: String,
    pub generation: String,
    pub born: u64,
    pub seen: u64,
    pub unlock: Option<(String, u64)>,
}
#[derive(Default)]
pub struct Auth {
    pub sessions: BTreeMap<String, Session>,
    attempts: Vec<u64>,
}
impl Auth {
    pub fn attempt(&mut self) -> Result<()> {
        let time = now();
        self.attempts.retain(|t| time.saturating_sub(*t) < 60);
        ensure!(self.attempts.len() < 10, "rate_limited");
        self.attempts.push(time);
        Ok(())
    }
    pub fn login(&mut self, s: &Store, token: &str) -> Result<(String, String)> {
        self.attempt()?;
        ensure!(verify(s, token), "unauthorized");
        let time = now();
        self.sessions.retain(|_, v| {
            time.saturating_sub(v.born) < 28800 && time.saturating_sub(v.seen) < 900
        });
        if self.sessions.len() >= 5 {
            let key = self
                .sessions
                .iter()
                .min_by_key(|(_, s)| s.born)
                .unwrap()
                .0
                .clone();
            self.sessions.remove(&key);
        }
        let id = random();
        let csrf = random();
        self.sessions.insert(
            store::digest(id.as_bytes()),
            Session {
                csrf: csrf.clone(),
                generation: generation(s)?,
                born: time,
                seen: time,
                unlock: None,
            },
        );
        Ok((id, csrf))
    }
    pub fn session(&mut self, s: &Store, id: &str, csrf: Option<&str>) -> Result<&mut Session> {
        let key = store::digest(id.as_bytes());
        let time = now();
        let current = generation(s)?;
        self.sessions.retain(|_, v| {
            v.generation == current
                && time.saturating_sub(v.born) < 28800
                && time.saturating_sub(v.seen) < 900
        });
        let v = self
            .sessions
            .get_mut(&key)
            .ok_or_else(|| anyhow::anyhow!("unauthorized"))?;
        if let Some(csrf) = csrf {
            ensure!(
                bool::from(v.csrf.as_bytes().ct_eq(csrf.as_bytes())),
                "csrf_denied"
            );
        }
        v.seen = time;
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reset_expiry_csrf_and_rate_limit() {
        let t = tempfile::tempdir().unwrap();
        let s = Store::new(t.path().join("config"), t.path().join("state")).unwrap();
        let token = reset(&s, false).unwrap().unwrap();
        assert!(!verify(&s, "subscription-token"));
        assert!(reset(&s, false).unwrap().is_none());
        let mut a = Auth::default();
        let (id, csrf) = a.login(&s, &token).unwrap();
        assert!(a.session(&s, &id, Some("wrong")).is_err());
        assert!(a.session(&s, &id, Some(&csrf)).is_ok());
        a.session(&s, &id, None).unwrap().seen = now() - 901;
        assert!(a.session(&s, &id, None).is_err());
        let (id, _) = a.login(&s, &token).unwrap();
        reset(&s, true).unwrap();
        assert!(a.session(&s, &id, None).is_err());
        assert!(!verify(&s, &token));
        for _ in 0..8 {
            let _ = a.login(&s, "bad");
        }
        assert!(a.attempt().is_err());
        assert!(
            !fs::read_to_string(directory(&s).join("credential.json"))
                .unwrap()
                .contains(&token)
        );
    }
    #[test]
    fn reset_remains_available_during_node_transaction() {
        let t = tempfile::tempdir().unwrap();
        let s = Store::new(t.path().join("config"), t.path().join("state")).unwrap();
        let old = reset(&s, false).unwrap().unwrap();
        let _transaction = s.lock().unwrap();
        let new = reset(&s, true).unwrap().unwrap();
        assert!(!verify(&s, &old));
        assert!(verify(&s, &new));
    }
}
