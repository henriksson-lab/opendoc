use serde::{Deserialize, Serialize};

use crate::{
    AppAuditView, AppDocument, AppDocumentLayout, AppEditorSelection, AppExport, AppFindMatches,
    AppSpreadsheetSelection, AppVersionView, EditorResult, OpenDocAuthorizationDecision,
    OpenDocRuntimeLookupResult, OpenDocRuntimeProfile, OpenDocRuntimeSession, OpenDocShareInvite,
    OpenDocSyncRelayResult,
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
    FindMatches(AppFindMatches),
    VersionView(AppVersionView),
    /// An export: the bytes plus what the target format could not carry.
    Export(AppExport),
    /// Where the document's blocks fall on which pages.
    DocumentLayout(AppDocumentLayout),
}
