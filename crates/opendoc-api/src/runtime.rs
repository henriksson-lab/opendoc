use crate::runtime_command_required_action;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenDocStorageBackend {
    Local,
    Flat,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocRuntimeProfile {
    pub mode: OpenDocRuntimeMode,
    pub label: String,
    pub permissions_enabled: bool,
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
            permissions_enabled: matches!(mode, OpenDocRuntimeMode::MultiUserService),
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocRuntimeSession {
    pub profile: OpenDocRuntimeProfile,
    pub authenticated_subject: Option<String>,
    pub document_uuid: Option<String>,
    pub permissions: Vec<OpenDocPermissionGrant>,
    pub presence: Vec<OpenDocPresencePeer>,
    pub warnings: Vec<String>,
}

impl OpenDocRuntimeSession {
    pub fn for_mode(
        mode: OpenDocRuntimeMode,
        authenticated_subject: Option<String>,
        document_uuid: Option<String>,
        presence: Vec<OpenDocPresencePeer>,
    ) -> Self {
        Self::for_mode_with_permissions(
            mode,
            authenticated_subject,
            document_uuid,
            presence,
            Vec::new(),
        )
    }

    pub fn for_mode_with_permissions(
        mode: OpenDocRuntimeMode,
        authenticated_subject: Option<String>,
        document_uuid: Option<String>,
        presence: Vec<OpenDocPresencePeer>,
        supplied_permissions: Vec<OpenDocPermissionGrant>,
    ) -> Self {
        Self::for_profile_with_permissions(
            OpenDocRuntimeProfile::for_mode(mode),
            authenticated_subject,
            document_uuid,
            presence,
            supplied_permissions,
        )
    }

    pub fn for_profile_with_permissions(
        profile: OpenDocRuntimeProfile,
        authenticated_subject: Option<String>,
        document_uuid: Option<String>,
        presence: Vec<OpenDocPresencePeer>,
        supplied_permissions: Vec<OpenDocPermissionGrant>,
    ) -> Self {
        let subject = authenticated_subject
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let document_uuid = document_uuid
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let mut warnings = profile.warning_messages();
        let permissions = if profile.permissions_enabled {
            let supplied_permissions = supplied_permissions
                .into_iter()
                .map(OpenDocPermissionGrant::normalized)
                .filter(OpenDocPermissionGrant::is_valid)
                .collect::<Vec<_>>();
            if !supplied_permissions.is_empty() {
                supplied_permissions
            } else {
                match &subject {
                    Some(subject) => {
                        OpenDocPermissionGrant::service_default_grants(subject, &document_uuid)
                    }
                    None => {
                        warnings.push(
                            "Multi-user service mode requires an authenticated subject before document permissions can be evaluated."
                                .to_string(),
                        );
                        Vec::new()
                    }
                }
            }
        } else {
            if !supplied_permissions.is_empty() {
                warnings.push(
                    "Runtime permission grants are ignored outside multi-user service mode."
                        .to_string(),
                );
            }
            Vec::new()
        };
        let mut presence = presence
            .into_iter()
            .map(OpenDocPresencePeer::normalized)
            .filter(|peer| !peer.subject.is_empty())
            .collect::<Vec<_>>();
        if let Some(subject) = &subject {
            if !presence.iter().any(|peer| peer.subject == *subject) {
                presence.push(OpenDocPresencePeer {
                    subject: subject.clone(),
                    display_name: subject.clone(),
                    role: if profile.permissions_enabled {
                        "editor".to_string()
                    } else {
                        "owner".to_string()
                    },
                    cursor_anchor: None,
                    last_seen_ms: 0,
                });
            }
        }
        presence.sort_by(|left, right| {
            (
                left.subject.as_str(),
                left.display_name.as_str(),
                left.role.as_str(),
            )
                .cmp(&(
                    right.subject.as_str(),
                    right.display_name.as_str(),
                    right.role.as_str(),
                ))
        });
        Self {
            profile,
            authenticated_subject: subject,
            document_uuid,
            permissions,
            presence,
            warnings,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocPermissionGrant {
    pub subject: String,
    pub action: String,
    pub scope: String,
    pub document_uuid: Option<String>,
}

impl OpenDocPermissionGrant {
    fn service_default_grants(subject: &str, document_uuid: &Option<String>) -> Vec<Self> {
        ["read", "comment", "write", "share"]
            .into_iter()
            .map(|action| Self {
                subject: subject.to_string(),
                action: action.to_string(),
                scope: if document_uuid.is_some() {
                    "document".to_string()
                } else {
                    "repository".to_string()
                },
                document_uuid: document_uuid.clone(),
            })
            .collect()
    }

    fn normalized(mut self) -> Self {
        self.subject = self.subject.trim().to_string();
        self.action = self.action.trim().to_string();
        self.scope = self.scope.trim().to_string();
        self.document_uuid = self
            .document_uuid
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self
    }

    fn is_valid(&self) -> bool {
        !self.subject.is_empty() && !self.action.is_empty() && !self.scope.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocPresencePeer {
    pub subject: String,
    pub display_name: String,
    pub role: String,
    pub cursor_anchor: Option<String>,
    pub last_seen_ms: u64,
}

impl OpenDocPresencePeer {
    fn normalized(mut self) -> Self {
        self.subject = self.subject.trim().to_string();
        self.display_name = self.display_name.trim().to_string();
        self.role = self.role.trim().to_string();
        self.cursor_anchor = self
            .cursor_anchor
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if self.display_name.is_empty() {
            self.display_name = self.subject.clone();
        }
        if self.role.is_empty() {
            self.role = "viewer".to_string();
        }
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocAuthorizationDecision {
    pub mode: OpenDocRuntimeMode,
    pub command: String,
    pub required_action: String,
    pub subject: Option<String>,
    pub document_uuid: Option<String>,
    pub allowed: bool,
    pub reason: String,
    pub warnings: Vec<String>,
}

impl OpenDocAuthorizationDecision {
    pub fn for_command(
        mode: OpenDocRuntimeMode,
        subject: Option<String>,
        document_uuid: Option<String>,
        command: impl Into<String>,
        grants: Vec<OpenDocPermissionGrant>,
    ) -> Self {
        Self::for_profile_command(
            OpenDocRuntimeProfile::for_mode(mode),
            subject,
            document_uuid,
            command,
            grants,
        )
    }

    pub fn for_profile_command(
        profile: OpenDocRuntimeProfile,
        subject: Option<String>,
        document_uuid: Option<String>,
        command: impl Into<String>,
        grants: Vec<OpenDocPermissionGrant>,
    ) -> Self {
        let command = command.into();
        let mode = profile.mode;
        let subject = subject
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let document_uuid = document_uuid
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let grants = grants
            .into_iter()
            .map(OpenDocPermissionGrant::normalized)
            .filter(OpenDocPermissionGrant::is_valid)
            .collect::<Vec<_>>();
        let mut warnings = profile.warning_messages();
        let required_action = runtime_command_required_action(&command)
            .unwrap_or("unknown")
            .to_string();
        let (allowed, reason) = if required_action == "unknown" {
            (false, format!("unknown app command {command}"))
        } else if runtime_command_requires_service(&command)
            && !matches!(mode, OpenDocRuntimeMode::MultiUserService)
        {
            (
                false,
                "runtime does not support service-managed sharing".to_string(),
            )
        } else if runtime_command_requires_storage(&command, OpenDocStorageBackend::Local)
            && !profile.supports_storage(OpenDocStorageBackend::Local)
        {
            (
                false,
                "runtime does not support local object repositories".to_string(),
            )
        } else if runtime_command_requires_storage(&command, OpenDocStorageBackend::OpenDalFs)
            && !profile.supports_storage(OpenDocStorageBackend::OpenDalFs)
        {
            (
                false,
                "runtime does not support OpenDAL filesystem repositories".to_string(),
            )
        } else if runtime_command_requires_storage(&command, OpenDocStorageBackend::Flat)
            && !profile.supports_storage(OpenDocStorageBackend::Flat)
        {
            (
                false,
                "runtime does not support flat object namespaces".to_string(),
            )
        } else if runtime_command_requires_signing(&command) && !profile.signing_enabled {
            (
                false,
                "runtime does not support private-key signing".to_string(),
            )
        } else if !profile.permissions_enabled {
            (
                true,
                "runtime does not enforce document-level permissions".to_string(),
            )
        } else if subject.is_none() {
            (
                false,
                "multi-user service mode requires an authenticated subject".to_string(),
            )
        } else {
            let subject_value = subject.as_deref().unwrap_or_default();
            let has_grant = grants.iter().any(|grant| {
                grant.subject == subject_value
                    && grant.action == required_action
                    && grant_scope_matches(grant, &document_uuid)
            });
            if has_grant {
                (true, "service permission grant allows command".to_string())
            } else {
                (
                    false,
                    format!("missing {required_action} permission grant for service command"),
                )
            }
        };
        if !allowed {
            warnings.push(reason.clone());
        }
        Self {
            mode,
            command,
            required_action,
            subject,
            document_uuid,
            allowed,
            reason,
            warnings,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocShareInvite {
    pub authorization: OpenDocAuthorizationDecision,
    pub issuer: Option<String>,
    pub target_subject: Option<String>,
    pub document_uuid: Option<String>,
    pub grants: Vec<OpenDocPermissionGrant>,
    pub created_at_ms: u64,
    pub warnings: Vec<String>,
}

impl OpenDocShareInvite {
    pub fn for_runtime(
        mode: OpenDocRuntimeMode,
        issuer: Option<String>,
        document_uuid: Option<String>,
        target_subject: Option<String>,
        actions: Vec<String>,
        issuer_permissions: Vec<OpenDocPermissionGrant>,
        created_at_ms: u64,
    ) -> Self {
        Self::for_profile(
            OpenDocRuntimeProfile::for_mode(mode),
            issuer,
            document_uuid,
            target_subject,
            actions,
            issuer_permissions,
            created_at_ms,
        )
    }

    pub fn for_profile(
        profile: OpenDocRuntimeProfile,
        issuer: Option<String>,
        document_uuid: Option<String>,
        target_subject: Option<String>,
        actions: Vec<String>,
        issuer_permissions: Vec<OpenDocPermissionGrant>,
        created_at_ms: u64,
    ) -> Self {
        let mode = profile.mode;
        let issuer = issuer
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let target_subject = target_subject
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let document_uuid = document_uuid
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let authorization = OpenDocAuthorizationDecision::for_profile_command(
            profile,
            issuer.clone(),
            document_uuid.clone(),
            "create_runtime_share_invite",
            issuer_permissions,
        );
        let mut warnings = authorization.warnings.clone();
        let grants = if !authorization.allowed {
            Vec::new()
        } else if !matches!(mode, OpenDocRuntimeMode::MultiUserService) {
            warnings.push(
                "Only multi-user service mode can persist document share grants.".to_string(),
            );
            Vec::new()
        } else if target_subject.is_none() {
            warnings.push("share invite target subject is empty".to_string());
            Vec::new()
        } else if document_uuid.is_none() {
            warnings.push("share invite requires a document UUID".to_string());
            Vec::new()
        } else {
            let mut seen = BTreeSet::new();
            actions
                .into_iter()
                .map(|action| action.trim().to_string())
                .filter(|action| ["read", "comment", "write", "share"].contains(&action.as_str()))
                .filter(|action| seen.insert(action.clone()))
                .map(|action| OpenDocPermissionGrant {
                    subject: target_subject.clone().unwrap_or_default(),
                    action,
                    scope: "document".to_string(),
                    document_uuid: document_uuid.clone(),
                })
                .collect()
        };
        let mut invite = Self {
            authorization,
            issuer,
            target_subject,
            document_uuid,
            grants,
            created_at_ms,
            warnings,
        };
        if invite.authorization.allowed
            && invite.grants.is_empty()
            && !invite.warnings.iter().any(|warning| {
                warning.contains("share invite did not contain any supported actions")
            })
        {
            invite
                .warnings
                .push("share invite did not contain any supported actions".to_string());
        }
        invite
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocSyncRelayResult {
    pub authorization: OpenDocAuthorizationDecision,
    pub document_uuid: Option<String>,
    pub base_manifest: Option<String>,
    pub accepted_operations: Vec<String>,
    pub deferred_operations: Vec<String>,
    pub rejected_operations: Vec<String>,
    pub presence: Vec<OpenDocPresencePeer>,
    pub warnings: Vec<String>,
}

impl OpenDocSyncRelayResult {
    pub fn for_runtime(
        mode: OpenDocRuntimeMode,
        subject: Option<String>,
        document_uuid: Option<String>,
        base_manifest: Option<String>,
        operations: Vec<OpenDocRelayOperation>,
        grants: Vec<OpenDocPermissionGrant>,
        presence: Vec<OpenDocPresencePeer>,
    ) -> Self {
        Self::for_profile(
            OpenDocRuntimeProfile::for_mode(mode),
            subject,
            document_uuid,
            base_manifest,
            operations,
            grants,
            presence,
        )
    }

    pub fn for_profile(
        profile: OpenDocRuntimeProfile,
        subject: Option<String>,
        document_uuid: Option<String>,
        base_manifest: Option<String>,
        operations: Vec<OpenDocRelayOperation>,
        grants: Vec<OpenDocPermissionGrant>,
        presence: Vec<OpenDocPresencePeer>,
    ) -> Self {
        let mode = profile.mode;
        let document_uuid = document_uuid
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let base_manifest = base_manifest
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let authorization = OpenDocAuthorizationDecision::for_profile_command(
            profile.clone(),
            subject.clone(),
            document_uuid.clone(),
            "relay_runtime_sync",
            grants,
        );
        let session = OpenDocRuntimeSession::for_profile_with_permissions(
            profile,
            subject,
            document_uuid.clone(),
            presence,
            Vec::new(),
        );
        let operations = operations
            .into_iter()
            .map(OpenDocRelayOperation::normalized)
            .collect::<Vec<_>>();
        let mut warnings = authorization.warnings.clone();
        warnings.extend(session.warnings.clone());
        let mut accepted_operations = Vec::new();
        let mut deferred_operations = Vec::new();
        let mut rejected_operations = Vec::new();
        let mut seen = BTreeSet::new();
        let mut seen_actor_sequences = BTreeSet::new();
        if !authorization.allowed {
            rejected_operations.extend(operations.into_iter().map(|operation| operation.id));
        } else if !matches!(mode, OpenDocRuntimeMode::MultiUserService) {
            warnings.push("sync relay is only active in multi-user service mode".to_string());
            deferred_operations.extend(operations.into_iter().map(|operation| operation.id));
        } else if document_uuid.is_none() {
            warnings.push("sync relay requires a document UUID".to_string());
            rejected_operations.extend(operations.into_iter().map(|operation| operation.id));
        } else {
            let authenticated_actor = session.authenticated_subject.as_deref().unwrap_or_default();
            for operation in operations {
                if operation.id.trim().is_empty()
                    || operation.actor.trim().is_empty()
                    || operation.kind.trim().is_empty()
                    || operation.seq == 0
                {
                    warnings.push("sync relay rejected malformed operation envelope".to_string());
                    rejected_operations.push(operation.id);
                } else if operation.actor != authenticated_actor {
                    warnings.push(format!(
                        "sync relay rejected operation {} because actor {} did not match authenticated subject {}",
                        operation.id, operation.actor, authenticated_actor
                    ));
                    rejected_operations.push(operation.id);
                } else if !seen.insert(operation.id.clone()) {
                    warnings.push(format!(
                        "sync relay deferred duplicate operation {}",
                        operation.id
                    ));
                    deferred_operations.push(operation.id);
                } else if !seen_actor_sequences.insert((operation.actor.clone(), operation.seq)) {
                    warnings.push(format!(
                        "sync relay deferred duplicate actor sequence {}#{}",
                        operation.actor, operation.seq
                    ));
                    deferred_operations.push(operation.id);
                } else if operation.base_manifest.as_deref() != base_manifest.as_deref() {
                    warnings.push(format!(
                        "sync relay deferred operation {} for candidate reconciliation",
                        operation.id
                    ));
                    deferred_operations.push(operation.id);
                } else {
                    accepted_operations.push(operation.id);
                }
            }
        }
        Self {
            authorization,
            document_uuid,
            base_manifest,
            accepted_operations,
            deferred_operations,
            rejected_operations,
            presence: session.presence,
            warnings,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenDocRelayOperation {
    pub id: String,
    pub actor: String,
    pub seq: u64,
    pub kind: String,
    pub base_manifest: Option<String>,
}

impl OpenDocRelayOperation {
    fn normalized(mut self) -> Self {
        self.id = self.id.trim().to_string();
        self.actor = self.actor.trim().to_string();
        self.kind = self.kind.trim().to_string();
        self.base_manifest = self
            .base_manifest
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
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
        subject: Option<String>,
        document_uuid: Option<String>,
        doi: Option<String>,
        grants: Vec<OpenDocPermissionGrant>,
        service_index: Vec<OpenDocRuntimeLookupEntry>,
        scanned_documents: Vec<OpenDocRuntimeLookupEntry>,
    ) -> Self {
        Self::for_profile(
            OpenDocRuntimeProfile::for_mode(mode),
            subject,
            document_uuid,
            doi,
            grants,
            service_index,
            scanned_documents,
        )
    }

    pub fn for_profile(
        profile: OpenDocRuntimeProfile,
        subject: Option<String>,
        document_uuid: Option<String>,
        doi: Option<String>,
        grants: Vec<OpenDocPermissionGrant>,
        service_index: Vec<OpenDocRuntimeLookupEntry>,
        scanned_documents: Vec<OpenDocRuntimeLookupEntry>,
    ) -> Self {
        let mode = profile.mode;
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
            subject,
            requested_document_uuid.clone(),
            "resolve_runtime_document_lookup",
            grants,
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
        } else if matches!(mode, OpenDocRuntimeMode::MultiUserService) {
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

fn grant_scope_matches(grant: &OpenDocPermissionGrant, document_uuid: &Option<String>) -> bool {
    match (grant.scope.as_str(), &grant.document_uuid, document_uuid) {
        ("repository", _, _) => true,
        ("document", Some(grant_uuid), Some(document_uuid)) => grant_uuid == document_uuid,
        ("document", None, _) => true,
        _ => false,
    }
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
