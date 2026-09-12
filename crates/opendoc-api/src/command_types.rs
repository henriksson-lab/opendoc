#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandArg {
    pub name: &'static str,
    pub ty: CommandArgType,
    pub optional: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandArgType {
    String,
    NullableString,
    Number,
    Boolean,
    NullableBoolean,
    StringArray,
    NumberArray,
    Object,
    ObjectArray,
    RuntimeMode,
    EditorSelection,
    CitationItems,
    SpreadsheetCellEdit,
    SpreadsheetCellEdits,
}

impl CommandArgType {
    pub fn as_contract_type(self) -> &'static str {
        match self {
            Self::String | Self::RuntimeMode => "string",
            Self::NullableString => "string|null",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::NullableBoolean => "boolean|null",
            Self::StringArray => "string[]",
            Self::NumberArray => "number[]",
            Self::Object => "object",
            Self::ObjectArray => "object[]",
            Self::EditorSelection => "EditorSelection",
            Self::CitationItems => "AppCitationItem[]",
            Self::SpreadsheetCellEdit => "SpreadsheetCellEdit",
            Self::SpreadsheetCellEdits => "SpreadsheetCellEdit[]",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CommandReturn {
    AppDocument,
    AppAuditView,
    OpenDocRuntimeProfile,
    OpenDocRuntimeSession,
    OpenDocAuthorizationDecision,
    OpenDocShareInvite,
    OpenDocSyncRelayResult,
    OpenDocRuntimeLookupResult,
    AppSpreadsheetSelection,
    AppEditorSelection,
    AppFindMatches,
    AppVersionView,
    /// Exported bytes plus what the target format could not carry.
    AppExport,
    /// Which page each block falls on, and where.
    AppDocumentLayout,
    String,
    EditorResult,
}

impl CommandReturn {
    pub fn as_contract_type(self) -> &'static str {
        match self {
            Self::AppDocument => "AppDocument",
            Self::AppAuditView => "AppAuditView",
            Self::OpenDocRuntimeProfile => "OpenDocRuntimeProfile",
            Self::OpenDocRuntimeSession => "OpenDocRuntimeSession",
            Self::OpenDocAuthorizationDecision => "OpenDocAuthorizationDecision",
            Self::OpenDocShareInvite => "OpenDocShareInvite",
            Self::OpenDocSyncRelayResult => "OpenDocSyncRelayResult",
            Self::OpenDocRuntimeLookupResult => "OpenDocRuntimeLookupResult",
            Self::AppSpreadsheetSelection => "AppSpreadsheetSelection",
            Self::AppEditorSelection => "AppEditorSelection",
            Self::AppFindMatches => "AppFindMatches",
            Self::AppVersionView => "AppVersionView",
            Self::AppExport => "AppExport",
            Self::AppDocumentLayout => "AppDocumentLayout",
            Self::String => "string",
            Self::EditorResult => "EditorResult",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub returns: CommandReturn,
    pub args: &'static [CommandArg],
    pub undoable: bool,
    pub allowed_without_open_document: bool,
    pub required_action: Option<&'static str>,
}
