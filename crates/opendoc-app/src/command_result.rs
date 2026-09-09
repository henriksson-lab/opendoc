use serde::{Deserialize, Serialize};

use crate::{
    AppAuditView, AppDocument, AppEditorSelection, AppSpreadsheetSelection, EditorResult,
    OpenDocAuthorizationDecision, OpenDocRuntimeLookupResult, OpenDocRuntimeProfile,
    OpenDocRuntimeSession, OpenDocShareInvite, OpenDocSyncRelayResult,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
#[allow(clippy::large_enum_variant)]
pub enum AppCommandResult {
    Document(AppDocument),
    Editor(EditorResult),
    Text(String),
    AuditView(AppAuditView),
    RuntimeProfile(OpenDocRuntimeProfile),
    RuntimeSession(OpenDocRuntimeSession),
    AuthorizationDecision(OpenDocAuthorizationDecision),
    ShareInvite(OpenDocShareInvite),
    SyncRelay(OpenDocSyncRelayResult),
    RuntimeLookup(OpenDocRuntimeLookupResult),
    SpreadsheetSelection(AppSpreadsheetSelection),
    EditorSelection(AppEditorSelection),
}
