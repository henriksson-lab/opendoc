//! Runtime, session and permission DTOs.
//!
//! # Who decides what the caller may do
//!
//! There are exactly two answers, and [`OpenDocPermissionAuthority`] names
//! them. A local runtime does not enforce document permissions at all — it is
//! the user's own machine editing the user's own files, and ADR 0004 calls
//! permissions there *advisory*. A service-backed runtime does not enforce
//! them either: `opendoc-service` does, by re-reading the grant from durable
//! storage on every submit, and the client is *told* the answer.
//!
//! Nothing in this module lets a caller supply its own grants. The earlier
//! shape did — `authorize_runtime_command` took `grants: Vec<_>` as an
//! argument, which is not an authorization check but the client telling the
//! app what it is allowed to do — and ADR 0015 names removing it as the first
//! of four changes. The service's answers reach here as an
//! [`OpenDocServiceSession`], which only a collaboration transport can build,
//! from the welcome and presence frames the socket delivered.

use crate::runtime_command_required_action;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenDocRuntimeMode {
    TauriLocal,
    BrowserLocal,
    HpcSingleUser,
    MultiUserService,
}

impl OpenDocRuntimeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TauriLocal => "tauri-local",
            Self::BrowserLocal => "browser-local",
            Self::HpcSingleUser => "hpc-single-user",
            Self::MultiUserService => "multi-user-service",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::TauriLocal => "Tauri local",
            Self::BrowserLocal => "Browser local",
            Self::HpcSingleUser => "HPC single-user",
            Self::MultiUserService => "Multi-user service",
        }
    }

    /// Who decides permissions in this mode.
    pub fn permission_authority(self) -> OpenDocPermissionAuthority {
        match self {
            Self::MultiUserService => OpenDocPermissionAuthority::Service,
            _ => OpenDocPermissionAuthority::LocalAdvisory,
        }
    }
}

/// Who decides whether a subject may run a command.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenDocPermissionAuthority {
    /// Nobody, and deliberately so. A local runtime is one user on their own
    /// machine; ADR 0004 says permissions there are advisory, and advisory
    /// means not enforced. An authorization decision in this mode is about
    /// what the *runtime* can do — which storage backends exist, whether a
    /// signing key is reachable — and never about who the caller is.
    LocalAdvisory,
    /// The collaboration service. It re-reads the grant from durable storage
    /// on every submit and sends the resulting role down the socket; this
    /// process stores that answer and can neither compute nor widen one.
    Service,
}

impl OpenDocPermissionAuthority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalAdvisory => "local-advisory",
            Self::Service => "service",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenDocStorageBackend {
    Local,
    Flat,
    /// Spelled out rather than left to `rename_all`, which would kebab this
    /// as `open-dal-fs`. Every other spelling of this backend in the
    /// workspace — `as_str`, `arg_runtime_storage_backends`, the repository
    /// target parser, the audit view — is `opendal-fs`, so the derive was the
    /// one place a client could be handed a string nothing else accepts.
    #[serde(rename = "opendal-fs")]
    OpenDalFs,
}

impl OpenDocStorageBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Flat => "flat",
            Self::OpenDalFs => "opendal-fs",
        }
    }
}

/// A role as the collaboration service defines it: a totally ordered bundle of
/// actions, Viewer < Commenter < Editor < Owner.
///
/// Restated here rather than imported because `opendoc-api` is in the WASM
/// dependency graph and `opendoc-service` deliberately is not (ADR 0015). A
/// restated definition is only as good as the test that compares it, so
/// `opendoc-service` asserts the two agree on every variant's wire string and
/// on which actions each allows.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenDocServiceRole {
    Viewer,
    Commenter,
    Editor,
    Owner,
}

impl OpenDocServiceRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Commenter => "commenter",
            Self::Editor => "editor",
            Self::Owner => "owner",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "viewer" => Some(Self::Viewer),
            "commenter" => Some(Self::Commenter),
            "editor" => Some(Self::Editor),
            "owner" => Some(Self::Owner),
            _ => None,
        }
    }

    /// The lowest role that may perform `action`, or `None` for an action name
    /// the service does not define.
    pub fn minimum_for_action(action: &str) -> Option<Self> {
        match action {
            "read" | "present" => Some(Self::Viewer),
            "comment" => Some(Self::Commenter),
            "write" => Some(Self::Editor),
            "share" => Some(Self::Owner),
            _ => None,
        }
    }

    pub fn allows_action(self, action: &str) -> bool {
        Self::minimum_for_action(action).is_some_and(|minimum| self >= minimum)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocRuntimeProfile {
    pub mode: OpenDocRuntimeMode,
    pub label: String,
    /// Who decides permissions here. Replaces the older `permissions_enabled`
    /// flag, which read as "this runtime evaluates permissions" — the thing no
    /// runtime may do.
    pub permission_authority: OpenDocPermissionAuthority,
    pub signing_enabled: bool,
    pub browser_signing_deferred: bool,
    pub default_repository_root: String,
    pub default_flat_namespace: String,
    pub storage_backends: Vec<OpenDocStorageBackend>,
}

impl OpenDocRuntimeProfile {
    pub fn for_mode(mode: OpenDocRuntimeMode) -> Self {
        Self {
            mode,
            label: mode.label().to_string(),
            permission_authority: mode.permission_authority(),
            signing_enabled: matches!(
                mode,
                OpenDocRuntimeMode::TauriLocal | OpenDocRuntimeMode::HpcSingleUser
            ),
            browser_signing_deferred: matches!(mode, OpenDocRuntimeMode::BrowserLocal),
            default_repository_root: match mode {
                OpenDocRuntimeMode::HpcSingleUser => "./opendoc-hpc-repo",
                OpenDocRuntimeMode::MultiUserService => "./opendoc-service-cache",
                _ => "./opendoc-repo",
            }
            .to_string(),
            default_flat_namespace: match mode {
                OpenDocRuntimeMode::HpcSingleUser => "hpc/opendoc",
                OpenDocRuntimeMode::MultiUserService => "service/opendoc",
                _ => "bucket/prefix",
            }
            .to_string(),
            storage_backends: match mode {
                OpenDocRuntimeMode::BrowserLocal => vec![OpenDocStorageBackend::Flat],
                _ => vec![
                    OpenDocStorageBackend::Local,
                    OpenDocStorageBackend::Flat,
                    OpenDocStorageBackend::OpenDalFs,
                ],
            },
        }
    }

    pub fn for_mode_with_capabilities(
        mode: OpenDocRuntimeMode,
        storage_backends: Option<Vec<OpenDocStorageBackend>>,
        signing_enabled: Option<bool>,
    ) -> Self {
        let mut profile = Self::for_mode(mode);
        if let Some(storage_backends) = storage_backends {
            let mut deduped = Vec::new();
            for backend in storage_backends {
                if !deduped.contains(&backend) {
                    deduped.push(backend);
                }
            }
            profile.storage_backends = deduped;
        }
        if let Some(signing_enabled) = signing_enabled {
            profile.signing_enabled = signing_enabled;
        }
        profile
    }

    pub fn supports_storage(&self, backend: OpenDocStorageBackend) -> bool {
        self.storage_backends.contains(&backend)
    }

    pub fn defers_permissions_to_a_service(&self) -> bool {
        matches!(
            self.permission_authority,
            OpenDocPermissionAuthority::Service
        )
    }

    pub fn warning_messages(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if !self.supports_storage(OpenDocStorageBackend::Local) {
            warnings.push("Local object repositories are unavailable in this runtime.".to_string());
        }
        if !self.supports_storage(OpenDocStorageBackend::Flat) {
            warnings.push("Flat object namespaces are unavailable in this runtime.".to_string());
        }
        if !self.supports_storage(OpenDocStorageBackend::OpenDalFs) {
            warnings.push(
                "OpenDAL filesystem repositories are unavailable in this runtime.".to_string(),
            );
        }
        if !self.signing_enabled {
            warnings.push(
                "Private-key signing is unavailable in this runtime; signed documents can still be opened and verified where supported."
                    .to_string(),
            );
        }
        warnings
    }
}

/// The collaboration service's answers about this client's session, exactly as
/// they arrived.
///
/// Every field here is server state. `subject`, `actor`, `document_uuid` and
/// `role` come from the service's welcome frame; `peers` and
/// `acknowledged_seq` are refreshed by its presence and acknowledgement
/// frames. A command argument can never produce one of these: the only way in
/// is `OpenDocApp::join_collaboration_session`, which a transport calls with
/// what the socket delivered.
///
/// Holding the answer is not the same as being able to compute it. If the
/// service revokes a grant, the next submit is refused on the wire whatever
/// this says — this is what the client renders and pre-checks against, not
/// what the service trusts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocServiceSession {
    pub subject: String,
    pub actor: String,
    pub document_uuid: String,
    pub role: OpenDocServiceRole,
    pub peers: Vec<OpenDocPresencePeer>,
    /// The highest sequence number of this actor's operations the service has
    /// acknowledged as durable. Zero before the first acknowledgement.
    pub acknowledged_seq: u64,
}

impl OpenDocServiceSession {
    pub fn new(
        subject: impl Into<String>,
        actor: impl Into<String>,
        document_uuid: impl Into<String>,
        role: OpenDocServiceRole,
    ) -> Self {
        Self {
            subject: subject.into(),
            actor: actor.into(),
            document_uuid: document_uuid.into(),
            role,
            peers: Vec::new(),
            acknowledged_seq: 0,
        }
    }

    pub fn with_peers(mut self, peers: Vec<OpenDocPresencePeer>) -> Self {
        self.set_peers(peers);
        self
    }

    pub fn set_peers(&mut self, peers: Vec<OpenDocPresencePeer>) {
        let mut peers = peers
            .into_iter()
            .map(OpenDocPresencePeer::normalized)
            .filter(OpenDocPresencePeer::is_valid)
            .collect::<Vec<_>>();
        peers.sort_by(|left, right| {
            (left.subject.as_str(), left.actor.as_str())
                .cmp(&(right.subject.as_str(), right.actor.as_str()))
        });
        self.peers = peers;
    }

    pub fn normalized(mut self) -> Self {
        self.subject = self.subject.trim().to_string();
        self.actor = self.actor.trim().to_string();
        self.document_uuid = self.document_uuid.trim().to_string();
        let peers = std::mem::take(&mut self.peers);
        self.set_peers(peers);
        self
    }

    pub fn is_valid(&self) -> bool {
        !self.subject.is_empty() && !self.actor.is_empty() && !self.document_uuid.is_empty()
    }
}

/// What a runtime knows about itself, and what a service has told it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocRuntimeSession {
    pub profile: OpenDocRuntimeProfile,
    /// `None` in every local runtime, and in a service runtime that has not
    /// connected yet. There is deliberately no locally synthesised stand-in:
    /// a fabricated session would be the client asserting its own identity.
    pub service_session: Option<OpenDocServiceSession>,
    pub warnings: Vec<String>,
}

impl OpenDocRuntimeSession {
    pub fn for_mode(mode: OpenDocRuntimeMode) -> Self {
        Self::for_profile(OpenDocRuntimeProfile::for_mode(mode), None)
    }

    pub fn for_profile(
        profile: OpenDocRuntimeProfile,
        service_session: Option<OpenDocServiceSession>,
    ) -> Self {
        let mut warnings = profile.warning_messages();
        let service_session = service_session
            .map(OpenDocServiceSession::normalized)
            .filter(OpenDocServiceSession::is_valid);
        if profile.defers_permissions_to_a_service() {
            if service_session.is_none() {
                warnings.push(
                    "Multi-user service mode has no answer from a collaboration service yet; no document command is authorized until one connects."
                        .to_string(),
                );
            }
        } else if service_session.is_some() {
            warnings.push(
                "A collaboration session is open but this runtime does not defer permissions to a service."
                    .to_string(),
            );
        }
        Self {
            profile,
            service_session,
            warnings,
        }
    }
}

/// One peer, as the service sees it.
///
/// `subject`, `actor` and `role` are server state; `display_name` and
/// `cursor_anchor` and `selection_anchor` are the only fields a peer contributes, and only for
/// itself. `actor` and `connections` mirror `opendoc_service::PeerView`:
/// without `actor` a client cannot map a cursor to the operations that
/// produced it, and without `connections` one person in two tabs looks like
/// two people.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocPresencePeer {
    pub subject: String,
    /// The actor id the service bound to this subject — the `id.actor` of
    /// every operation this peer authors.
    pub actor: String,
    pub display_name: String,
    pub role: OpenDocServiceRole,
    pub cursor_anchor: Option<String>,
    /// The fixed endpoint of a remote textual selection. Paired with
    /// `cursor_anchor`, which is the focus/caret endpoint.
    pub selection_anchor: Option<String>,
    pub last_seen_ms: u64,
    /// How many live connections this subject holds. One person in two tabs is
    /// one peer, not two.
    pub connections: u32,
}

impl OpenDocPresencePeer {
    pub fn normalized(mut self) -> Self {
        self.subject = self.subject.trim().to_string();
        self.actor = self.actor.trim().to_string();
        self.display_name = self.display_name.trim().to_string();
        self.cursor_anchor = self
            .cursor_anchor
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.selection_anchor = self
            .selection_anchor
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if self.display_name.is_empty() {
            self.display_name = self.subject.clone();
        }
        self
    }

    pub fn is_valid(&self) -> bool {
        !self.subject.is_empty() && !self.actor.is_empty()
    }
}

/// Where an authorization decision came from.
///
/// This is the field that makes the DTO honest: a caller can see whether it is
/// looking at a runtime capability check, at a service's answer, or at the
/// absence of one. There is no fourth variant meaning "the client worked it
/// out", because there is no such thing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenDocAuthorizationSource {
    /// Decided from this runtime's own capabilities: whether the command needs
    /// a service, a storage backend or a signing key this runtime has. No
    /// subject and no permission are involved, because a `LocalAdvisory`
    /// runtime has neither.
    RuntimeCapability,
    /// Decided from the role the service attested for this session. The client
    /// received this; it did not compute it.
    ServiceAnswer,
    /// This runtime defers permissions to a service and has no answer from
    /// one. Refused — there is deliberately no local fallback, because a
    /// fallback is the client deciding for itself.
    ServiceAnswerMissing,
}

impl OpenDocAuthorizationSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeCapability => "runtime-capability",
            Self::ServiceAnswer => "service-answer",
            Self::ServiceAnswerMissing => "service-answer-missing",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocAuthorizationDecision {
    pub mode: OpenDocRuntimeMode,
    pub command: String,
    pub required_action: String,
    pub decided_by: OpenDocAuthorizationSource,
    /// The subject the *service* named, never one a caller supplied. `None`
    /// outside a service session.
    pub subject: Option<String>,
    pub document_uuid: Option<String>,
    /// The role the service attested, when there is one.
    pub role: Option<OpenDocServiceRole>,
    pub allowed: bool,
    pub reason: String,
    pub warnings: Vec<String>,
}

impl OpenDocAuthorizationDecision {
    pub fn for_command(
        mode: OpenDocRuntimeMode,
        command: impl Into<String>,
        service_session: Option<&OpenDocServiceSession>,
    ) -> Self {
        Self::for_profile_command(
            OpenDocRuntimeProfile::for_mode(mode),
            command,
            service_session,
        )
    }

    /// The decision for one command.
    ///
    /// There is no `grants` parameter and there will not be one. A service
    /// session is the only thing that can make a command permitted in a
    /// service runtime, and it can only be built from what the service sent.
    pub fn for_profile_command(
        profile: OpenDocRuntimeProfile,
        command: impl Into<String>,
        service_session: Option<&OpenDocServiceSession>,
    ) -> Self {
        let command = command.into();
        let mode = profile.mode;
        let mut warnings = profile.warning_messages();
        let required_action = runtime_command_required_action(&command)
            .unwrap_or("unknown")
            .to_string();
        let session = service_session.filter(|session| session.is_valid());
        let (decided_by, allowed, reason) = if required_action == "unknown" {
            (
                OpenDocAuthorizationSource::RuntimeCapability,
                false,
                format!("unknown app command {command}"),
            )
        } else if runtime_command_requires_service(&command)
            && !profile.defers_permissions_to_a_service()
        {
            (
                OpenDocAuthorizationSource::RuntimeCapability,
                false,
                "runtime does not support service-managed sharing".to_string(),
            )
        } else if runtime_command_requires_storage(&command, OpenDocStorageBackend::Local)
            && !profile.supports_storage(OpenDocStorageBackend::Local)
        {
            (
                OpenDocAuthorizationSource::RuntimeCapability,
                false,
                "runtime does not support local object repositories".to_string(),
            )
        } else if runtime_command_requires_storage(&command, OpenDocStorageBackend::OpenDalFs)
            && !profile.supports_storage(OpenDocStorageBackend::OpenDalFs)
        {
            (
                OpenDocAuthorizationSource::RuntimeCapability,
                false,
                "runtime does not support OpenDAL filesystem repositories".to_string(),
            )
        } else if runtime_command_requires_storage(&command, OpenDocStorageBackend::Flat)
            && !profile.supports_storage(OpenDocStorageBackend::Flat)
        {
            (
                OpenDocAuthorizationSource::RuntimeCapability,
                false,
                "runtime does not support flat object namespaces".to_string(),
            )
        } else if runtime_command_requires_signing(&command) && !profile.signing_enabled {
            (
                OpenDocAuthorizationSource::RuntimeCapability,
                false,
                "runtime does not support private-key signing".to_string(),
            )
        } else if !profile.defers_permissions_to_a_service() {
            (
                OpenDocAuthorizationSource::RuntimeCapability,
                true,
                "runtime does not enforce document-level permissions".to_string(),
            )
        } else {
            match session {
                None => (
                    OpenDocAuthorizationSource::ServiceAnswerMissing,
                    false,
                    "no collaboration service session; the service has not answered for this subject"
                        .to_string(),
                ),
                Some(session) if session.role.allows_action(&required_action) => (
                    OpenDocAuthorizationSource::ServiceAnswer,
                    true,
                    format!(
                        "the service attested role {} for {}, which allows {required_action}",
                        session.role.as_str(),
                        session.subject
                    ),
                ),
                Some(session) => (
                    OpenDocAuthorizationSource::ServiceAnswer,
                    false,
                    format!(
                        "the service attested role {} for {}, which does not allow {required_action}",
                        session.role.as_str(),
                        session.subject
                    ),
                ),
            }
        };
        if !allowed {
            warnings.push(reason.clone());
        }
        Self {
            mode,
            command,
            required_action,
            decided_by,
            subject: session.map(|session| session.subject.clone()),
            document_uuid: session.map(|session| session.document_uuid.clone()),
            role: session.map(|session| session.role),
            allowed,
            reason,
            warnings,
        }
    }
}

/// A request to the service to grant `target_subject` a role on this document.
///
/// It is a *request*, not a grant: only the service writes grants, into its own
/// `service/permissions/` namespace. The invite carries the role the issuer
/// would like set and the service's answer about whether the issuer may ask.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocShareInvite {
    pub authorization: OpenDocAuthorizationDecision,
    pub issuer: Option<String>,
    pub target_subject: Option<String>,
    pub document_uuid: Option<String>,
    /// `None` when the request is not one the service could act on — the
    /// issuer may not share, there is no target, or the role is not a role.
    pub requested_role: Option<OpenDocServiceRole>,
    pub created_at_ms: u64,
    pub warnings: Vec<String>,
}

impl OpenDocShareInvite {
    pub fn for_runtime(
        mode: OpenDocRuntimeMode,
        target_subject: Option<String>,
        requested_role: Option<String>,
        service_session: Option<&OpenDocServiceSession>,
        created_at_ms: u64,
    ) -> Self {
        Self::for_profile(
            OpenDocRuntimeProfile::for_mode(mode),
            target_subject,
            requested_role,
            service_session,
            created_at_ms,
        )
    }

    pub fn for_profile(
        profile: OpenDocRuntimeProfile,
        target_subject: Option<String>,
        requested_role: Option<String>,
        service_session: Option<&OpenDocServiceSession>,
        created_at_ms: u64,
    ) -> Self {
        let target_subject = target_subject
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let authorization = OpenDocAuthorizationDecision::for_profile_command(
            profile,
            "create_runtime_share_invite",
            service_session,
        );
        let mut warnings = authorization.warnings.clone();
        let parsed_role = requested_role
            .as_deref()
            .and_then(OpenDocServiceRole::parse);
        let requested_role = if !authorization.allowed {
            None
        } else if target_subject.is_none() {
            warnings.push("share invite target subject is empty".to_string());
            None
        } else if parsed_role.is_none() {
            warnings.push(format!(
                "share invite role {:?} is not one of viewer, commenter, editor or owner",
                requested_role.unwrap_or_default()
            ));
            None
        } else {
            parsed_role
        };
        Self {
            issuer: authorization.subject.clone(),
            document_uuid: authorization.document_uuid.clone(),
            authorization,
            target_subject,
            requested_role,
            created_at_ms,
            warnings,
        }
    }
}

/// How the service would answer a batch a client is holding.
///
/// The service answers a *batch*, not its operations one at a time: it is
/// accepted, refused, or a byte-identical retry of something already logged
/// (ADR 0015, "History immutability" and "Durability before acknowledgement").
/// There is no fourth state — in particular nothing is ever *deferred*, which
/// is what the earlier `deferred_operations` field claimed. A batch the
/// service cannot place is refused and nothing is stored.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenDocSyncBatchOutcome {
    /// At least one operation would be committed.
    Accepted,
    /// Nothing would be stored and nothing about the document would change.
    Refused,
    /// Every operation in the batch is already durable under the same id. The
    /// service acknowledges without committing anything.
    Retried,
}

impl OpenDocSyncBatchOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Refused => "refused",
            Self::Retried => "retried",
        }
    }
}

/// The preflight a transport runs before putting a batch on the wire.
///
/// This is **not** an answer and does not pretend to be one — the answer comes
/// back over the socket as `Accepted` or `Rejected`. It applies the checks the
/// service applies, from what the service has already told this client, so a
/// transport learns about a batch it must not send without discovering it as a
/// rejection. Every rule mirrored here is stated in ADR 0015's "What the
/// service actually enforces": the session's actor binding, per-actor sequence
/// density, and history immutability.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocSyncRelayResult {
    pub authorization: OpenDocAuthorizationDecision,
    pub document_uuid: Option<String>,
    pub outcome: OpenDocSyncBatchOutcome,
    /// Operation ids (`actor#seq`) that would be committed.
    pub accepted_operations: Vec<String>,
    /// Operation ids the service already holds; resending them is a retry.
    pub retried_operations: Vec<String>,
    /// Operation ids in a refused batch. Refusal is all-or-nothing, so this is
    /// either empty or the whole batch.
    pub refused_operations: Vec<String>,
    pub presence: Vec<OpenDocPresencePeer>,
    pub warnings: Vec<String>,
}

impl OpenDocSyncRelayResult {
    pub fn for_runtime(
        mode: OpenDocRuntimeMode,
        operations: Vec<OpenDocRelayOperation>,
        service_session: Option<&OpenDocServiceSession>,
    ) -> Self {
        Self::for_profile(
            OpenDocRuntimeProfile::for_mode(mode),
            operations,
            service_session,
        )
    }

    pub fn for_profile(
        profile: OpenDocRuntimeProfile,
        operations: Vec<OpenDocRelayOperation>,
        service_session: Option<&OpenDocServiceSession>,
    ) -> Self {
        let authorization = OpenDocAuthorizationDecision::for_profile_command(
            profile,
            "relay_runtime_sync",
            service_session,
        );
        let session = service_session.filter(|session| session.is_valid());
        let operations = operations
            .into_iter()
            .map(OpenDocRelayOperation::normalized)
            .collect::<Vec<_>>();
        let all_ids = operations
            .iter()
            .map(OpenDocRelayOperation::id)
            .collect::<Vec<_>>();
        let mut warnings = authorization.warnings.clone();
        let mut accepted_operations = Vec::new();
        let mut retried_operations = Vec::new();
        let mut refused_operations = Vec::new();
        let mut outcome = OpenDocSyncBatchOutcome::Refused;

        match session {
            _ if !authorization.allowed => {
                refused_operations = all_ids;
            }
            None => {
                // Unreachable while `authorization.allowed` implies a session in
                // service mode, but stated rather than assumed.
                warnings.push("sync preflight needs a collaboration service session".to_string());
                refused_operations = all_ids;
            }
            Some(session) => {
                let mut expected = session.acknowledged_seq;
                let mut problem = None;
                for operation in &operations {
                    if operation.actor.is_empty() || operation.kind.is_empty() || operation.seq == 0
                    {
                        problem = Some("sync preflight refused a malformed operation".to_string());
                        break;
                    }
                    if operation.actor != session.actor {
                        problem = Some(format!(
                            "operation {} claims actor {} but the service bound this session to {}",
                            operation.id(),
                            operation.actor,
                            session.actor
                        ));
                        break;
                    }
                    if operation.seq <= session.acknowledged_seq {
                        // Already durable under this id: a resend of it is a
                        // retry, which the service acknowledges without
                        // committing. Whether the payload is byte-identical is
                        // the service's call, not this preflight's.
                        retried_operations.push(operation.id());
                        continue;
                    }
                    expected += 1;
                    if operation.seq != expected {
                        problem = Some(format!(
                            "operation {} is out of sequence; the next sequence for this actor is {expected}",
                            operation.id()
                        ));
                        break;
                    }
                    accepted_operations.push(operation.id());
                }
                match problem {
                    Some(message) => {
                        warnings.push(message);
                        accepted_operations.clear();
                        retried_operations.clear();
                        refused_operations = all_ids;
                    }
                    None if !accepted_operations.is_empty() => {
                        outcome = OpenDocSyncBatchOutcome::Accepted;
                    }
                    None if !retried_operations.is_empty() => {
                        outcome = OpenDocSyncBatchOutcome::Retried;
                    }
                    None => {
                        warnings.push("sync preflight was given an empty batch".to_string());
                    }
                }
            }
        }

        Self {
            document_uuid: authorization.document_uuid.clone(),
            authorization,
            outcome,
            accepted_operations,
            retried_operations,
            refused_operations,
            presence: session
                .map(|session| session.peers.clone())
                .unwrap_or_default(),
            warnings,
        }
    }
}

/// One operation in a batch, as far as the preflight cares.
///
/// An operation's identity is `(actor, seq)` — the same `OperationId` the merge
/// and the service use — so there is no separate opaque id to disagree with it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocRelayOperation {
    pub actor: String,
    pub seq: u64,
    pub kind: String,
}

impl OpenDocRelayOperation {
    pub fn id(&self) -> String {
        format!("{}#{}", self.actor, self.seq)
    }

    fn normalized(mut self) -> Self {
        self.actor = self.actor.trim().to_string();
        self.kind = self.kind.trim().to_string();
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocRuntimeLookupResult {
    pub authorization: OpenDocAuthorizationDecision,
    pub requested_document_uuid: Option<String>,
    pub requested_doi: Option<String>,
    pub resolved_document_uuid: Option<String>,
    pub manifest: Option<String>,
    pub lookup_source: String,
    pub used_scan: bool,
    pub warnings: Vec<String>,
}

impl OpenDocRuntimeLookupResult {
    pub fn for_runtime(
        mode: OpenDocRuntimeMode,
        document_uuid: Option<String>,
        doi: Option<String>,
        service_session: Option<&OpenDocServiceSession>,
        service_index: Vec<OpenDocRuntimeLookupEntry>,
        scanned_documents: Vec<OpenDocRuntimeLookupEntry>,
    ) -> Self {
        Self::for_profile(
            OpenDocRuntimeProfile::for_mode(mode),
            document_uuid,
            doi,
            service_session,
            service_index,
            scanned_documents,
        )
    }

    pub fn for_profile(
        profile: OpenDocRuntimeProfile,
        document_uuid: Option<String>,
        doi: Option<String>,
        service_session: Option<&OpenDocServiceSession>,
        service_index: Vec<OpenDocRuntimeLookupEntry>,
        scanned_documents: Vec<OpenDocRuntimeLookupEntry>,
    ) -> Self {
        let defers_to_service = profile.defers_permissions_to_a_service();
        let requested_document_uuid = document_uuid
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let requested_doi = doi
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let (service_index, invalid_service_entries) =
            normalize_runtime_lookup_entries(service_index);
        let (scanned_documents, invalid_scanned_entries) =
            normalize_runtime_lookup_entries(scanned_documents);
        let authorization = OpenDocAuthorizationDecision::for_profile_command(
            profile,
            "resolve_runtime_document_lookup",
            service_session,
        );
        let mut warnings = authorization.warnings.clone();
        if invalid_service_entries > 0 {
            warnings.push(format!(
                "runtime lookup ignored {invalid_service_entries} invalid service index entries"
            ));
        }
        if invalid_scanned_entries > 0 {
            warnings.push(format!(
                "runtime lookup ignored {invalid_scanned_entries} invalid scanned entries"
            ));
        }
        let mut resolved = None;
        let mut lookup_source = "unresolved".to_string();
        let mut used_scan = false;
        if !authorization.allowed {
            warnings.push("runtime lookup denied by authorization policy".to_string());
        } else if requested_document_uuid.is_none() && requested_doi.is_none() {
            warnings.push("runtime lookup requires a document UUID or DOI".to_string());
        } else if let Some(document_uuid) = requested_document_uuid.as_ref() {
            resolved = Some(OpenDocRuntimeLookupEntry {
                document_uuid: document_uuid.clone(),
                doi: requested_doi.clone(),
                manifest: lookup_manifest_for(
                    document_uuid,
                    requested_doi.as_deref(),
                    &service_index,
                )
                .or_else(|| {
                    lookup_manifest_for(document_uuid, requested_doi.as_deref(), &scanned_documents)
                }),
            });
            lookup_source = "direct-uuid".to_string();
        } else if defers_to_service {
            match lookup_entry_by_doi(&service_index, requested_doi.as_deref()) {
                RuntimeLookupMatch::Unique(entry) => {
                    resolved = Some(entry);
                    lookup_source = "service-index".to_string();
                }
                RuntimeLookupMatch::Ambiguous(count) => {
                    warnings.push(format!(
                        "runtime lookup found {count} service index entries for the requested DOI"
                    ));
                }
                RuntimeLookupMatch::None => {}
            }
        }
        if resolved.is_none() && authorization.allowed {
            match lookup_entry_by_doi(&scanned_documents, requested_doi.as_deref()) {
                RuntimeLookupMatch::Unique(entry) => {
                    resolved = Some(entry);
                    lookup_source = "scan-fallback".to_string();
                    used_scan = true;
                    warnings.push("runtime lookup used repository scan fallback".to_string());
                }
                RuntimeLookupMatch::Ambiguous(count) => {
                    warnings.push(format!(
                        "runtime lookup found {count} scanned entries for the requested DOI"
                    ));
                }
                RuntimeLookupMatch::None => {}
            }
        }
        let (resolved_document_uuid, manifest) = resolved
            .map(|entry| (Some(entry.document_uuid), entry.manifest))
            .unwrap_or((None, None));
        Self {
            authorization,
            requested_document_uuid,
            requested_doi,
            resolved_document_uuid,
            manifest,
            lookup_source,
            used_scan,
            warnings,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocRuntimeLookupEntry {
    pub document_uuid: String,
    pub doi: Option<String>,
    pub manifest: Option<String>,
}

impl OpenDocRuntimeLookupEntry {
    fn normalized(mut self) -> Self {
        self.document_uuid = self.document_uuid.trim().to_string();
        self.doi = self
            .doi
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.manifest = self
            .manifest
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self
    }

    fn is_valid(&self) -> bool {
        !self.document_uuid.is_empty()
    }
}

fn normalize_runtime_lookup_entries(
    entries: Vec<OpenDocRuntimeLookupEntry>,
) -> (Vec<OpenDocRuntimeLookupEntry>, usize) {
    let mut invalid = 0usize;
    let entries = entries
        .into_iter()
        .map(OpenDocRuntimeLookupEntry::normalized)
        .filter(|entry| {
            let valid = entry.is_valid();
            if !valid {
                invalid += 1;
            }
            valid
        })
        .collect();
    (entries, invalid)
}

fn runtime_command_requires_service(command: &str) -> bool {
    matches!(
        command,
        "create_runtime_share_invite" | "relay_runtime_sync"
    )
}

fn runtime_command_requires_storage(command: &str, backend: OpenDocStorageBackend) -> bool {
    match backend {
        OpenDocStorageBackend::Local => matches!(
            command,
            "save_local_repository"
                | "save_local_repository_or_candidate"
                | "open_local_repository"
                | "open_local_repository_by_doi"
                | "merge_local_repository_candidates"
                | "compact_local_repository"
        ),
        OpenDocStorageBackend::OpenDalFs => matches!(
            command,
            "save_opendal_fs_repository"
                | "save_opendal_fs_repository_or_candidate"
                | "open_opendal_fs_repository"
                | "open_opendal_fs_repository_by_doi"
                | "merge_opendal_fs_repository_candidates"
        ),
        OpenDocStorageBackend::Flat => matches!(
            command,
            "save_flat_repository"
                | "save_flat_repository_or_candidate"
                | "open_flat_repository"
                | "open_flat_repository_by_doi"
                | "merge_flat_repository_candidates"
        ),
    }
}

fn runtime_command_requires_signing(command: &str) -> bool {
    matches!(
        command,
        "sign_with_openssh_private_key"
            | "sign_blob_with_openssh_private_key"
            | "sign_fastq_blob_with_openssh_private_key"
            | "sign_image_pixels_blob_with_openssh_private_key"
    )
}

enum RuntimeLookupMatch {
    None,
    Unique(OpenDocRuntimeLookupEntry),
    Ambiguous(usize),
}

fn lookup_entry_by_doi(
    entries: &[OpenDocRuntimeLookupEntry],
    doi: Option<&str>,
) -> RuntimeLookupMatch {
    let Some(normalized) = doi.map(normalize_doi) else {
        return RuntimeLookupMatch::None;
    };
    let matches = entries
        .iter()
        .filter(|entry| !entry.document_uuid.trim().is_empty())
        .filter(|entry| {
            entry.doi.as_deref().map(normalize_doi).as_deref() == Some(normalized.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    match matches.len() {
        0 => RuntimeLookupMatch::None,
        1 => RuntimeLookupMatch::Unique(matches.into_iter().next().unwrap()),
        count => RuntimeLookupMatch::Ambiguous(count),
    }
}

fn lookup_manifest_for(
    document_uuid: &str,
    doi: Option<&str>,
    entries: &[OpenDocRuntimeLookupEntry],
) -> Option<String> {
    let normalized_uuid = document_uuid.trim();
    let normalized_doi = doi.map(normalize_doi);
    entries
        .iter()
        .find(|entry| {
            entry.document_uuid.trim() == normalized_uuid
                || normalized_doi.as_ref().is_some_and(|doi| {
                    entry.doi.as_deref().map(normalize_doi).as_deref() == Some(doi.as_str())
                })
        })
        .and_then(|entry| {
            entry
                .manifest
                .as_ref()
                .map(|manifest| manifest.trim().to_string())
                .filter(|manifest| !manifest.is_empty())
        })
}

fn normalize_doi(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service_profile() -> OpenDocRuntimeProfile {
        OpenDocRuntimeProfile::for_mode(OpenDocRuntimeMode::MultiUserService)
    }

    fn session(role: OpenDocServiceRole) -> OpenDocServiceSession {
        OpenDocServiceSession::new("alice", "actor-alice", "doc-1", role)
    }

    #[test]
    fn a_service_runtime_without_an_answer_authorizes_nothing() {
        let decision = OpenDocAuthorizationDecision::for_profile_command(
            service_profile(),
            "get_document",
            None,
        );
        assert!(!decision.allowed);
        assert_eq!(
            decision.decided_by,
            OpenDocAuthorizationSource::ServiceAnswerMissing
        );
        assert!(decision.subject.is_none());
        assert!(decision.role.is_none());
    }

    #[test]
    fn a_service_answer_decides_both_ways() {
        let viewer = session(OpenDocServiceRole::Viewer);
        let read = OpenDocAuthorizationDecision::for_profile_command(
            service_profile(),
            "get_document",
            Some(&viewer),
        );
        assert!(read.allowed);
        assert_eq!(read.decided_by, OpenDocAuthorizationSource::ServiceAnswer);
        assert_eq!(read.role, Some(OpenDocServiceRole::Viewer));

        let write = OpenDocAuthorizationDecision::for_profile_command(
            service_profile(),
            "add_paragraph",
            Some(&viewer),
        );
        assert!(!write.allowed);
        assert_eq!(write.decided_by, OpenDocAuthorizationSource::ServiceAnswer);

        let editor = session(OpenDocServiceRole::Editor);
        let write = OpenDocAuthorizationDecision::for_profile_command(
            service_profile(),
            "add_paragraph",
            Some(&editor),
        );
        assert!(write.allowed);
    }

    #[test]
    fn a_local_runtime_decides_capability_and_never_identity() {
        let decision = OpenDocAuthorizationDecision::for_profile_command(
            OpenDocRuntimeProfile::for_mode(OpenDocRuntimeMode::BrowserLocal),
            "add_paragraph",
            None,
        );
        assert!(decision.allowed);
        assert_eq!(
            decision.decided_by,
            OpenDocAuthorizationSource::RuntimeCapability
        );
        assert!(decision.subject.is_none());

        let refused = OpenDocAuthorizationDecision::for_profile_command(
            OpenDocRuntimeProfile::for_mode(OpenDocRuntimeMode::BrowserLocal),
            "save_local_repository",
            None,
        );
        assert!(!refused.allowed);
        assert_eq!(
            refused.decided_by,
            OpenDocAuthorizationSource::RuntimeCapability
        );
    }

    #[test]
    fn a_share_invite_is_a_request_only_an_owner_may_make() {
        let editor = session(OpenDocServiceRole::Editor);
        let refused = OpenDocShareInvite::for_profile(
            service_profile(),
            Some("bob".to_string()),
            Some("editor".to_string()),
            Some(&editor),
            7,
        );
        assert!(!refused.authorization.allowed);
        assert_eq!(refused.requested_role, None);

        let owner = session(OpenDocServiceRole::Owner);
        let invite = OpenDocShareInvite::for_profile(
            service_profile(),
            Some("bob".to_string()),
            Some("editor".to_string()),
            Some(&owner),
            7,
        );
        assert!(invite.authorization.allowed);
        assert_eq!(invite.requested_role, Some(OpenDocServiceRole::Editor));
        assert_eq!(invite.issuer.as_deref(), Some("alice"));
        assert_eq!(invite.document_uuid.as_deref(), Some("doc-1"));
    }

    fn operation(actor: &str, seq: u64) -> OpenDocRelayOperation {
        OpenDocRelayOperation {
            actor: actor.to_string(),
            seq,
            kind: "insert-text".to_string(),
        }
    }

    #[test]
    fn a_batch_is_accepted_refused_or_retried_and_never_deferred() {
        let mut editor = session(OpenDocServiceRole::Editor);
        editor.acknowledged_seq = 3;

        let accepted = OpenDocSyncRelayResult::for_profile(
            service_profile(),
            vec![operation("actor-alice", 4), operation("actor-alice", 5)],
            Some(&editor),
        );
        assert_eq!(accepted.outcome, OpenDocSyncBatchOutcome::Accepted);
        assert_eq!(
            accepted.accepted_operations,
            vec!["actor-alice#4", "actor-alice#5"]
        );

        let retried = OpenDocSyncRelayResult::for_profile(
            service_profile(),
            vec![operation("actor-alice", 2), operation("actor-alice", 3)],
            Some(&editor),
        );
        assert_eq!(retried.outcome, OpenDocSyncBatchOutcome::Retried);
        assert!(retried.accepted_operations.is_empty());

        // The gap this preflight exists to catch: a service refuses a
        // non-dense sequence rather than storing it.
        let gap = OpenDocSyncRelayResult::for_profile(
            service_profile(),
            vec![operation("actor-alice", 4), operation("actor-alice", 6)],
            Some(&editor),
        );
        assert_eq!(gap.outcome, OpenDocSyncBatchOutcome::Refused);
        assert_eq!(
            gap.refused_operations,
            vec!["actor-alice#4", "actor-alice#6"]
        );
        assert!(gap.accepted_operations.is_empty());

        let impersonated = OpenDocSyncRelayResult::for_profile(
            service_profile(),
            vec![operation("actor-bob", 4)],
            Some(&editor),
        );
        assert_eq!(impersonated.outcome, OpenDocSyncBatchOutcome::Refused);

        let viewer = session(OpenDocServiceRole::Viewer);
        let forbidden = OpenDocSyncRelayResult::for_profile(
            service_profile(),
            vec![operation("actor-alice", 1)],
            Some(&viewer),
        );
        assert_eq!(forbidden.outcome, OpenDocSyncBatchOutcome::Refused);
        assert!(!forbidden.authorization.allowed);
    }

    #[test]
    fn a_runtime_session_carries_the_services_answer_or_says_it_has_none() {
        let empty = OpenDocRuntimeSession::for_profile(service_profile(), None);
        assert!(empty.service_session.is_none());
        assert!(empty
            .warnings
            .iter()
            .any(|warning| warning.contains("no answer from a collaboration service")));

        let peer = OpenDocPresencePeer {
            subject: "bob".to_string(),
            actor: "actor-bob".to_string(),
            display_name: String::new(),
            role: OpenDocServiceRole::Commenter,
            cursor_anchor: Some("  ".to_string()),
            selection_anchor: Some("  ".to_string()),
            last_seen_ms: 5,
            connections: 2,
        };
        let joined = OpenDocRuntimeSession::for_profile(
            service_profile(),
            Some(session(OpenDocServiceRole::Editor).with_peers(vec![peer])),
        );
        let service_session = joined.service_session.expect("a session");
        assert_eq!(service_session.role, OpenDocServiceRole::Editor);
        assert_eq!(service_session.peers.len(), 1);
        assert_eq!(service_session.peers[0].display_name, "bob");
        assert_eq!(service_session.peers[0].actor, "actor-bob");
        assert_eq!(service_session.peers[0].connections, 2);
        assert_eq!(service_session.peers[0].cursor_anchor, None);
    }

    /// Every runtime enum spells itself the same way twice: once through
    /// `as_str`, which the workspace's parsers and the hand-written
    /// TypeScript unions follow, and once through serde, which is what a
    /// client actually receives. `OpenDocStorageBackend::OpenDalFs` is why
    /// this test exists — `rename_all = "kebab-case"` turned `opendal-fs`
    /// into `open-dal-fs` on the wire and nothing compared the two.
    #[test]
    fn every_runtime_enum_serializes_as_the_string_it_says_it_is() {
        fn same<T: Serialize + std::fmt::Debug>(value: T, expected: &str) {
            let json = serde_json::to_value(&value).expect("serialize");
            assert_eq!(
                json,
                serde_json::Value::String(expected.to_string()),
                "{value:?} serializes as {json} but calls itself {expected:?}"
            );
        }
        for mode in [
            OpenDocRuntimeMode::TauriLocal,
            OpenDocRuntimeMode::BrowserLocal,
            OpenDocRuntimeMode::HpcSingleUser,
            OpenDocRuntimeMode::MultiUserService,
        ] {
            same(mode, mode.as_str());
        }
        for authority in [
            OpenDocPermissionAuthority::LocalAdvisory,
            OpenDocPermissionAuthority::Service,
        ] {
            same(authority, authority.as_str());
        }
        for backend in [
            OpenDocStorageBackend::Local,
            OpenDocStorageBackend::Flat,
            OpenDocStorageBackend::OpenDalFs,
        ] {
            same(backend, backend.as_str());
        }
        for role in [
            OpenDocServiceRole::Viewer,
            OpenDocServiceRole::Commenter,
            OpenDocServiceRole::Editor,
            OpenDocServiceRole::Owner,
        ] {
            same(role, role.as_str());
        }
        for source in [
            OpenDocAuthorizationSource::RuntimeCapability,
            OpenDocAuthorizationSource::ServiceAnswer,
            OpenDocAuthorizationSource::ServiceAnswerMissing,
        ] {
            same(source, source.as_str());
        }
        for outcome in [
            OpenDocSyncBatchOutcome::Accepted,
            OpenDocSyncBatchOutcome::Refused,
            OpenDocSyncBatchOutcome::Retried,
        ] {
            same(outcome, outcome.as_str());
        }
    }
}
