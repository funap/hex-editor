use gpui::Action;
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = app)]
#[serde(deny_unknown_fields)]
pub struct OpenFile {
    pub path: String,
}

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = app)]
#[serde(deny_unknown_fields)]
pub struct SetFileTreeFolder {
    pub path: String,
}

#[derive(Clone, PartialEq, Action)]
pub struct Rename;

#[derive(Clone, PartialEq, Action)]
pub struct SelectItem;

#[derive(Clone, PartialEq, Action)]
pub struct OpenFolder;

#[derive(Clone, PartialEq, Action)]
pub struct CloseFolder;

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = app)]
#[serde(deny_unknown_fields)]
pub struct LoadChildren {
    pub path: String,
}

#[derive(Clone, PartialEq, Action)]
pub struct ToggleSearch;

#[derive(Clone, PartialEq, Action)]
pub struct SearchNext;

#[derive(Clone, PartialEq, Action)]
pub struct SearchPrev;

#[derive(Clone, PartialEq, Action)]
pub struct FocusHexView;

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = app)]
#[serde(deny_unknown_fields)]
pub struct OpenDiff {
    pub left_path: String,
    pub right_path: String,
}

#[derive(Clone, PartialEq, Action)]
pub struct NextDifference;

#[derive(Clone, PartialEq, Action)]
pub struct PrevDifference;

#[derive(Clone, PartialEq, Action)]
pub struct ToggleSyncScroll;

#[derive(Clone, PartialEq, Action)]
pub struct ToggleLeftPanel;

#[derive(Clone, PartialEq, Action)]
pub struct OpenSettings;

#[derive(Clone, PartialEq, Action)]
pub struct OpenFileDialog;

#[derive(Clone, PartialEq, Action)]
pub struct Quit;

#[derive(Clone, PartialEq, Action)]
pub struct SelectAll;

#[derive(Clone, PartialEq, Action)]
pub struct GoToBeginning;

#[derive(Clone, PartialEq, Action)]
pub struct GoToEnd;

#[derive(Clone, PartialEq, Action)]
pub struct SetEncodingAscii;

#[derive(Clone, PartialEq, Action)]
pub struct SetEncodingUtf8;

#[derive(Clone, PartialEq, Action)]
pub struct SetEncodingUtf16Le;

#[derive(Clone, PartialEq, Action)]
pub struct SetEncodingUtf16Be;

#[derive(Clone, PartialEq, Action)]
pub struct ShowFilesTab;

#[derive(Clone, PartialEq, Action)]
pub struct ShowStructureTab;

#[derive(Clone, PartialEq, Action)]
pub struct ShowChecksumTab;

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = app)]
pub struct LoadStructureDefinition;

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = app)]
pub struct ClearStructureDefinition;

#[derive(Clone, PartialEq, Action)]
pub struct OpenVisualMap;

#[derive(Clone, PartialEq, Action)]
pub struct CloseActivePanel;

#[derive(Clone, PartialEq, Action)]
pub struct AddCustomBreak;

#[derive(Clone, PartialEq, Action)]
pub struct RemoveCustomBreakBackward;

#[derive(Clone, PartialEq, Action)]
pub struct RemoveCustomBreakForward;

#[derive(Clone, PartialEq, Action)]
pub struct JoinLine;

#[derive(Clone, PartialEq, Action)]
pub struct ClearAllCustomBreaks;

#[derive(Clone, PartialEq, Action)]
pub struct ActivateNextTab;

#[derive(Clone, PartialEq, Action)]
pub struct ActivatePreviousTab;

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = app)]
pub struct ActivateTab {
    pub index: usize,
}

#[derive(Clone, PartialEq, Action)]
pub struct CloseOtherTabs;

#[derive(Clone, PartialEq, Action)]
pub struct CloseAllTabs;

#[derive(Clone, PartialEq, Action)]
pub struct SplitRight;

#[derive(Clone, PartialEq, Action)]
pub struct SplitDown;

#[derive(Clone, PartialEq, Action)]
pub struct ToggleInlineStructureView;

#[derive(Clone, PartialEq, Action)]
pub struct Copy;

#[derive(Clone, PartialEq, Action)]
pub struct CopyAsHexDump;

#[derive(Clone, PartialEq, Action)]
pub struct CopyAsCppArray;

#[derive(Clone, PartialEq, Action)]
pub struct CopyAsHexStream;

#[derive(Clone, PartialEq, Action)]
pub struct CopyAsHexSpaces;

#[derive(Clone, PartialEq, Action)]
pub struct CopyAsPrintableText;

#[derive(Clone, PartialEq, Action)]
pub struct CopyAsBase64;

#[derive(Clone, PartialEq, Action)]
pub struct CopyAsEscapedString;

#[derive(Clone, PartialEq, Action)]
pub struct CopyAsBinary;

#[derive(Clone, PartialEq, Action)]
pub struct CopyAsRustArray;

#[derive(Clone, PartialEq, Action)]
pub struct CopyAsJsonArray;
