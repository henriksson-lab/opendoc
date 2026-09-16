//! Who the caller is — decided by the server, from state the server holds.
//!
//! Two things live here and they are deliberately separate:
//!
//! * a **subject directory**, mapping a subject name to the digest of its
//!   long-lived API key and to the [`ActorId`] the server will stamp on every
//!   operation that subject submits;
//! * a **session table**, mapping short-lived bearer tokens to subjects.
//!
//! The actor id is the load-bearing part. `opendoc-merge` decides operation
//! identity and last-writer-wins from `OperationId { actor, seq }`, so an
//! actor id a client could choose is an actor id a client could impersonate.
//! Here it is a property of the directory entry, and
//! [`AuthenticatedSubject::actor`] is the only place the rest of the service
//! reads it from.
//!
//! What this is not: a password store. API keys are high-entropy secrets an
//! operator provisions; there is no rotation, no lockout, no federation. See
//! "What this does not do" in `docs/adr/0015`.

use crate::clock::Clock;
use crate::error::{ServiceError, ServiceResult};
use opendoc_merge::ActorId;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// How long a session token stays valid after it is issued.
pub const DEFAULT_SESSION_TTL_MS: u64 = 12 * 60 * 60 * 1000;

/// A 32-byte digest of a secret. Never the secret.
pub type SecretDigest = [u8; 32];

pub fn digest_secret(secret: &str) -> SecretDigest {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.finalize().into()
}

/// Compares two digests without an early return.
///
/// The digests are of high-entropy secrets, so a timing leak here is weak —
/// but "weak" is not a reason to write the branchy version.
fn constant_time_eq(left: &SecretDigest, right: &SecretDigest) -> bool {
    let mut difference = 0u8;
    for index in 0..left.len() {
        difference |= left[index] ^ right[index];
    }
    difference == 0
}

/// 32 bytes from the operating system CSPRNG, base64url without padding.
pub fn random_token() -> ServiceResult<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| ServiceError::Internal(format!("csprng unavailable: {error}")))?;
    Ok(base64_url(&bytes))
}

fn base64_url(bytes: &[u8]) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    URL_SAFE_NO_PAD.encode(bytes)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubjectRecord {
    pub subject: String,
    /// The merge actor id the server stamps on this subject's operations.
    pub actor: ActorId,
    pub key_digest: SecretDigest,
}

/// The authenticated caller, as the server understands it.
///
/// Constructed only by [`IdentityService`]; every field is server state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedSubject {
    pub subject: String,
    pub actor: ActorId,
}

#[derive(Clone, Debug)]
struct SessionRecord {
    subject: String,
    actor: ActorId,
    expires_at_ms: u64,
}

pub struct IdentityService {
    subjects: Mutex<BTreeMap<String, SubjectRecord>>,
    sessions: Mutex<BTreeMap<SecretDigest, SessionRecord>>,
    clock: Clock,
    session_ttl_ms: u64,
}

impl IdentityService {
    pub fn new(clock: Clock) -> Self {
        Self {
            subjects: Mutex::new(BTreeMap::new()),
            sessions: Mutex::new(BTreeMap::new()),
            clock,
            session_ttl_ms: DEFAULT_SESSION_TTL_MS,
        }
    }

    pub fn with_session_ttl_ms(mut self, session_ttl_ms: u64) -> Self {
        self.session_ttl_ms = session_ttl_ms;
        self
    }

    pub fn session_ttl_ms(&self) -> u64 {
        self.session_ttl_ms
    }

    /// Registers a subject, its API key and the actor id bound to it.
    ///
    /// The actor id must be unique: two subjects sharing one actor id would
    /// make `OperationId` ambiguous, and the merge would read their edits as
    /// one actor's sequential stream rather than as concurrent work.
    pub fn register_subject(
        &self,
        subject: &str,
        actor: &str,
        api_key: &str,
    ) -> ServiceResult<SubjectRecord> {
        let subject_name = subject.trim();
        let actor_name = actor.trim();
        if subject_name.is_empty() {
            return Err(ServiceError::BadRequest("subject is empty".to_string()));
        }
        if actor_name.is_empty() {
            return Err(ServiceError::BadRequest("actor is empty".to_string()));
        }
        if api_key.len() < 16 {
            return Err(ServiceError::BadRequest(
                "api key must be at least 16 characters".to_string(),
            ));
        }
        let record = SubjectRecord {
            subject: subject_name.to_string(),
            actor: ActorId(actor_name.to_string()),
            key_digest: digest_secret(api_key),
        };
        let mut subjects = self.lock_subjects()?;
        if let Some(conflict) = subjects
            .values()
            .find(|existing| existing.actor == record.actor && existing.subject != record.subject)
        {
            return Err(ServiceError::Conflict(format!(
                "actor {} is already bound to subject {}",
                record.actor.0, conflict.subject
            )));
        }
        subjects.insert(record.subject.clone(), record.clone());
        Ok(record)
    }

    pub fn subject(&self, subject: &str) -> ServiceResult<Option<SubjectRecord>> {
        Ok(self.lock_subjects()?.get(subject.trim()).cloned())
    }

    /// Exchanges an API key for a session token.
    ///
    /// An unknown subject and a wrong key produce the identical error: the
    /// directory is not an oracle for which names exist.
    pub fn open_session(&self, subject: &str, api_key: &str) -> ServiceResult<IssuedSession> {
        let presented = digest_secret(api_key);
        let record = self.lock_subjects()?.get(subject.trim()).cloned();
        let record = match record {
            Some(record) if constant_time_eq(&record.key_digest, &presented) => record,
            _ => {
                return Err(ServiceError::Unauthenticated(
                    "subject or api key is not valid".to_string(),
                ))
            }
        };
        let token = random_token()?;
        let now = self.clock.now_ms();
        let expires_at_ms = now.saturating_add(self.session_ttl_ms);
        // Sweep here rather than on a timer. `authenticate` drops an expired
        // session as it refuses it, which only reaches the tokens somebody
        // still presents; a session abandoned at the end of a browsing day is
        // never presented again and used to sit in this map for the life of
        // the process. Signing in is the one moment that is both cheap to do
        // this in and guaranteed to happen while the table is growing.
        self.sweep_expired_sessions(now)?;
        self.lock_sessions()?.insert(
            digest_secret(&token),
            SessionRecord {
                subject: record.subject.clone(),
                actor: record.actor.clone(),
                expires_at_ms,
            },
        );
        Ok(IssuedSession {
            token,
            subject: record.subject,
            actor: record.actor,
            expires_at_ms,
        })
    }

    /// Resolves a bearer token to the caller the server believes it is.
    ///
    /// An expired session is removed as it is rejected, so a clock that has
    /// moved past `expires_at_ms` is enough to end a session — there is no
    /// separate sweep to forget.
    pub fn authenticate(&self, token: &str) -> ServiceResult<AuthenticatedSubject> {
        let key = digest_secret(token);
        let now = self.clock.now_ms();
        let mut sessions = self.lock_sessions()?;
        match sessions.get(&key).cloned() {
            Some(session) if session.expires_at_ms > now => Ok(AuthenticatedSubject {
                subject: session.subject,
                actor: session.actor,
            }),
            Some(_) => {
                sessions.remove(&key);
                Err(ServiceError::Unauthenticated(
                    "session has expired".to_string(),
                ))
            }
            None => Err(ServiceError::Unauthenticated(
                "session token is not valid".to_string(),
            )),
        }
    }

    /// Ends one session. Idempotent: signing out twice is not an error.
    pub fn close_session(&self, token: &str) -> ServiceResult<bool> {
        Ok(self
            .lock_sessions()?
            .remove(&digest_secret(token))
            .is_some())
    }

    /// Ends every session a subject holds. This is what revoking a person's
    /// access has to call — dropping the directory entry alone would leave
    /// live tokens working until they expired.
    pub fn close_sessions_for_subject(&self, subject: &str) -> ServiceResult<usize> {
        let subject = subject.trim();
        let mut sessions = self.lock_sessions()?;
        let doomed: Vec<SecretDigest> = sessions
            .iter()
            .filter(|(_, session)| session.subject == subject)
            .map(|(key, _)| *key)
            .collect();
        for key in &doomed {
            sessions.remove(key);
        }
        Ok(doomed.len())
    }

    /// Forgets every session that has already expired.
    ///
    /// Returns how many were dropped, so a test can prove the sweep ran rather
    /// than assert on a count that a lazy removal would also produce.
    pub fn sweep_expired_sessions(&self, now_ms: u64) -> ServiceResult<usize> {
        let mut sessions = self.lock_sessions()?;
        let doomed: Vec<SecretDigest> = sessions
            .iter()
            .filter(|(_, session)| session.expires_at_ms <= now_ms)
            .map(|(key, _)| *key)
            .collect();
        for key in &doomed {
            sessions.remove(key);
        }
        Ok(doomed.len())
    }

    pub fn open_session_count(&self) -> ServiceResult<usize> {
        Ok(self.lock_sessions()?.len())
    }

    fn lock_subjects(
        &self,
    ) -> ServiceResult<std::sync::MutexGuard<'_, BTreeMap<String, SubjectRecord>>> {
        self.subjects
            .lock()
            .map_err(|_| ServiceError::Internal("subject directory lock poisoned".to_string()))
    }

    fn lock_sessions(
        &self,
    ) -> ServiceResult<std::sync::MutexGuard<'_, BTreeMap<SecretDigest, SessionRecord>>> {
        self.sessions
            .lock()
            .map_err(|_| ServiceError::Internal("session table lock poisoned".to_string()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssuedSession {
    pub token: String,
    pub subject: String,
    pub actor: ActorId,
    pub expires_at_ms: u64,
}
