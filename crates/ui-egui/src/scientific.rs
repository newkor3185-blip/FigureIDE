//! Shared scientific workspace state, commands, and R message orchestration.

#[cfg(test)]
#[path = "scientific_ui_tests.rs"]
mod ui_verification;

mod console;
mod data_viewer;
pub(crate) mod diagnostics;
#[cfg(test)]
pub(crate) mod distribution;
mod environment;
pub(crate) mod export;
pub(crate) mod graph_style;
pub(crate) mod graph_tab;
mod inspector;
pub(crate) mod linked_plot_protection;
mod linked_plots;
mod plot_registry;
pub(crate) mod project;
mod source_editor;
mod status;
mod workspace;

pub(crate) use self::project::{open_fide_project_bytes, open_project_from_path};

#[cfg(test)]
use self::console::remove_console_enter_newline;
use self::console::{clear_console_transcript, show_console};
pub(crate) use self::data_viewer::show_data_viewer_tab;
use self::environment::{show_environment, show_help, show_history, show_packages};
use self::graph_style::GraphStyle;
use self::inspector::observe_selected_panel_resize;
pub(crate) use self::inspector::show_inspector_plot_properties;
use self::linked_plot_protection::{CapturedRotation, PendingUnlinkEdit};
use self::linked_plots::apply_rendered_plot;
#[cfg(test)]
use self::linked_plots::show_linked_panel_entries as show_linked_panels;
#[cfg(test)]
use self::plot_registry::show_plot_registry;
#[cfg(test)]
use self::source_editor::{append_eof_newline, choose_script_encoding, code_for_char_range};
use self::source_editor::{
    go_to_plot_source, line_start_byte, navigate_source, request_close_script, run_current_expression, selected_code, show_close_script_prompt,
    show_script_encoding_prompt, show_script_tabs, show_source_editor,
};
#[cfg(test)]
use self::status::show_status_indicator;
use self::status::{RIndicator, r_indicator, workspace_status};
pub(crate) use self::workspace::app_bar_rect_id;
pub use self::workspace::show;
#[cfg(test)]
use self::workspace::show_bottom_tabs;
use self::workspace::workspace_geometry_id;

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

enum RMessage {
    Availability(Result<String, String>),
    Started(&'static str),
    ConsoleChunk(String),
    ConsoleInputParsed(Result<crate::r_session::ParseState, String>),
    Execution(Result<crate::r_session::Execution, String>, bool, Option<SourceExecutionDefinitions>),
    SourceDefinitions(String, Result<Vec<(u64, usize, String)>, String>),
    Session(Result<String, String>),
    Rendered(u64, u64, String, PanelAction, Result<RenderedPlot, String>),
    DataPage(String, Result<crate::r_session::DataPage, String>),
    EnvironmentAction(Result<crate::r_session::Execution, String>),
    Help(Result<String, String>),
    Packages(Result<Vec<crate::r_session::RPackage>, String>),
    CompletionMetadata {
        script_id: u64,
        source_revision: String,
        object_columns: Option<(String, Vec<(String, String)>)>,
        namespace_items: Option<((String, bool), Vec<crate::r_editor::CompletionCandidate>)>,
    },
    RDataInspected {
        path: String,
        policy_override: Option<crate::r_session::ImportCollisionPolicy>,
        inspection: Result<crate::r_session::RDataInspection, String>,
    },
    RDataImported {
        path: String,
        result: Result<(crate::r_session::Execution, usize, usize), String>,
    },
}

pub(crate) const MAX_QUEUE_CAPACITY: usize = 100;

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub(crate) struct QueuedRCommand {
    pub(crate) id: u64,
    pub(crate) script_id: Option<u64>,
    pub(crate) code: String,
    pub(crate) source_revision: Option<String>,
    pub(crate) start_line: Option<usize>,
    pub(crate) refresh_linked: bool,
    pub(crate) is_selection: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveCompletion {
    pub(crate) script_id: u64,
    pub(crate) source_revision: String,
    pub(crate) context: crate::r_editor::CompletionContext,
    pub(crate) candidates: Vec<crate::r_editor::CompletionCandidate>,
    pub(crate) selected: usize,
    pub(crate) scroll_to_selected: bool,
}

type SourceExecutionDefinitions = (u64, Vec<(String, usize)>, String);

#[derive(Clone)]
struct SourceOrigin {
    script_id: u64,
    line: usize,
    source_revision: String,
}

struct RenderedPlot {
    variable: String,
    svg: String,
    plots: Vec<crate::r_session::RObject>,
    action: PanelAction,
}

#[derive(Clone, PartialEq)]
struct GraphRenderRequest {
    style: GraphStyle,
    width_mm: f64,
    height_mm: f64,
}

impl GraphRenderRequest {
    fn matches(&self, action: &PanelAction) -> bool {
        matches!(action, PanelAction::Graph { width_mm, height_mm, style, .. }
            if self.width_mm == *width_mm && self.height_mm == *height_mm && self.style == *style)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum LinkedPanelState {
    Offline,
    MissingObject,
    IncompatibleObject,
    ReadyToUpdate,
    Updating,
    Connected,
    Error,
}

#[derive(Clone)]
enum PanelAction {
    Create { label: String, width_mm: f64, height_mm: f64 },
    Refresh { panel_id: u64, generated_id: u64, width_mm: f64, height_mm: f64 },
    Reflow { panel_id: u64, generated_id: u64, width_mm: f64, height_mm: f64 },
    Graph { panel_id: u64, generated_id: u64, width_mm: f64, height_mm: f64, style: GraphStyle },
    Relink { panel_id: u64, generated_id: u64, target_variable: String, width_mm: f64, height_mm: f64 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BottomTab {
    #[default]
    Console,
    #[cfg(test)]
    PlotRegistry,
    #[cfg(test)]
    LinkedPanels,
    Environment,
    History,
    Help,
    Packages,
}

impl BottomTab {
    const ALL: [Self; 5] = [Self::Console, Self::Environment, Self::History, Self::Help, Self::Packages];

    fn key(self) -> &'static str {
        match self {
            Self::Console => "console",
            #[cfg(test)]
            Self::PlotRegistry => "plots",
            #[cfg(test)]
            Self::LinkedPanels => "linked",
            Self::Environment => "environment",
            Self::History => "history",
            Self::Help => "help",
            Self::Packages => "packages",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Console => "Console",
            #[cfg(test)]
            Self::PlotRegistry => "Plot Registry",
            #[cfg(test)]
            Self::LinkedPanels => "Linked Plot Panel",
            Self::Environment => "Environment",
            Self::History => "History",
            Self::Help => "Help",
            Self::Packages => "Packages",
        }
    }

    fn from_key(key: &str) -> Self {
        Self::ALL.into_iter().find(|tab| tab.key() == key).unwrap_or_default()
    }
}

fn is_graph_tab_request(key: &str) -> bool {
    matches!(key, "plots" | "linked" | "linked_panels" | "graph")
}

fn focus_graph_tab(app: &mut crate::VectorcraftApp) {
    app.scientific.enabled = true;
    if let Err(error) = app.run("window.panel.activate", serde_json::json!({"panel": "graph"})) {
        app.scientific.r_status = format!("Could not open Graph tab: {error}");
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ResizeMode {
    #[default]
    Reflow,
    Scale,
}

impl ResizeMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Reflow => "reflow",
            Self::Scale => "scale",
        }
    }
    fn from_str(value: &str) -> Self {
        if value == "scale" { Self::Scale } else { Self::Reflow }
    }
}

const DEFAULT_PLOT_WIDTH_MM: f64 = 70.0;
// Keep the previous 7:5 device aspect ratio when no plot-specific ratio is available.
const DEFAULT_PLOT_HEIGHT_MM: f64 = 50.0;
const DOC_POINTS_PER_MM: f64 = 72.0 / 25.4;

pub(crate) struct ScriptDocument {
    pub(crate) id: u64,
    pub(crate) untitled_name: String,
    pub(crate) text: String,
    pub(crate) path: Option<String>,
    pub(crate) saved_text: String,
    pub(crate) saved_bytes: Option<Vec<u8>>,
    pub(crate) source_format: ScriptFormat,
    pub(crate) highlight_cache: crate::r_editor::HighlightCache,
    pub(crate) pending_cursor_index: Option<usize>,
    pub(crate) pending_reveal_index: Option<usize>,
    pub(crate) reveal_requested: bool,
}

impl ScriptDocument {
    pub(crate) fn new(id: u64, text: String, path: Option<String>, saved_bytes: Option<Vec<u8>>) -> Self {
        Self {
            id,
            untitled_name: if id == 1 { "Untitled.R".into() } else { format!("Untitled-{id}.R") },
            saved_text: text.clone(),
            text,
            path,
            saved_bytes,
            source_format: ScriptFormat::default(),
            highlight_cache: Default::default(),
            pending_cursor_index: None,
            pending_reveal_index: None,
            reveal_requested: false,
        }
    }

    fn is_dirty(&self) -> bool {
        self.text != self.saved_text
    }

    fn filename(&self) -> &str {
        self.path.as_deref().and_then(|path| std::path::Path::new(path).file_name()).and_then(|name| name.to_str()).unwrap_or(&self.untitled_name)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ScriptEncoding {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    WindowsKorean,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ScriptLineEnding {
    #[default]
    Lf,
    CrLf,
    Cr,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScriptFormat {
    pub(crate) encoding: ScriptEncoding,
    pub(crate) line_ending: ScriptLineEnding,
}

impl ScriptLineEnding {
    fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }
}

#[derive(Clone)]
struct PendingScriptDecode {
    path: String,
    bytes: Vec<u8>,
    reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CentralTabId {
    Script(u64),
    DataViewer(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DataViewerTab {
    pub(crate) name: String,
    pub(crate) page_start: usize,
    pub(crate) page_size: usize,
    pub(crate) page: Option<crate::r_session::DataPage>,
    pub(crate) error: Option<String>,
    pub(crate) filter: String,
    pub(crate) sort_column: Option<usize>,
    pub(crate) sort_ascending: bool,
    pub(crate) loading: bool,
}

pub struct Workspace {
    pub enabled: bool,
    pub(crate) tab_order: Vec<CentralTabId>,
    pub(crate) viewer_tabs: Vec<DataViewerTab>,
    pub(crate) active_viewer_tab: Option<String>,
    scripts: Vec<ScriptDocument>,
    active_script_id: u64,
    next_script_id: u64,
    pending_close_script_id: Option<u64>,
    pending_focus_script_id: Option<u64>,
    editor_widget_ids: HashMap<u64, egui::Id>,
    pub console_input: String,
    console_input_id: Option<egui::Id>,
    pending_console_focus: bool,
    pub console_output: String,
    console_pending_lines: Vec<String>,
    console_continuation: bool,
    console_scroll_at_bottom: bool,
    plots: Vec<crate::r_session::RObject>,
    objects: Vec<crate::r_session::RObject>,
    environment_filter: String,
    selected_object: Option<String>,
    selected_plot: Option<String>,
    last_selected_linked_panel: Option<u64>,
    source_origins: HashMap<String, SourceOrigin>,
    viewer: Option<(String, crate::r_session::DataPage)>,
    viewer_start: usize,
    #[allow(dead_code)]
    viewer_filter: String,
    #[allow(dead_code)]
    viewer_sort_column: Option<usize>,
    #[allow(dead_code)]
    viewer_sort_ascending: bool,
    confirm_environment_clear: bool,
    confirm_object_remove: Option<String>,
    selected_properties_panel: Option<u64>,
    properties_source: Option<(f64, f64)>,
    properties_width_mm: f64,
    properties_height_mm: f64,
    properties_aspect_lock: bool,
    properties_aspect_ratio: f64,
    graph_drafts: HashMap<(u64, u64), GraphStyle>,
    graph_pending: HashMap<(u64, u64), GraphRenderRequest>,
    render_epoch: HashMap<u64, u64>,
    observed_panel_sizes: HashMap<(u64, u64), (f64, f64)>,
    pending_mouse_resize: HashMap<(u64, u64), (f64, f64)>,
    linked_panels: Vec<RLinkedPanel>,
    pending_refresh: VecDeque<RLinkedPanel>,
    console_history: Vec<String>,
    history_cursor: Option<usize>,
    history_draft: Option<String>,
    pending_script_decode: Option<PendingScriptDecode>,
    history_filter: String,
    help_query: String,
    help_text: String,
    packages: Vec<crate::r_session::RPackage>,
    packages_loaded: bool,
    package_filter: String,
    delete_artwork_confirmation: Option<u64>,
    pending_unlink_edit: Option<PendingUnlinkEdit>,
    captured_rotation: Option<CapturedRotation>,
    confirming_unlink: bool,
    panel_relink_drafts: HashMap<u64, String>,
    panel_relink_open: Option<u64>,
    console_error: bool,
    checking_r: bool,
    availability_checked: bool,
    running: bool,
    pub(crate) execution_queue: VecDeque<QueuedRCommand>,
    pub(crate) next_queue_id: u64,
    r_status: String,
    _tx: Sender<RMessage>,
    _rx: Receiver<RMessage>,
    repaint_ctx: Option<egui::Context>,
    pub(crate) session: std::sync::Arc<std::sync::Mutex<crate::r_session::RSession>>,
    pub(crate) session_active: bool,
    pub(crate) completion: Option<ActiveCompletion>,
    pub(crate) completion_object_columns: HashMap<String, Vec<(String, String)>>,
    pub(crate) completion_namespace_cache: HashMap<(String, bool), Vec<crate::r_editor::CompletionCandidate>>,
    pub(crate) completion_request_in_flight: bool,
    pub(crate) working_directory: Option<std::path::PathBuf>,
    pub(crate) pending_rdata_import: Option<crate::r_session::RDataInspection>,
    pub(crate) project_path: Option<String>,
    pub(crate) project_dirty: bool,
    pub(crate) project_rdata_reference: Option<String>,
    pub(crate) panel_states: HashMap<u64, LinkedPanelState>,
    pub(crate) last_project_parent: Option<std::path::PathBuf>,
    pub(crate) save_r_workspace_preference: Option<bool>,
}

/// Runtime association between a persistent document group ID and its R variable name.
/// The group ID is allocated by the document and remains independent of the mutable variable name.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RLinkedPanel {
    pub(crate) panel_id: u64,
    pub(crate) generated_id: u64,
    pub(crate) label: String,
    pub(crate) r_variable: String,
    pub(crate) width_mm: f64,
    pub(crate) height_mm: f64,
    pub(crate) resize_mode: ResizeMode,
    pub(crate) graph_style: GraphStyle,
    pub(crate) small_figure_typography: bool,
    pub(crate) document_uid: u64,
}

impl Default for Workspace {
    fn default() -> Self {
        Self::studio_default()
    }
}

impl Workspace {
    pub(crate) fn has_linked_panels(&self, document_uid: u64) -> bool {
        self.linked_panels.iter().any(|panel| panel.document_uid == document_uid)
    }

    pub(crate) fn panel_connection_state(&self, panel: &RLinkedPanel) -> LinkedPanelState {
        if self.running
            && (self.graph_pending.contains_key(&(panel.document_uid, panel.panel_id))
                || self.pending_refresh.iter().any(|p| p.panel_id == panel.panel_id))
        {
            return LinkedPanelState::Updating;
        }
        if let Some(&state) = self.panel_states.get(&panel.panel_id) {
            if state == LinkedPanelState::Updating {
                return LinkedPanelState::Updating;
            }
            if state == LinkedPanelState::Error {
                return LinkedPanelState::Error;
            }
            if state == LinkedPanelState::Connected {
                if !self.session_active {
                    return LinkedPanelState::Offline;
                }
                let obj = self.objects.iter().find(|o| o.variable == panel.r_variable);
                match obj {
                    None => return LinkedPanelState::MissingObject,
                    Some(o) if o.plot_kind.is_empty() => return LinkedPanelState::IncompatibleObject,
                    Some(_) => return LinkedPanelState::Connected,
                }
            }
        }
        if !self.session_active {
            return LinkedPanelState::Offline;
        }
        let obj = self.objects.iter().find(|o| o.variable == panel.r_variable);
        match obj {
            None => LinkedPanelState::MissingObject,
            Some(o) if o.plot_kind.is_empty() => LinkedPanelState::IncompatibleObject,
            Some(_) => LinkedPanelState::ReadyToUpdate,
        }
    }

    pub(crate) fn resolve_project_rdata_path(&self) -> Option<std::path::PathBuf> {
        let rdata_ref = self.project_rdata_reference.as_deref()?;
        let path = std::path::Path::new(rdata_ref);
        if path.is_absolute() {
            Some(path.to_path_buf())
        } else if let Some(project_path) = self.project_path.as_deref() {
            let p = std::path::Path::new(project_path);
            let base = if p.is_dir() {
                p
            } else if p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("fide")) {
                p.parent().unwrap_or(p)
            } else {
                p
            };
            Some(base.join(path))
        } else {
            Some(path.to_path_buf())
        }
    }

    pub(crate) fn studio_default() -> Self {
        let (tx, rx) = mpsc::channel();
        let initial_script = ScriptDocument::new(1, "# One shared R script can define multiple named plot objects.\n".into(), None, None);
        Self {
            enabled: true,
            tab_order: vec![CentralTabId::Script(1)],
            viewer_tabs: Vec::new(),
            active_viewer_tab: None,
            scripts: vec![initial_script],
            active_script_id: 1,
            next_script_id: 2,
            pending_close_script_id: None,
            pending_focus_script_id: None,
            editor_widget_ids: HashMap::new(),
            console_input: String::new(),
            console_input_id: None,
            pending_console_focus: false,
            console_output: String::new(),
            console_pending_lines: Vec::new(),
            console_continuation: false,
            console_scroll_at_bottom: true,
            plots: Vec::new(),
            objects: Vec::new(),
            environment_filter: String::new(),
            selected_object: None,
            selected_plot: None,
            last_selected_linked_panel: None,
            source_origins: HashMap::new(),
            viewer: None,
            viewer_start: 0,
            viewer_filter: String::new(),
            viewer_sort_column: None,
            viewer_sort_ascending: true,
            confirm_environment_clear: false,
            confirm_object_remove: None,
            selected_properties_panel: None,
            properties_source: None,
            properties_width_mm: DEFAULT_PLOT_WIDTH_MM,
            properties_height_mm: DEFAULT_PLOT_HEIGHT_MM,
            properties_aspect_lock: false,
            properties_aspect_ratio: DEFAULT_PLOT_WIDTH_MM / DEFAULT_PLOT_HEIGHT_MM,
            graph_drafts: HashMap::new(),
            graph_pending: HashMap::new(),
            render_epoch: HashMap::new(),
            observed_panel_sizes: HashMap::new(),
            pending_mouse_resize: HashMap::new(),
            linked_panels: Vec::new(),
            pending_refresh: VecDeque::new(),
            console_history: Vec::new(),
            history_cursor: None,
            history_draft: None,
            pending_script_decode: None,
            history_filter: String::new(),
            help_query: String::new(),
            help_text: String::new(),
            packages: Vec::new(),
            packages_loaded: false,
            package_filter: String::new(),
            delete_artwork_confirmation: None,
            pending_unlink_edit: None,
            captured_rotation: None,
            confirming_unlink: false,
            panel_relink_drafts: HashMap::new(),
            panel_relink_open: None,
            console_error: false,
            checking_r: false,
            availability_checked: false,
            running: false,
            execution_queue: VecDeque::new(),
            next_queue_id: 1,
            r_status: "Checking for R...".into(),
            _tx: tx,
            _rx: rx,
            repaint_ctx: None,
            session: std::sync::Arc::new(std::sync::Mutex::new(crate::r_session::RSession::default())),
            session_active: false,
            completion: None,
            completion_object_columns: HashMap::new(),
            completion_namespace_cache: HashMap::new(),
            completion_request_in_flight: false,
            working_directory: std::env::current_dir().ok(),
            pending_rdata_import: None,
            project_path: None,
            project_dirty: false,
            project_rdata_reference: None,
            panel_states: HashMap::new(),
            last_project_parent: None,
            save_r_workspace_preference: None,
        }
    }

    pub(crate) fn app_default() -> Self {
        let mut workspace = Self::studio_default();
        // Legacy UI tests exercise the original docking layout; the dedicated studio test
        // enables this layout explicitly. Production builds always start in R Studio.
        if cfg!(test) {
            workspace.enabled = false;
        }
        workspace
    }

    fn active_script_index(&self) -> Option<usize> {
        self.scripts.iter().position(|script| script.id == self.active_script_id)
    }

    fn active_script(&self) -> Option<&ScriptDocument> {
        self.scripts.iter().find(|script| script.id == self.active_script_id)
    }

    fn active_script_mut(&mut self) -> Option<&mut ScriptDocument> {
        self.scripts.iter_mut().find(|script| script.id == self.active_script_id)
    }

    fn new_script(&mut self) -> u64 {
        self.completion = None;
        let id = self.next_script_id;
        self.next_script_id = self.next_script_id.saturating_add(1).max(1);
        let mut script = ScriptDocument::new(id, String::new(), None, None);
        // A wrapped counter is exceptionally unlikely; avoid colliding with an open document.
        if self.scripts.iter().any(|open| open.id == id) {
            script.id = self.scripts.iter().map(|open| open.id).max().unwrap_or(0).saturating_add(1);
            self.next_script_id = script.id.saturating_add(1).max(1);
        }
        self.active_script_id = script.id;
        self.pending_focus_script_id = Some(script.id);
        self.scripts.push(script);
        self.active_viewer_tab = None;
        self.tab_order.push(CentralTabId::Script(self.active_script_id));
        self.sync_tab_order();
        self.active_script_id
    }

    fn activate_script(&mut self, id: u64) -> bool {
        self.completion = None;
        if self.scripts.iter().any(|script| script.id == id) {
            self.active_script_id = id;
            self.pending_focus_script_id = Some(id);
            self.active_viewer_tab = None;
            self.sync_tab_order();
            true
        } else {
            false
        }
    }

    fn cycle_script(&mut self, direction: isize) {
        self.completion = None;
        let Some(index) = self.active_script_index() else { return };
        if self.scripts.is_empty() {
            return;
        }
        let count = self.scripts.len() as isize;
        let next = (index as isize + direction).rem_euclid(count) as usize;
        if let Some(script) = self.scripts.get(next) {
            self.active_script_id = script.id;
            self.pending_focus_script_id = Some(script.id);
        }
    }

    fn close_script_now(&mut self, id: u64) {
        self.completion = None;
        let Some(index) = self.scripts.iter().position(|script| script.id == id) else { return };
        self.scripts.remove(index);
        self.editor_widget_ids.remove(&id);
        self.tab_order.retain(|t| *t != CentralTabId::Script(id));
        if self.scripts.is_empty() {
            self.new_script();
        } else if self.active_script_id == id
            && let Some(next) = self.scripts.get(index.min(self.scripts.len() - 1)).or_else(|| self.scripts.first()).map(|script| script.id)
        {
            self.active_script_id = next;
            self.pending_focus_script_id = Some(next);
        }
        if self.pending_close_script_id == Some(id) {
            self.pending_close_script_id = None;
        }
        self.sync_tab_order();
    }

    pub(crate) fn sync_tab_order(&mut self) {
        for script in &self.scripts {
            let tab = CentralTabId::Script(script.id);
            if !self.tab_order.contains(&tab) {
                self.tab_order.push(tab);
            }
        }
        for viewer in &self.viewer_tabs {
            let tab = CentralTabId::DataViewer(viewer.name.clone());
            if !self.tab_order.contains(&tab) {
                self.tab_order.push(tab);
            }
        }
        self.tab_order.retain(|tab| match tab {
            CentralTabId::Script(id) => self.scripts.iter().any(|s| s.id == *id),
            CentralTabId::DataViewer(name) => self.viewer_tabs.iter().any(|v| &v.name == name),
        });
        if let Some(active_name) = &self.active_viewer_tab {
            if !self.viewer_tabs.iter().any(|v| &v.name == active_name) {
                self.active_viewer_tab = None;
            }
        }
    }

    pub(crate) fn open_data_viewer(&mut self, name: &str) {
        let name_owned = name.to_owned();
        self.selected_object = Some(name_owned.clone());
        if self.viewer_tabs.iter().any(|t| t.name == name_owned) {
            self.active_viewer_tab = Some(name_owned);
            self.sync_tab_order();
            return;
        }
        let tab = DataViewerTab {
            name: name_owned.clone(),
            page_start: 0,
            page_size: 50,
            page: None,
            error: None,
            filter: String::new(),
            sort_column: None,
            sort_ascending: true,
            loading: true,
        };
        self.viewer_tabs.push(tab);
        self.tab_order.push(CentralTabId::DataViewer(name_owned.clone()));
        self.active_viewer_tab = Some(name_owned.clone());
        self.sync_tab_order();
        request_data_page(self, name_owned, 0);
    }

    pub(crate) fn close_data_viewer(&mut self, name: &str) {
        let was_active = self.active_viewer_tab.as_deref() == Some(name);
        let tab_idx = self.tab_order.iter().position(|t| match t {
            CentralTabId::DataViewer(n) => n == name,
            _ => false,
        });
        self.tab_order.retain(|t| match t {
            CentralTabId::DataViewer(n) => n != name,
            _ => true,
        });
        self.viewer_tabs.retain(|t| t.name != name);
        if was_active {
            if let Some(idx) = tab_idx {
                let fallback = if self.tab_order.is_empty() {
                    None
                } else if idx < self.tab_order.len() {
                    Some(self.tab_order[idx].clone())
                } else {
                    self.tab_order.last().cloned()
                };
                match fallback {
                    Some(CentralTabId::DataViewer(next_name)) => {
                        self.active_viewer_tab = Some(next_name);
                    }
                    Some(CentralTabId::Script(next_script_id)) => {
                        self.active_viewer_tab = None;
                        self.activate_script(next_script_id);
                    }
                    None => {
                        self.active_viewer_tab = None;
                        if self.scripts.is_empty() {
                            self.new_script();
                        }
                    }
                }
            } else {
                self.active_viewer_tab = None;
            }
        }
        self.sync_tab_order();
    }
}

/// Read-only state for desktop verification through the existing control channel.
pub(crate) fn inspect_workspace(app: &crate::VectorcraftApp, ctx: &egui::Context) -> serde_json::Value {
    let layout = ctx.data(|d| d.get_temp::<WorkspaceGeometry>(workspace_geometry_id())).unwrap_or_default();
    let bounds = |r: Option<egui::Rect>| r.map(|r| [r.left(), r.top(), r.width(), r.height()]);
    let rect = |id: Option<egui::Id>| {
        id.and_then(|id| ctx.read_response(id)).map(|response| {
            let r = response.interact_rect;
            serde_json::json!([r.left(), r.top(), r.width(), r.height()])
        })
    };
    let interaction = serde_json::json!({
        "pointerPos": ctx.input(|i| i.pointer.latest_pos().map(|p| [p.x,p.y])),
        "pointerDown": ctx.input(|i| i.pointer.primary_down()),
        "draggedWidget": ctx.dragged_id().map(|id|format!("{id:?}")),
        "draggedWidgetRect": rect(ctx.dragged_id()),
        "editorDividerHovered": layout.editor_divider_id.and_then(|id| ctx.read_response(id)).is_some_and(|r|r.hovered()),
        "editorDividerDragged": layout.editor_divider_id.and_then(|id| ctx.read_response(id)).is_some_and(|r|r.dragged()),
        "workspaceDividerFocused": layout.divider_id.is_some_and(|id|ctx.memory(|m|m.has_focus(id))),
        "editorDividerFocused": layout.editor_divider_id.is_some_and(|id|ctx.memory(|m|m.has_focus(id))),
    });
    serde_json::json!({
        "enabled": app.scientific.enabled,
        "tab": app.ui.r_studio_tab,
        "width": app.ui.r_studio_workspace_width,
        "editorFraction": app.ui.r_studio_editor_fraction,
        "editorMaximized": app.ui.r_studio_editor_maximized,
        "toolbarRect": bounds(layout.toolbar),
        "workspaceRect": bounds(layout.workspace),
        "vectorRect": bounds(layout.vector),
        "toolsRect": bounds(layout.tools),
        "workspaceDividerRect": bounds(layout.divider),
        "editorDividerRect": bounds(layout.editor_divider),
        "interaction": interaction,
        "pixelsPerPoint": ctx.pixels_per_point(),
        "running": app.scientific.running,
        "sessionActive": app.scientific.session_active,
        "status": app.scientific.r_status,
        "consoleError": app.scientific.console_error,
        "consoleOutput": app.scientific.console_output,
        "scriptBytes": app.scientific.active_script().map(|script| script.text.len()),
        "activeScriptId": app.scientific.active_script_id,
        "scripts": app.scientific.scripts.iter().map(|script| serde_json::json!({
            "id": script.id, "filename": script.filename(), "dirty": script.is_dirty(),
        })).collect::<Vec<_>>(),
        "editorFocused": app.scientific.editor_widget_ids.get(&app.scientific.active_script_id)
            .is_some_and(|id| ctx.memory(|memory| memory.focused()) == Some(*id)),
        "editorCursorChar": app.scientific.editor_widget_ids.get(&app.scientific.active_script_id)
            .and_then(|id| egui::TextEdit::load_state(ctx, *id))
            .and_then(|state| state.cursor.char_range())
            .map(|range| usize::from(range.primary.index)),
        "consoleFocused": app.scientific.console_input_id.is_some_and(|id| ctx.memory(|memory| memory.focused()) == Some(id)),
        "consoleInput": app.scientific.console_input,
        "consoleScrollAtBottom": app.scientific.console_scroll_at_bottom,
        "selectedPlot": app.scientific.selected_plot,
        "selectedObject": app.scientific.selected_object,
        "editorRect": rect(app.scientific.editor_widget_ids.get(&app.scientific.active_script_id).copied()),
        "consoleInputRect": rect(app.scientific.console_input_id),
        "objects": app.scientific.objects.iter().map(|object| serde_json::json!({
            "variable": object.variable, "classes": object.classes, "type": object.data_type,
            "dimensions": object.dimensions, "preview": object.preview,
            "category": object.category, "description": object.description, "plotKind": object.plot_kind,
        })).collect::<Vec<_>>(),
        "plots": app.scientific.plots.iter().map(|plot| &plot.variable).collect::<Vec<_>>(),
        "pendingRDataImport": app.scientific.pending_rdata_import.as_ref().map(|pending| serde_json::json!({
            "path": pending.file_path,
            "totalObjects": pending.total_objects,
            "objectNames": pending.object_names,
            "collisions": pending.collisions,
        })),
        "linkedPanels": app.scientific.linked_panels.iter().map(|panel| serde_json::json!({
            "id": panel.panel_id, "variable": panel.r_variable,
        })).collect::<Vec<_>>(),
    })
}

/// Configure presentation only; execution continues through the real editor/Console UI.
pub(crate) fn configure_workspace(app: &mut crate::VectorcraftApp, params: &serde_json::Value) -> Result<(), String> {
    let number = |key: &str, min: f64, max: f64| -> Result<Option<f32>, String> {
        params
            .get(key)
            .map(|value| {
                let value = value
                    .as_f64()
                    .filter(|value| value.is_finite() && (min..=max).contains(value))
                    .ok_or_else(|| format!("`{key}` must be between {min} and {max}"))?;
                Ok(value as f32)
            })
            .transpose()
    };
    let width = number("rWorkspaceWidth", 240.0, 1600.0)?;
    let fraction = number("rEditorFraction", 0.25, 0.85)?;
    let tab = params
        .get("rTab")
        .map(|value| {
            let key = value.as_str().ok_or("`rTab` must be a tab key")?;
            if is_graph_tab_request(key) {
                Ok(None)
            } else {
                BottomTab::ALL.into_iter().find(|tab| tab.key() == key).map(Some).ok_or("unknown `rTab`")
            }
        })
        .transpose()?
        .flatten();
    let graph_requested = params.get("rTab").and_then(serde_json::Value::as_str).is_some_and(is_graph_tab_request);
    let enabled = params.get("rWorkspace").map(|value| value.as_bool().ok_or("`rWorkspace` must be true or false")).transpose()?;
    if let Some(width) = width {
        app.ui.r_studio_workspace_width = width;
    }
    if let Some(fraction) = fraction {
        app.ui.r_studio_editor_fraction = fraction;
    }
    if let Some(tab) = tab {
        app.ui.r_studio_tab = tab.key().into();
    }
    if let Some(enabled) = enabled {
        app.scientific.enabled = enabled;
    }
    if graph_requested {
        app.ui.r_studio_tab = BottomTab::Console.key().into();
        focus_graph_tab(app);
    }
    Ok(())
}

fn consume_key_no_repeat(input: &mut egui::InputState, modifiers: egui::Modifiers, key: egui::Key) -> bool {
    let mut triggered = false;
    input.events.retain(|event| {
        if let egui::Event::Key { key: k, pressed: true, repeat, modifiers: m, .. } = event
            && *k == key
            && m.matches_exact(modifiers)
        {
            if !*repeat && !triggered {
                triggered = true;
            }
            return false;
        }
        true
    });
    triggered
}

/// R source shortcuts take precedence over canvas commands only while the active R editor owns focus.
pub(crate) fn handle_shortcuts(app: &mut crate::VectorcraftApp, ctx: &egui::Context) -> bool {
    if !app.scientific.enabled {
        return false;
    }
    poll_messages(app);
    let layout = ctx.data(|d| d.get_temp::<WorkspaceGeometry>(workspace_geometry_id())).unwrap_or_default();
    let focused = ctx.memory(|m| m.focused());
    if focused.is_some() && (focused == layout.divider_id || focused == layout.editor_divider_id) {
        // Divider arrows belong to the workspace UI, never to canvas nudging/tools.
        return true;
    }
    if app.scientific.console_input_id.is_some_and(|id| ctx.memory(|memory| memory.focused()) == Some(id)) {
        // The focused Console owns text-editor keys; do not route them to canvas tools/commands.
        return true;
    }
    let active_id = app.scientific.active_script_id;
    let Some(editor_id) = app.scientific.editor_widget_ids.get(&active_id).copied() else { return false };
    if ctx.memory(|memory| memory.focused()) != Some(editor_id) {
        return false;
    }
    let (new_script, open_script_shortcut, save_as, save, close, previous, next, source, run) = ctx.input_mut(|input| {
        let command = egui::Modifiers::COMMAND;
        let command_shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
        (
            input.consume_key(command, egui::Key::N),
            input.consume_key(command, egui::Key::O),
            input.consume_key(command_shift, egui::Key::S),
            input.consume_key(command, egui::Key::S),
            input.consume_key(command, egui::Key::W),
            input.consume_key(command_shift, egui::Key::Tab),
            input.consume_key(command, egui::Key::Tab),
            consume_key_no_repeat(input, command_shift, egui::Key::Enter),
            consume_key_no_repeat(input, command, egui::Key::Enter),
        )
    });
    if !(new_script || open_script_shortcut || save_as || save || close || previous || next || source || run) {
        return false;
    }
    if app.scientific.pending_close_script_id.is_some() {
        return true;
    }
    if new_script {
        app.scientific.new_script();
    } else if open_script_shortcut {
        crate::picks::button(app, open_script);
    } else if save_as {
        let id = app.scientific.active_script_id;
        crate::picks::button(app, move |app| save_script_for(app, id, true, false));
    } else if save {
        let id = app.scientific.active_script_id;
        crate::picks::button(app, move |app| save_script_for(app, id, false, false));
    } else if close {
        request_close_script(app, active_id);
    } else if previous {
        app.scientific.cycle_script(-1);
    } else if next {
        app.scientific.cycle_script(1);
    } else if source {
        app.scientific.completion = None;
        source_active_script(&mut app.scientific);
    } else if run {
        app.scientific.completion = None;
        run_selection_or_current_expression(&mut app.scientific);
    }
    true
}

#[derive(Clone, Default)]
struct WorkspaceGeometry {
    toolbar: Option<egui::Rect>,
    workspace: Option<egui::Rect>,
    vector: Option<egui::Rect>,
    tools: Option<egui::Rect>,
    divider: Option<egui::Rect>,
    editor_divider: Option<egui::Rect>,
    divider_id: Option<egui::Id>,
    editor_divider_id: Option<egui::Id>,
}

fn run_selection_or_current_expression(workspace: &mut Workspace) {
    let id = workspace.active_script_id;
    let editor_id = workspace.editor_widget_ids.get(&id).copied().unwrap_or_else(|| egui::Id::unique("scientific-r-script-editor").with(id));
    let selected = workspace.repaint_ctx.as_ref().and_then(|ctx| {
        let script = workspace.active_script()?.text.as_str();
        selected_code(script, ctx, editor_id)
    });
    if let Some((code, end_cursor)) = selected {
        navigate_source(workspace, id, end_cursor);
        let command = QueuedRCommand {
            id: workspace.next_queue_id,
            script_id: Some(id),
            code,
            source_revision: workspace.active_script().map(|s| s.text.clone()),
            start_line: None,
            refresh_linked: false,
            is_selection: true,
        };
        workspace.next_queue_id = workspace.next_queue_id.wrapping_add(1).max(1);
        enqueue_or_execute(workspace, command);
    } else {
        run_current_expression(workspace, editor_id);
    }
}

pub(crate) fn run_command(app: &mut crate::VectorcraftApp, id: &str, p: &serde_json::Value) -> Option<Result<serde_json::Value, String>> {
    use serde_json::{Value, json};

    if id.starts_with("r.") || id.starts_with("window.r") {
        poll_messages(app);
    }
    let result = match id {
        "r.script.new" => {
            app.scientific.enabled = true;
            let script_id = app.scientific.new_script();
            Ok(json!({ "id": script_id }))
        }
        "r.script.open" => {
            app.scientific.enabled = true;
            if let Some(path) = p.get("path").and_then(Value::as_str).filter(|path| !path.is_empty()) {
                open_script_path(app, path);
            } else {
                open_script(app);
            }
            Ok(Value::Null)
        }
        "r.script.save" => {
            let script_id = p.get("id").and_then(Value::as_u64).unwrap_or(app.scientific.active_script_id);
            save_script_for(app, script_id, false, false);
            Ok(Value::Null)
        }
        "r.script.saveAs" => {
            let script_id = p.get("id").and_then(Value::as_u64).unwrap_or(app.scientific.active_script_id);
            save_script_for(app, script_id, true, false);
            Ok(Value::Null)
        }
        "r.script.close" => {
            if let Some(name) = app.scientific.active_viewer_tab.clone() {
                app.scientific.close_data_viewer(&name);
            } else {
                let script_id = p.get("id").and_then(Value::as_u64).unwrap_or(app.scientific.active_script_id);
                request_close_script(app, script_id);
            }
            Ok(Value::Null)
        }
        "r.run" => {
            if app.scientific.active_viewer_tab.is_some() {
                return Some(Ok(Value::Null));
            }
            if app.scientific.execution_queue.len() >= MAX_QUEUE_CAPACITY {
                return Some(Err("R execution queue is full".into()));
            }
            app.scientific.enabled = true;
            if app.scientific.scripts.is_empty() {
                app.scientific.new_script();
            }
            run_selection_or_current_expression(&mut app.scientific);
            Ok(Value::Null)
        }
        "r.source" => {
            if app.scientific.active_viewer_tab.is_some() {
                return Some(Ok(Value::Null));
            }
            if app.scientific.running {
                return Some(Err("R is already running".into()));
            }
            app.scientific.enabled = true;
            if app.scientific.scripts.is_empty() {
                app.scientific.new_script();
            }
            source_active_script(&mut app.scientific);
            Ok(Value::Null)
        }
        "r.plots.updateSelected" => {
            if app.scientific.running {
                return Some(Err("R is already running".into()));
            }
            update_selected_panel(app);
            Ok(Value::Null)
        }
        "r.plots.updateAll" => {
            if app.scientific.running {
                return Some(Err("R is already running".into()));
            }
            update_all_panels(app);
            Ok(Value::Null)
        }
        "r.session.restart" => {
            if app.scientific.running {
                return Some(Err("R is already running".into()));
            }
            control_r_session(&mut app.scientific, true);
            Ok(Value::Null)
        }
        "r.session.stop" => {
            if !app.scientific.session_active || app.scientific.running {
                return Some(Err("No idle R session to stop".into()));
            }
            control_r_session(&mut app.scientific, false);
            Ok(Value::Null)
        }
        "r.console.clear" => {
            clear_console_transcript(&mut app.scientific);
            Ok(Value::Null)
        }
        "r.environment.importRData" => {
            app.scientific.enabled = true;
            let path_opt = p.get("path").and_then(Value::as_str).filter(|path| !path.is_empty());
            let policy_opt = p.get("policy").and_then(Value::as_str).and_then(|pol| match pol.to_lowercase().as_str() {
                "overwrite" => Some(crate::r_session::ImportCollisionPolicy::Overwrite),
                "skip" => Some(crate::r_session::ImportCollisionPolicy::Skip),
                _ => None,
            });
            if let Some(path) = path_opt {
                import_rdata_path(app, path, policy_opt);
            } else {
                import_rdata_dialog(app);
            }
            Ok(Value::Null)
        }
        "r.environment.resolveImport" => {
            let policy_str = p.get("policy").and_then(Value::as_str).unwrap_or("overwrite");
            match policy_str.to_lowercase().as_str() {
                "overwrite" => resolve_rdata_import(app, crate::r_session::ImportCollisionPolicy::Overwrite),
                "skip" => resolve_rdata_import(app, crate::r_session::ImportCollisionPolicy::Skip),
                "cancel" => cancel_rdata_import(&mut app.scientific),
                other => return Some(Err(format!("Unknown resolve import policy: {other}"))),
            }
            Ok(Value::Null)
        }
        "window.rWorkspace" => crate::menus::opt_bool(p, "on").map(|on| {
            let on = on.unwrap_or(!app.scientific.enabled);
            app.scientific.enabled = on;
            json!(on)
        }),
        "window.rEditorMaximize" => crate::menus::opt_bool(p, "on").map(|on| {
            let on = on.unwrap_or(!app.ui.r_studio_editor_maximized);
            app.ui.r_studio_editor_maximized = on;
            json!(on)
        }),
        "window.rResetPanes" => {
            app.ui.r_studio_workspace_width = 420.0;
            app.ui.r_studio_editor_fraction = 0.62;
            app.ui.r_studio_editor_maximized = false;
            Ok(Value::Null)
        }
        "window.rTab" => {
            let Some(key) = p.get("tab").and_then(Value::as_str) else {
                return Some(Err("`tab` is required".into()));
            };
            if is_graph_tab_request(key) {
                app.ui.r_studio_tab = BottomTab::Console.key().into();
                focus_graph_tab(app);
                return Some(Ok(json!({"tab": "graph"})));
            }
            let Some(tab) = BottomTab::ALL.into_iter().find(|tab| tab.key() == key) else {
                return Some(Err(format!("unknown R tab `{key}`")));
            };
            app.scientific.enabled = true;
            app.ui.r_studio_tab = tab.key().to_owned();
            if tab == BottomTab::Console {
                app.scientific.console_error = false;
                app.scientific.console_scroll_at_bottom = true;
                app.scientific.pending_console_focus = true;
            }
            Ok(json!({ "tab": tab.key() }))
        }
        "project.new" => project::new_project_command(app, p),
        "project.open" => project::open_project_command(app, p),
        "project.save" => project::save_project_command(app, p),
        "project.saveAs" => project::save_project_as_command(app, p),
        "figure.save" => project::save_figure_command(app, p),
        "figure.saveAs" => project::save_figure_as_command(app, p),
        "r.workspace.save" => project::save_r_workspace_command(app, p),
        "r.workspace.load" => project::load_r_workspace_command(app, p),
        "figure.export.svg" | "figure.exportSvg" => export::export_figure_svg_command(app, p),
        "figure.export.pdf" | "figure.exportPdf" => export::export_figure_pdf_command(app, p),
        "r.environment.diagnostics" | "r.diagnostics" => diagnostics::run_diagnostics_command(app, p),
        _ => return None,
    };
    Some(result)
}

pub(crate) fn command_enabled(app: &crate::VectorcraftApp, id: &str) -> Option<bool> {
    Some(match id {
        "project.new" | "project.open" | "project.saveAs" => true,
        "project.save" => project::is_project_dirty(app) || app.scientific.project_path.is_none(),
        "figure.save" | "figure.saveAs" => app.session.active().is_some(),
        "figure.export.svg" | "figure.exportSvg" | "figure.export.pdf" | "figure.exportPdf" => app.session.active().is_some(),
        "r.workspace.save" | "r.workspace.load" => !app.scientific.running,
        "r.environment.diagnostics" | "r.diagnostics" => true,
        "r.script.new"
        | "r.script.open"
        | "r.console.clear"
        | "window.rWorkspace"
        | "window.rEditorMaximize"
        | "window.rResetPanes"
        | "window.rTab" => true,
        "r.script.save" | "r.script.saveAs" => app.scientific.active_viewer_tab.is_none() && !app.scientific.scripts.is_empty(),
        "r.script.close" => app.scientific.active_viewer_tab.is_some() || !app.scientific.scripts.is_empty(),
        "r.run" => app.scientific.active_viewer_tab.is_none() && app.scientific.execution_queue.len() < MAX_QUEUE_CAPACITY,
        "r.source" => app.scientific.active_viewer_tab.is_none() && !app.scientific.running,
        "r.plots.updateSelected" | "r.plots.updateAll" | "r.session.restart" | "r.environment.importRData" => !app.scientific.running,
        "r.environment.resolveImport" => !app.scientific.running && app.scientific.pending_rdata_import.is_some(),
        "r.session.stop" => app.scientific.session_active && !app.scientific.running,
        _ => return None,
    })
}

pub(crate) fn command_checked(app: &crate::VectorcraftApp, id: &str, p: &serde_json::Value) -> Option<bool> {
    Some(match id {
        "window.rWorkspace" => app.scientific.enabled,
        "window.rEditorMaximize" => app.ui.r_studio_editor_maximized,
        "window.rTab" => {
            let key = p.get("tab").and_then(serde_json::Value::as_str).unwrap_or("");
            if is_graph_tab_request(key) {
                app.scientific.enabled && crate::panel_docking::showing(&app.ui, "graph")
            } else {
                app.scientific.enabled && BottomTab::from_key(&app.ui.r_studio_tab).key() == key
            }
        }
        _ => return None,
    })
}

// Shared locally by Source and Console; the global application theme is unchanged.
const R_CODE_SIZE: f32 = 13.0;
const R_CONSOLE_INPUT_SIZE: f32 = 13.5;
const R_CODE_PADDING: i8 = 8;
const R_CODE_LINE_GAP: f32 = 2.0;

const R_PANEL_ENTRY_PADDING: egui::Margin = egui::Margin::symmetric(6, 4);
const R_PANEL_ITEM_GAP: f32 = 2.0;
const R_PANEL_ENTRY_GAP: f32 = 4.0;
#[cfg(test)]
const R_PANEL_CLASS_CHARS: usize = 28;

fn compact_panel_style(ui: &mut egui::Ui) {
    let spacing = ui.spacing_mut();
    spacing.item_spacing = egui::vec2(6.0, R_PANEL_ITEM_GAP);
    spacing.button_padding = egui::vec2(5.0, 1.0);
    spacing.interact_size.y = 20.0;
}

fn panel_caption(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    let tokens = crate::theme::Tokens::get(ui.ctx());
    ui.add(egui::Label::new(egui::RichText::new(text).size(tokens.font_size - 1.0).color(tokens.text_dim)).wrap())
}

fn panel_action(ui: &mut egui::Ui, text: &str, enabled: bool, destructive: bool) -> egui::Response {
    let tokens = crate::theme::Tokens::get(ui.ctx());
    let color = if destructive { tokens.error } else { tokens.text_strong };
    ui.add_enabled(
        enabled,
        egui::Button::new(egui::RichText::new(text).size(tokens.font_size - 1.0).color(color)).small().min_size(egui::vec2(0.0, 20.0)),
    )
}

fn panel_entry(ui: &mut egui::Ui, selected: bool, contents: impl FnOnce(&mut egui::Ui)) {
    let tokens = crate::theme::Tokens::get(ui.ctx());
    egui::Frame::new()
        .inner_margin(R_PANEL_ENTRY_PADDING)
        .corner_radius(tokens.radius)
        .fill(if selected { tokens.row_selected } else { tokens.panel })
        .stroke(if selected { egui::Stroke::new(1.0, tokens.accent) } else { egui::Stroke::NONE })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            contents(ui);
        });
    ui.add_space(R_PANEL_ENTRY_GAP);
}

fn panel_name(ui: &mut egui::Ui, name: &str, selected: bool) -> egui::Response {
    let tokens = crate::theme::Tokens::get(ui.ctx());
    ui.add(egui::Button::selectable(selected, egui::RichText::new(name).font(crate::theme::semibold(tokens.font_size))).frame(false).wrap())
        .on_hover_text(name)
}

fn submit_console(workspace: &mut Workspace) {
    let expression = std::mem::take(&mut workspace.console_input);
    if workspace.running || (expression.trim().is_empty() && workspace.console_pending_lines.is_empty()) {
        return;
    }
    if !expression.trim().is_empty() || !workspace.console_pending_lines.is_empty() {
        let prompt = if workspace.console_continuation { "+ " } else { "> " };
        append_console_input(workspace, &expression, prompt);
        workspace.console_pending_lines.push(expression);
    }
    workspace.history_cursor = None;
    workspace.history_draft = None;
    let source = workspace.console_pending_lines.join("\n");
    workspace.console_error = false;
    workspace.running = true;
    workspace.r_status = "Checking R input...".into();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let session = workspace.session.clone();
        let tx = workspace._tx.clone();
        let ctx = workspace.repaint_ctx.clone();
        if let Err(error) = std::thread::Builder::new().name("r-console-parse".into()).spawn(move || {
            let result =
                session.lock().map_err(|error| format!("R session lock failed: {error}")).and_then(|mut session| session.parse_input(&source));
            let _ = tx.send(RMessage::ConsoleInputParsed(result));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        }) {
            workspace.running = false;
            append_console_output(workspace, &format!("Error: Could not start R parser: {error}\n"));
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        workspace.running = false;
        append_console_output(workspace, "Error: R execution is available in the native desktop app only\n");
    }
}

fn record_history(workspace: &mut Workspace, expression: &str) {
    if expression.trim().is_empty() {
        return;
    }
    workspace.console_history.push(expression.to_owned());
    if workspace.console_history.len() > 500 {
        workspace.console_history.remove(0);
    }
}

const CONSOLE_MAX_BYTES: usize = 1024 * 1024;

fn append_console_output(workspace: &mut Workspace, text: &str) {
    workspace.console_output.push_str(text);
    if workspace.console_output.len() > CONSOLE_MAX_BYTES + 64 * 1024 {
        let remove_before = workspace.console_output.len().saturating_sub(CONSOLE_MAX_BYTES);
        let boundary =
            workspace.console_output.get(remove_before..).and_then(|tail| tail.find('\n')).map_or(remove_before, |n| remove_before + n + 1);
        workspace.console_output.drain(..boundary);
        if !workspace.console_output.starts_with("Info: Earlier console output was trimmed.\n") {
            workspace.console_output.insert_str(0, "Info: Earlier console output was trimmed.\n");
        }
    }
}

fn append_console_input(workspace: &mut Workspace, code: &str, first_prompt: &str) {
    for (index, line) in code.lines().enumerate() {
        let prompt = if index == 0 { first_prompt } else { "+ " };
        append_console_output(workspace, &format!("{prompt}{line}\n"));
    }
}

fn append_console_code(workspace: &mut Workspace, code: &str) {
    append_console_input(workspace, code, "> ");
}

fn source_active_script(workspace: &mut Workspace) {
    if workspace.running {
        return;
    }
    let Some(script) = workspace.active_script() else {
        return;
    };
    let code = script.text.clone();
    let script_id = script.id;
    if code.trim().is_empty() {
        return;
    }
    let name = script.filename().to_owned();
    append_console_output(workspace, &format!("> # Source: {name}\n"));
    run_r_with_origin(workspace, code, true, Some(script_id));
}

fn history_previous(workspace: &mut Workspace) {
    if workspace.console_history.is_empty() {
        return;
    }
    let index = match workspace.history_cursor {
        Some(index) => index.saturating_sub(1),
        None => {
            workspace.history_draft = Some(workspace.console_input.clone());
            workspace.console_history.len().saturating_sub(1)
        }
    };
    workspace.history_cursor = Some(index);
    workspace.console_input = workspace.console_history.get(index).cloned().unwrap_or_default();
}

fn history_next(workspace: &mut Workspace) {
    let Some(index) = workspace.history_cursor else {
        return;
    };
    if index + 1 < workspace.console_history.len() {
        let next = index + 1;
        workspace.history_cursor = Some(next);
        workspace.console_input = workspace.console_history.get(next).cloned().unwrap_or_default();
    } else {
        workspace.history_cursor = None;
        workspace.console_input = workspace.history_draft.take().unwrap_or_default();
    }
}

#[cfg(test)]
fn object_metadata_summary(category: &str, classes: &[String], data_type: &str) -> String {
    let primary = if category == "Plot" || classes.iter().any(|class| class == "ggplot") {
        "ggplot"
    } else {
        classes.first().map_or(data_type, String::as_str)
    };
    let primary = if primary.chars().count() > R_PANEL_CLASS_CHARS {
        format!("{}...", primary.chars().take(R_PANEL_CLASS_CHARS - 3).collect::<String>())
    } else {
        primary.to_owned()
    };
    format!("{category} | {primary}")
}

fn environment_metadata_summary(object: &crate::r_session::RObject) -> String {
    object.description.clone()
}

fn environment_metadata_tooltip(object: &crate::r_session::RObject) -> String {
    format!(
        "{}\nCategory: {}\nDimensions: {}\nLength: {}\nPreview: {}",
        object_metadata_tooltip(&object.classes, &object.data_type),
        object.category,
        object.dimensions,
        object.length,
        object.preview
    )
}

fn object_metadata_tooltip(classes: &[String], data_type: &str) -> String {
    format!("Classes: {} | Type: {data_type}", classes.join("/"))
}

fn linked_panel_metadata(variable: &str, panel_id: u64) -> String {
    format!("{variable} | ID {panel_id}")
}

fn linked_panel_status(plot_exists: bool, object_exists: bool) -> &'static str {
    if plot_exists {
        "Connected"
    } else if object_exists {
        "R object is no longer a supported plot | artwork kept"
    } else {
        "Missing R variable | artwork kept"
    }
}

fn refresh_environment(workspace: &mut Workspace) {
    if workspace.running {
        return;
    }
    run_r(workspace, "invisible(NULL)".into(), false);
}

fn remove_environment_object(workspace: &mut Workspace, name: String) {
    environment_action(workspace, move |session| session.remove_object(&name), "remove");
}

fn clear_environment(workspace: &mut Workspace) {
    environment_action(workspace, crate::r_session::RSession::clear_environment, "clear");
}

fn environment_action(
    workspace: &mut Workspace,
    action: impl FnOnce(&mut crate::r_session::RSession) -> Result<crate::r_session::Execution, String> + Send + 'static,
    label: &'static str,
) {
    if workspace.running {
        return;
    }
    workspace.running = true;
    workspace.r_status = format!("{label}ing R environment...");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let session = workspace.session.clone();
        let tx = workspace._tx.clone();
        let ctx = workspace.repaint_ctx.clone();
        if let Err(error) = std::thread::Builder::new().name("r-environment-action".into()).spawn(move || {
            let result = session.lock().map_err(|error| format!("R session lock failed: {error}")).and_then(|mut session| action(&mut session));
            let _ = tx.send(RMessage::EnvironmentAction(result));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        }) {
            workspace.running = false;
            workspace.r_status = format!("Could not start environment operation: {error}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = action;
        workspace.running = false;
        workspace.r_status = "R execution is available in the native desktop app only".into();
    }
}

pub(crate) fn import_rdata_dialog(app: &mut crate::VectorcraftApp) {
    let pick = crate::FilePick { filters: vec![crate::FilePick::filter("R Data files", &["RData", "rdata", "rda", "RDA"])], ..Default::default() };
    let Some(path) = crate::picks::open(app, &pick) else { return };
    import_rdata_path(app, &path, None);
}

pub(crate) fn import_rdata_path(app: &mut crate::VectorcraftApp, path: &str, policy_override: Option<crate::r_session::ImportCollisionPolicy>) {
    if app.scientific.running {
        app.scientific.r_status = "R session is busy; wait for previous operation to complete".into();
        return;
    }
    app.scientific.running = true;
    app.scientific.r_status = format!("Inspecting RData file {path}...");
    let path = path.to_owned();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let session = app.scientific.session.clone();
        let tx = app.scientific._tx.clone();
        let ctx = app.scientific.repaint_ctx.clone();
        let target_path = path.clone();
        if let Err(error) = std::thread::Builder::new().name("r-inspect-rdata".into()).spawn(move || {
            let result =
                session.lock().map_err(|error| format!("R session lock failed: {error}")).and_then(|mut session| session.inspect_rdata(&target_path));
            let _ = tx.send(RMessage::RDataInspected { path: target_path, policy_override, inspection: result });
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        }) {
            app.scientific.running = false;
            app.scientific.r_status = format!("Could not start RData inspection: {error}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (path, policy_override);
        app.scientific.running = false;
        app.scientific.r_status = "RData import requires the native desktop R session".into();
    }
}

pub(crate) fn apply_rdata_import_async(app: &mut crate::VectorcraftApp, path: String, policy: crate::r_session::ImportCollisionPolicy) {
    if app.scientific.running {
        app.scientific.r_status = "R session is busy; wait for previous operation to complete".into();
        return;
    }
    app.scientific.running = true;
    let policy_label = match policy {
        crate::r_session::ImportCollisionPolicy::Overwrite => "Overwriting",
        crate::r_session::ImportCollisionPolicy::Skip => "Importing (skipping collisions)",
    };
    app.scientific.r_status = format!("{policy_label} from RData file...");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let session = app.scientific.session.clone();
        let tx = app.scientific._tx.clone();
        let ctx = app.scientific.repaint_ctx.clone();
        let target_path = path.clone();
        if let Err(error) = std::thread::Builder::new().name("r-apply-rdata".into()).spawn(move || {
            let result = session
                .lock()
                .map_err(|error| format!("R session lock failed: {error}"))
                .and_then(|mut session| session.apply_rdata_import(&target_path, policy));
            let _ = tx.send(RMessage::RDataImported { path: target_path, result });
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        }) {
            app.scientific.running = false;
            app.scientific.r_status = format!("Could not start RData import: {error}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (path, policy);
        app.scientific.running = false;
        app.scientific.r_status = "RData import requires the native desktop R session".into();
    }
}

pub(crate) fn resolve_rdata_import(app: &mut crate::VectorcraftApp, policy: crate::r_session::ImportCollisionPolicy) {
    if let Some(pending) = app.scientific.pending_rdata_import.take() {
        apply_rdata_import_async(app, pending.file_path, policy);
    }
}

pub(crate) fn cancel_rdata_import(workspace: &mut Workspace) {
    workspace.pending_rdata_import = None;
    workspace.r_status = "RData import cancelled".into();
}

fn request_data_page(workspace: &mut Workspace, name: String, start: usize) {
    if let Some(tab) = workspace.viewer_tabs.iter_mut().find(|t| t.name == name) {
        tab.loading = true;
        tab.error = None;
        tab.page_start = start;
    }
    workspace.viewer_start = start;
    if workspace.running {
        return;
    }
    workspace.running = true;
    let session = workspace.session.clone();
    let tx = workspace._tx.clone();
    let ctx = workspace.repaint_ctx.clone();
    let target_name = name.clone();
    #[cfg(not(target_arch = "wasm32"))]
    if let Err(error) = std::thread::Builder::new().name("r-data-page".into()).spawn(move || {
        let result = session
            .lock()
            .map_err(|error| format!("R session lock failed: {error}"))
            .and_then(|mut session| session.data_page(&target_name, start, 50));
        let _ = tx.send(RMessage::DataPage(target_name, result));
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    }) {
        workspace.running = false;
        workspace.r_status = format!("Could not start data viewer: {error}");
        if let Some(tab) = workspace.viewer_tabs.iter_mut().find(|t| t.name == name) {
            tab.loading = false;
            tab.error = Some(format!("Could not start data viewer: {error}"));
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (session, tx, ctx, name, start);
        workspace.running = false;
        if let Some(tab) = workspace.viewer_tabs.iter_mut().find(|t| t.name == name) {
            tab.loading = false;
            tab.error = Some("R execution is available in the native desktop app only".into());
        }
    }
}

fn request_help(workspace: &mut Workspace) {
    let topic = workspace.help_query.trim().to_owned();
    if topic.is_empty() || workspace.running {
        return;
    }
    workspace.running = true;
    workspace.r_status = format!("Looking up R help for {topic}...");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let session = workspace.session.clone();
        let tx = workspace._tx.clone();
        let ctx = workspace.repaint_ctx.clone();
        if let Err(error) = std::thread::Builder::new().name("r-help".into()).spawn(move || {
            let result = session.lock().map_err(|error| format!("R session lock failed: {error}")).and_then(|mut session| session.help_text(&topic));
            let _ = tx.send(RMessage::Help(result));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        }) {
            workspace.running = false;
            workspace.r_status = format!("Could not start R help lookup: {error}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        workspace.running = false;
        workspace.help_text = "R help requires the native desktop R session.".into();
    }
}

fn request_packages(workspace: &mut Workspace) {
    if workspace.running {
        return;
    }
    workspace.running = true;
    workspace.r_status = "Reading installed R packages...".into();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let session = workspace.session.clone();
        let tx = workspace._tx.clone();
        let ctx = workspace.repaint_ctx.clone();
        if let Err(error) = std::thread::Builder::new().name("r-packages".into()).spawn(move || {
            let result = session.lock().map_err(|error| format!("R session lock failed: {error}")).and_then(|mut session| session.packages());
            let _ = tx.send(RMessage::Packages(result));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        }) {
            workspace.running = false;
            workspace.r_status = format!("Could not start R package query: {error}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        workspace.running = false;
        workspace.r_status = "R packages require the native desktop R session.".into();
    }
}

fn url_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

fn open_script(app: &mut crate::VectorcraftApp) {
    let pick = crate::FilePick { filters: vec![crate::FilePick::filter("R scripts", &["R", "r"])], ..Default::default() };
    let Some(path) = crate::picks::open(app, &pick) else { return };
    open_script_path(app, &path);
}

fn open_script_path(app: &mut crate::VectorcraftApp, path: &str) {
    let result = app.services.read.as_ref().ok_or_else(|| "No file reader is available".to_owned()).and_then(|read| read(path));
    match result {
        Ok(bytes) => match decode_script_bytes(&bytes) {
            Ok((script, format)) => install_script_document(app, path, bytes, script, format),
            Err(error) => {
                app.scientific.pending_script_decode = Some(PendingScriptDecode { path: path.to_owned(), bytes, reason: error.clone() });
                app.scientific.r_status = format!("Could not safely decode {path}: {error}. Choose the file encoding to open it.");
            }
        },
        Err(error) => app.scientific.r_status = format!("Could not open R script: {error}"),
    }
}

fn install_script_document(app: &mut crate::VectorcraftApp, path: &str, bytes: Vec<u8>, script: String, format: ScriptFormat) {
    log_script_import_diagnostics(path, &bytes, &script, format);
    if let Some(existing) = app.scientific.scripts.iter().find(|open| open.path.as_deref().is_some_and(|open_path| same_script_path(open_path, path)))
    {
        let id = existing.id;
        app.scientific.activate_script(id);
        app.scientific.r_status = format!("Already open: {path}");
        return;
    }
    let id = app.scientific.new_script();
    if let Some(document) = app.scientific.scripts.iter_mut().find(|document| document.id == id) {
        document.text = script.clone();
        document.saved_text = script;
        document.path = Some(path.to_owned());
        document.saved_bytes = Some(bytes);
        document.source_format = format;
    }
    app.scientific.r_status = format!("Opened {path}");
}

fn log_script_import_diagnostics(path: &str, original: &[u8], decoded: &str, format: ScriptFormat) {
    if std::env::var_os("VECTORCRAFT_R_SOURCE_DIAGNOSTICS").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let Ok(reencoded) = encode_script_bytes(decoded, format) else {
        eprintln!("[r-source-diagnostic] stage=file-import path={path} encoding={:?} reencode=failed", format.encoding);
        return;
    };
    let difference = first_differing_byte(original, &reencoded).map_or_else(|| "none".to_owned(), |offset| offset.to_string());
    eprintln!(
        "[r-source-diagnostic] stage=file-import path={} encoding={:?} line_ending={:?} source_bytes={} source_fnv1a64={:016x} roundtrip_bytes={} roundtrip_fnv1a64={:016x} first_diff_offset={difference}",
        path,
        format.encoding,
        format.line_ending,
        original.len(),
        source_diagnostic_hash(original),
        reencoded.len(),
        source_diagnostic_hash(&reencoded),
    );
}

fn first_differing_byte(left: &[u8], right: &[u8]) -> Option<usize> {
    left.iter().zip(right).position(|(left, right)| left != right).or_else(|| (left.len() != right.len()).then_some(left.len().min(right.len())))
}

fn source_diagnostic_hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3))
}

fn decode_script_bytes(bytes: &[u8]) -> Result<(String, ScriptFormat), String> {
    use encoding_rs::{EUC_KR, UTF_8};

    let (payload, encoding) = if let Some(payload) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (payload, ScriptEncoding::Utf8Bom)
    } else if let Some(payload) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16_script(payload, true, ScriptEncoding::Utf16Le);
    } else if let Some(payload) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16_script(payload, false, ScriptEncoding::Utf16Be);
    } else if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(normalize_script(text, ScriptEncoding::Utf8));
    } else if EUC_KR.decode_without_bom_handling_and_without_replacement(bytes).is_some() {
        let offset = std::str::from_utf8(bytes).err().map_or(0, |error| error.valid_up_to());
        return Err(format!("not valid UTF-8; CP949/EUC-KR may be valid. Select the encoding before opening (byte offset {offset})"));
    } else {
        let offset = std::str::from_utf8(bytes).err().map_or(0, |error| error.valid_up_to());
        return Err(format!("invalid UTF-8 and CP949 data near byte offset {offset}"));
    };
    let text = UTF_8.decode_without_bom_handling_and_without_replacement(payload).ok_or_else(|| {
        let offset = std::str::from_utf8(payload).err().map_or(0, |error| error.valid_up_to());
        format!("invalid UTF-8 data near byte offset {}", offset.saturating_add(3))
    })?;
    Ok(normalize_script(&text, encoding))
}

fn decode_script_as(bytes: &[u8], encoding: ScriptEncoding) -> Result<(String, ScriptFormat), String> {
    use encoding_rs::{EUC_KR, UTF_8};

    match encoding {
        ScriptEncoding::Utf8 | ScriptEncoding::Utf8Bom => {
            let payload = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
            let text = UTF_8.decode_without_bom_handling_and_without_replacement(payload).ok_or_else(|| {
                let offset = std::str::from_utf8(payload).err().map_or(0, |error| error.valid_up_to());
                format!("invalid UTF-8 data near byte offset {offset}")
            })?;
            Ok(normalize_script(&text, encoding))
        }
        ScriptEncoding::Utf16Le | ScriptEncoding::Utf16Be => {
            let (payload, little_endian) = match encoding {
                ScriptEncoding::Utf16Le => (bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes), true),
                ScriptEncoding::Utf16Be => (bytes.strip_prefix(&[0xFE, 0xFF]).unwrap_or(bytes), false),
                _ => return Err("invalid UTF-16 encoding selection".into()),
            };
            decode_utf16_script(payload, little_endian, encoding)
        }
        ScriptEncoding::WindowsKorean => EUC_KR
            .decode_without_bom_handling_and_without_replacement(bytes)
            .map(|text| normalize_script(&text, encoding))
            .ok_or_else(|| "the file is not valid CP949/EUC-KR".into()),
    }
}

fn decode_utf16_script(bytes: &[u8], little_endian: bool, encoding: ScriptEncoding) -> Result<(String, ScriptFormat), String> {
    let (pairs, remainder) = bytes.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(format!("odd-length UTF-16 data near byte offset {}", bytes.len().saturating_sub(1).saturating_add(2)));
    }
    let units = pairs.iter().map(|pair| if little_endian { u16::from_le_bytes(*pair) } else { u16::from_be_bytes(*pair) }).collect::<Vec<_>>();
    if let Some((index, _)) = std::char::decode_utf16(units.iter().copied()).enumerate().find(|(_, result)| result.is_err()) {
        return Err(format!("unpaired UTF-16 surrogate near byte offset {}", index.saturating_mul(2).saturating_add(2)));
    }
    let text = String::from_utf16(&units).map_err(|_| "invalid UTF-16 data".to_owned())?;
    Ok(normalize_script(&text, encoding))
}

fn normalize_script(text: &str, encoding: ScriptEncoding) -> (String, ScriptFormat) {
    let newline = detect_line_ending(text);
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    (normalized, ScriptFormat { encoding, line_ending: newline })
}

fn detect_line_ending(text: &str) -> ScriptLineEnding {
    let bytes = text.as_bytes();
    let mut crlf = 0usize;
    let mut lf = 0usize;
    let mut cr = 0usize;
    let mut index = 0usize;
    while let Some(byte) = bytes.get(index) {
        match byte {
            b'\r' if bytes.get(index.saturating_add(1)) == Some(&b'\n') => {
                crlf = crlf.saturating_add(1);
                index = index.saturating_add(2);
            }
            b'\r' => {
                cr = cr.saturating_add(1);
                index = index.saturating_add(1);
            }
            b'\n' => {
                lf = lf.saturating_add(1);
                index = index.saturating_add(1);
            }
            _ => index = index.saturating_add(1),
        }
    }
    if crlf >= lf && crlf >= cr && crlf > 0 {
        ScriptLineEnding::CrLf
    } else if cr > lf && cr > 0 {
        ScriptLineEnding::Cr
    } else {
        ScriptLineEnding::Lf
    }
}

fn encode_script_bytes(text: &str, format: ScriptFormat) -> Result<Vec<u8>, String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let serialized = if format.line_ending == ScriptLineEnding::Lf { normalized } else { normalized.replace('\n', format.line_ending.as_str()) };
    match format.encoding {
        ScriptEncoding::Utf8 => Ok(serialized.into_bytes()),
        ScriptEncoding::Utf8Bom => {
            let mut bytes = vec![0xEF, 0xBB, 0xBF];
            bytes.extend_from_slice(serialized.as_bytes());
            Ok(bytes)
        }
        ScriptEncoding::Utf16Le | ScriptEncoding::Utf16Be => {
            let mut bytes = if format.encoding == ScriptEncoding::Utf16Le { vec![0xFF, 0xFE] } else { vec![0xFE, 0xFF] };
            for unit in serialized.encode_utf16() {
                let pair = if format.encoding == ScriptEncoding::Utf16Le { unit.to_le_bytes() } else { unit.to_be_bytes() };
                bytes.extend_from_slice(&pair);
            }
            Ok(bytes)
        }
        ScriptEncoding::WindowsKorean => {
            let (bytes, _, had_errors) = encoding_rs::EUC_KR.encode(&serialized);
            if had_errors {
                return Err("the edited script contains characters that cannot be saved as CP949/EUC-KR; the existing file was left unchanged".into());
            }
            Ok(bytes.into_owned())
        }
    }
}

fn same_script_path(left: &str, right: &str) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| std::path::PathBuf::from(left));
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| std::path::PathBuf::from(right));
    let left = left.to_string_lossy().replace('\\', "/");
    let right = right.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    return left.eq_ignore_ascii_case(&right);
    #[cfg(not(windows))]
    return left == right;
}

fn save_script_for(app: &mut crate::VectorcraftApp, script_id: u64, save_as: bool, close_after_save: bool) {
    let Some(document) = app.scientific.scripts.iter().find(|script| script.id == script_id) else { return };
    let mut path = document.path.clone().filter(|_| !save_as);
    let verify_existing = path.is_some();
    let pick = crate::FilePick {
        name: document.filename().to_owned(),
        filters: vec![crate::FilePick::filter("R scripts", &["R", "r"])],
        ..Default::default()
    };
    if path.is_none() {
        let Some(picked) = crate::picks::save(app, &pick) else { return };
        if crate::picks::ask_to_replace(app, crate::picks::PickRequest::Save(pick), &picked, None, &picked) {
            return;
        }
        path = Some(picked);
    }
    let Some(path) = path else { return };
    if verify_existing {
        let Some(expected) = app.scientific.scripts.iter().find(|script| script.id == script_id).and_then(|script| script.saved_bytes.as_deref())
        else {
            app.scientific.r_status = "Could not verify the saved R script; use Save As to write safely".into();
            return;
        };
        let disk = app.services.read.as_ref().ok_or_else(|| "No file reader is available".to_owned()).and_then(|read| read(&path));
        match disk {
            Ok(bytes) if bytes == expected => {}
            Ok(_) => {
                app.scientific.r_status = format!("{path} changed outside FigureIDE; use Save As to avoid overwriting it");
                return;
            }
            Err(error) => {
                app.scientific.r_status = format!("Could not verify {path} before saving: {error}");
                return;
            }
        }
    }
    let Some((text, format)) =
        app.scientific.scripts.iter().find(|script| script.id == script_id).map(|script| (script.text.clone(), script.source_format))
    else {
        return;
    };
    let script_bytes = match encode_script_bytes(&text, format) {
        Ok(bytes) => bytes,
        Err(error) => {
            app.scientific.r_status = format!("Could not save {path}: {error}");
            return;
        }
    };
    let result = crate::io::write_reporting(app, &path, &script_bytes);
    match result {
        Ok(()) => {
            if let Some(document) = app.scientific.scripts.iter_mut().find(|script| script.id == script_id) {
                document.path = Some(path.clone());
                document.saved_text = document.text.clone();
                document.saved_bytes = Some(script_bytes);
            }
            app.scientific.r_status = format!("Saved {path}");
            if close_after_save {
                app.scientific.close_script_now(script_id);
            }
        }
        Err(error) => app.scientific.r_status = format!("Could not save R script: {error}"),
    }
}

fn panel_label(index: usize) -> String {
    let mut value = index.saturating_add(1);
    let mut label = String::new();
    while value > 0 {
        value -= 1;
        let ch = char::from_u32(u32::from(b'A') + u32::try_from(value % 26).unwrap_or(0)).unwrap_or('A');
        label.insert(0, ch);
        value /= 26;
    }
    label
}

fn update_selected_panel(app: &mut crate::VectorcraftApp) {
    restore_linked_panels(app);
    let Some(document_uid) = app.session.active().map(|doc| doc.uid) else { return };
    let selected = app.session.active().map(|doc| doc.selection.objects.clone()).unwrap_or_default();
    let Some(panel) = app
        .scientific
        .linked_panels
        .iter()
        .find(|panel| {
            panel.document_uid == document_uid
                && (selected.contains(&vectorcraft_doc::NodeId(panel.panel_id)) || selected.contains(&vectorcraft_doc::NodeId(panel.generated_id)))
        })
        .cloned()
    else {
        app.scientific.r_status = "Select a linked plot panel to update it".into();
        return;
    };
    render_plot(
        &mut app.scientific,
        panel.document_uid,
        panel.r_variable.clone(),
        PanelAction::Refresh { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm: panel.width_mm, height_mm: panel.height_mm },
    );
}

pub(crate) fn update_panel_async(app: &mut crate::VectorcraftApp, panel_id: u64) -> Result<(), String> {
    if app.scientific.running {
        return Err("R session is busy".into());
    }
    let panel = app.scientific.linked_panels.iter().find(|p| p.panel_id == panel_id).cloned().ok_or_else(|| format!("Panel {panel_id} not found"))?;

    app.scientific.panel_states.insert(panel_id, LinkedPanelState::Updating);
    render_plot(
        &mut app.scientific,
        panel.document_uid,
        panel.r_variable.clone(),
        PanelAction::Refresh { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm: panel.width_mm, height_mm: panel.height_mm },
    );
    Ok(())
}

pub(crate) fn relink_panel_async(app: &mut crate::VectorcraftApp, panel_id: u64, target_variable: String) -> Result<(), String> {
    if app.scientific.running {
        return Err("R session is busy".into());
    }
    let panel = app.scientific.linked_panels.iter().find(|p| p.panel_id == panel_id).cloned().ok_or_else(|| format!("Panel {panel_id} not found"))?;

    let is_plot = app.scientific.plots.iter().any(|p| p.variable == target_variable);
    if !is_plot {
        return Err(format!("Object '{target_variable}' is not a valid plot object"));
    }

    app.scientific.panel_states.insert(panel_id, LinkedPanelState::Updating);
    render_plot(
        &mut app.scientific,
        panel.document_uid,
        target_variable.clone(),
        PanelAction::Relink {
            panel_id: panel.panel_id,
            generated_id: panel.generated_id,
            target_variable,
            width_mm: panel.width_mm,
            height_mm: panel.height_mm,
        },
    );
    Ok(())
}

pub(crate) fn restore_project_rdata_action(app: &mut crate::VectorcraftApp) {
    let Some(path_buf) = app.scientific.resolve_project_rdata_path() else {
        app.scientific.r_status = "No RData reference in current project".into();
        return;
    };
    if !path_buf.exists() {
        app.scientific.r_status = format!("Referenced RData file not found: {}", path_buf.display());
        return;
    }
    let path_str = path_buf.to_string_lossy().to_string();
    import_rdata_path(app, &path_str, None);
}

fn update_all_panels(app: &mut crate::VectorcraftApp) {
    restore_linked_panels(app);
    let Some(document_uid) = app.session.active().map(|doc| doc.uid) else { return };
    app.scientific.pending_refresh = app.scientific.linked_panels.iter().filter(|panel| panel.document_uid == document_uid).cloned().collect();
    if app.scientific.pending_refresh.is_empty() {
        app.scientific.r_status = "There are no linked panels to update".into();
    } else {
        start_next_refresh(app);
    }
}

fn selected_linked_panel(app: &crate::VectorcraftApp) -> Option<RLinkedPanel> {
    let document_uid = app.session.active()?.uid;
    let selection = app.session.active()?.selection.objects.as_slice();
    app.scientific
        .linked_panels
        .iter()
        .find(|panel| {
            panel.document_uid == document_uid
                && (selection.contains(&vectorcraft_doc::NodeId(panel.panel_id)) || selection.contains(&vectorcraft_doc::NodeId(panel.generated_id)))
        })
        .cloned()
}

fn start_next_refresh(app: &mut crate::VectorcraftApp) {
    if app.scientific.running {
        return;
    }
    let Some(document_uid) = app.session.active().map(|document| document.uid) else { return };
    if let Some(current) = take_next_refresh(&mut app.scientific, document_uid) {
        render_plot(
            &mut app.scientific,
            current.document_uid,
            current.r_variable,
            PanelAction::Refresh {
                panel_id: current.panel_id,
                generated_id: current.generated_id,
                width_mm: current.width_mm,
                height_mm: current.height_mm,
            },
        );
    }
}

fn take_next_refresh(workspace: &mut Workspace, document_uid: u64) -> Option<RLinkedPanel> {
    while let Some(index) = workspace.pending_refresh.iter().position(|panel| panel.document_uid == document_uid) {
        let panel = workspace.pending_refresh.remove(index)?;
        let current = workspace.linked_panels.iter().find(|current| {
            current.document_uid == panel.document_uid
                && current.panel_id == panel.panel_id
                && current.generated_id == panel.generated_id
                && current.r_variable == panel.r_variable
        });
        let Some(current) = current.cloned() else {
            continue;
        };
        if workspace.graph_pending.contains_key(&(current.document_uid, current.panel_id)) {
            continue;
        }
        return Some(current);
    }
    None
}

pub(super) fn restore_linked_panels(app: &mut crate::VectorcraftApp) {
    let docs: Vec<(u64, std::sync::Arc<vectorcraft_doc::Document>)> = app.session.documents().iter().map(|d| (d.uid, d.doc.clone())).collect();
    for (document_uid, doc) in docs {
        let mut restored = Vec::new();
        doc.walk(|node| {
            let Some(name) = node.name.as_deref().and_then(|name| name.strip_prefix("RVS_PANEL|")) else { return };
            let fields: Vec<_> = name.split('|').collect();
            let (label, encoded_variable, width_mm, height_mm, small_figure_typography, graph_style) = if fields.first().copied() == Some("v4") {
                if fields.len() != 8 {
                    return;
                }
                let Some(width) = fields.get(3).and_then(|value| value.parse::<f64>().ok()).filter(|value| value.is_finite() && *value > 0.0) else {
                    return;
                };
                let Some(height) = fields.get(4).and_then(|value| value.parse::<f64>().ok()).filter(|value| value.is_finite() && *value > 0.0) else {
                    return;
                };
                let graph_style = fields
                    .get(7)
                    .filter(|encoded| encoded.len() <= 32768)
                    .and_then(|encoded| decode_name(encoded))
                    .and_then(|json| serde_json::from_str::<GraphStyle>(&json).ok())
                    .filter(|style| style.validate().is_ok())
                    .unwrap_or_default();
                (fields[1], fields[2], width, height, fields[6] == "small", graph_style)
            } else if fields.first().copied() == Some("v3") {
                if fields.len() != 7 {
                    return;
                }
                let Some(width) = fields.get(3).and_then(|value| value.parse::<f64>().ok()).filter(|value| value.is_finite() && *value > 0.0) else {
                    return;
                };
                let Some(height) = fields.get(4).and_then(|value| value.parse::<f64>().ok()).filter(|value| value.is_finite() && *value > 0.0) else {
                    return;
                };
                (fields[1], fields[2], width, height, fields[6] == "small", GraphStyle::default())
            } else if fields.first().copied() == Some("v2") {
                if fields.len() != 6 {
                    return;
                }
                let Some(width) = fields.get(3).and_then(|value| value.parse::<f64>().ok()).filter(|value| value.is_finite() && *value > 0.0) else {
                    return;
                };
                let Some(height) = fields.get(4).and_then(|value| value.parse::<f64>().ok()).filter(|value| value.is_finite() && *value > 0.0) else {
                    return;
                };
                (fields[1], fields[2], width, height, false, GraphStyle::default())
            } else {
                if fields.len() != 2 {
                    return;
                }
                (fields[0], fields[1], 177.8, 127.0, false, GraphStyle::default())
            };
            // Old Scale metadata remains readable, but every restored link now follows Reflow.
            let stored_mode = fields.get(5).map(|mode| ResizeMode::from_str(mode)).unwrap_or_default();
            let resize_mode = match stored_mode {
                ResizeMode::Reflow | ResizeMode::Scale => ResizeMode::Reflow,
            };
            let Some(variable) = decode_name(encoded_variable) else { return };
            let Some(children) = node.children() else { return };
            let Some(generated) = children.iter().find(|child| child.name.as_deref() == Some("RVS_GENERATED")) else { return };
            if !children.iter().any(|child| child.name.as_deref() == Some("RVS_LABEL")) {
                return;
            }
            restored.push(RLinkedPanel {
                panel_id: node.id.0,
                generated_id: generated.id.0,
                label: label.to_owned(),
                r_variable: variable,
                width_mm,
                height_mm,
                resize_mode,
                graph_style,
                small_figure_typography,
                document_uid,
            });
        });
        let restored_ids: std::collections::HashSet<u64> = restored.iter().map(|panel| panel.panel_id).collect();
        app.scientific.linked_panels.retain(|panel| panel.document_uid != document_uid || restored_ids.contains(&panel.panel_id));
        for panel in restored {
            if let Some(existing) =
                app.scientific.linked_panels.iter_mut().find(|existing| existing.document_uid == document_uid && existing.panel_id == panel.panel_id)
            {
                *existing = panel;
            } else {
                app.scientific.linked_panels.push(panel);
            }
        }
    }
}

fn persistent_panel_name(panel: &RLinkedPanel, width_mm: f64, height_mm: f64, _mode: ResizeMode) -> String {
    let graph_json = serde_json::to_string(&panel.graph_style).unwrap_or_else(|_| "{}".into());
    format!(
        "RVS_PANEL|v4|{}|{}|{width_mm:.6}|{height_mm:.6}|reflow|{}|{}",
        panel.label,
        encode_name(&panel.r_variable),
        if panel.small_figure_typography { "small" } else { "original" },
        encode_name(&graph_json)
    )
}

fn encode_name(name: &str) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::with_capacity(name.len().saturating_mul(2));
    for byte in name.as_bytes() {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn decode_name(encoded: &str) -> Option<String> {
    let (pairs, remainder) = encoded.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    let mut bytes = Vec::with_capacity(pairs.len());
    for pair in pairs {
        bytes.push(u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?);
    }
    String::from_utf8(bytes).ok()
}

pub(crate) fn enqueue_or_execute(workspace: &mut Workspace, command: QueuedRCommand) {
    if workspace.running {
        if workspace.execution_queue.len() >= MAX_QUEUE_CAPACITY {
            workspace.r_status = format!("R execution queue is full (max {MAX_QUEUE_CAPACITY} commands)");
            return;
        }
        workspace.execution_queue.push_back(command);
        let queue_len = workspace.execution_queue.len();
        workspace.r_status = format!("R expression queued ({queue_len} pending)...");
    } else {
        execute_queued_command(workspace, command);
    }
}

pub(crate) fn dispatch_next_queued_command(workspace: &mut Workspace) {
    if workspace.running {
        return;
    }
    if let Some(next_cmd) = workspace.execution_queue.pop_front() {
        execute_queued_command(workspace, next_cmd);
    }
}

fn execute_queued_command(workspace: &mut Workspace, command: QueuedRCommand) {
    append_console_code(workspace, &command.code);
    workspace.console_error = false;
    workspace.running = true;
    workspace.r_status = "R command queued...".into();
    #[cfg(not(target_arch = "wasm32"))]
    let tx = workspace._tx.clone();
    #[cfg(not(target_arch = "wasm32"))]
    let ctx = workspace.repaint_ctx.clone();
    #[cfg(not(target_arch = "wasm32"))]
    let session = workspace.session.clone();
    #[cfg(not(target_arch = "wasm32"))]
    let script_id = command.script_id;
    #[cfg(not(target_arch = "wasm32"))]
    let source_revision = command.source_revision;
    #[cfg(not(target_arch = "wasm32"))]
    let start_line = command.start_line;
    #[cfg(not(target_arch = "wasm32"))]
    let refresh_linked = command.refresh_linked;
    #[cfg(not(target_arch = "wasm32"))]
    let code = command.code;

    #[cfg(not(target_arch = "wasm32"))]
    if let Err(error) = std::thread::Builder::new().name("r-execution".into()).spawn(move || {
        let _ = tx.send(RMessage::Started("R command"));
        if let Some(ctx) = &ctx {
            ctx.request_repaint();
        }
        let (result, definitions) = match session.lock().map_err(|error| format!("R session lock failed: {error}")) {
            Ok(mut session) => {
                std::thread::sleep(std::time::Duration::from_millis(5));
                let result = session.execute_streaming(&code, &mut |chunk| {
                    let _ = tx.send(RMessage::ConsoleChunk(chunk));
                    if let Some(ctx) = &ctx {
                        ctx.request_repaint();
                    }
                });
                let definitions = if result.as_ref().is_ok_and(|execution| execution.error.is_none()) {
                    script_id.and_then(|id| {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        let defs = session.assignment_definitions(&code).ok()?;
                        let base_line = start_line.unwrap_or(0);
                        let mapped = defs.into_iter().map(|(name, rel)| (name, base_line.saturating_add(rel))).collect();
                        source_revision.map(|rev| (id, mapped, rev))
                    })
                } else {
                    None
                };
                (result, definitions)
            }
            Err(error) => (Err(error), None),
        };
        let _ = tx.send(RMessage::Execution(result, refresh_linked, definitions));
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    }) {
        workspace.running = false;
        append_console_output(workspace, &format!("Error: Could not start R worker: {error}\n"));
        dispatch_next_queued_command(workspace);
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = command;
        workspace.running = false;
        append_console_output(workspace, "Error: R execution is available in the native desktop app only\n");
    }
}

fn run_r(workspace: &mut Workspace, code: String, refresh_linked: bool) {
    run_r_with_origin(workspace, code, refresh_linked, None);
}

fn run_r_with_origin(workspace: &mut Workspace, code: String, refresh_linked: bool, source_id: Option<u64>) {
    let source_revision = source_id.and_then(|id| workspace.scripts.iter().find(|s| s.id == id).map(|s| s.text.clone()));
    let command = QueuedRCommand {
        id: workspace.next_queue_id,
        script_id: source_id,
        code,
        source_revision,
        start_line: None,
        refresh_linked,
        is_selection: false,
    };
    workspace.next_queue_id = workspace.next_queue_id.wrapping_add(1).max(1);
    enqueue_or_execute(workspace, command);
}

fn control_r_session(workspace: &mut Workspace, restart: bool) {
    let discarded = workspace.execution_queue.len();
    workspace.execution_queue.clear();
    if discarded > 0 {
        append_console_output(workspace, &format!("[Cancelled {discarded} pending queued R expression(s)]\n"));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let session = workspace.session.clone();
        let tx = workspace._tx.clone();
        let ctx = workspace.repaint_ctx.clone();
        workspace.console_error = false;
        workspace.running = true;
        workspace.r_status = if restart {
            if discarded > 0 {
                format!("Restarting R session ({discarded} queued command(s) cancelled)...")
            } else {
                "Restarting R session...".into()
            }
        } else if discarded > 0 {
            format!("Stopping R session ({discarded} queued command(s) cancelled)...")
        } else {
            "Stopping R session...".into()
        };
        if let Err(error) = std::thread::Builder::new().name("r-session-control".into()).spawn(move || {
            let result = session
                .lock()
                .map_err(|error| format!("R session lock failed: {error}"))
                .and_then(|mut session| if restart { session.restart() } else { session.stop() });
            let _ = tx.send(RMessage::Session(result));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        }) {
            workspace.running = false;
            workspace.r_status = format!("Could not start session control: {error}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = restart;
        workspace.r_status = "R execution is available in the native desktop app only".into();
    }
}

fn request_graph_apply(workspace: &mut Workspace, panel: &RLinkedPanel, style: GraphStyle, width_mm: f64, height_mm: f64) -> Result<(), String> {
    style.validate()?;
    if !width_mm.is_finite() || !height_mm.is_finite() || !(1.0..=1000.0).contains(&width_mm) || !(1.0..=1000.0).contains(&height_mm) {
        return Err("Plot dimensions must be between 1 and 1000 mm".into());
    }
    let kind = workspace.plots.iter().find(|plot| plot.variable == panel.r_variable).map(|plot| plot.plot_kind.as_str());
    if kind.is_none() || (kind != Some("ggplot") && style != panel.graph_style) {
        return Err("Graph styling requires a connected ggplot object".into());
    }
    workspace.graph_pending.insert((panel.document_uid, panel.panel_id), GraphRenderRequest { style, width_mm, height_mm });
    workspace.r_status = format!("Graph changes queued for Panel {}", panel.label);
    if let Some(ctx) = &workspace.repaint_ctx {
        ctx.request_repaint();
    }
    Ok(())
}

fn start_next_graph(app: &mut crate::VectorcraftApp) {
    if app.scientific.running {
        return;
    }
    let Some(document_uid) = app.session.active().map(|doc| doc.uid) else { return };
    let next = app.scientific.graph_pending.iter().find(|((uid, _), _)| *uid == document_uid).map(|(key, request)| (*key, request.clone()));
    let Some((key, request)) = next else { return };
    let Some(panel) = app.scientific.linked_panels.iter().find(|panel| panel.document_uid == key.0 && panel.panel_id == key.1).cloned() else {
        app.scientific.graph_pending.remove(&key);
        return;
    };
    if panel.graph_style == request.style && panel.width_mm == request.width_mm && panel.height_mm == request.height_mm {
        app.scientific.graph_pending.remove(&key);
        return;
    }
    // A Graph render reads the current R object too, so an older queued refresh for this panel
    // would only repeat work and could replay an obsolete layout.
    app.scientific.pending_refresh.retain(|queued| queued.document_uid != key.0 || queued.panel_id != key.1);
    render_plot(
        &mut app.scientific,
        panel.document_uid,
        panel.r_variable,
        PanelAction::Graph {
            panel_id: panel.panel_id,
            generated_id: panel.generated_id,
            width_mm: request.width_mm,
            height_mm: request.height_mm,
            style: request.style,
        },
    );
    if !app.scientific.running {
        app.scientific.graph_pending.remove(&key);
    }
}

pub(super) fn reset_graph_drafts_after_history(app: &mut crate::VectorcraftApp) {
    if let Some(document_uid) = app.session.active().map(|doc| doc.uid) {
        let epoch = app.scientific.render_epoch.entry(document_uid).or_default();
        *epoch = epoch.wrapping_add(1);
        app.scientific.graph_pending.retain(|(uid, _), _| *uid != document_uid);
        app.scientific.graph_drafts.retain(|(uid, _), _| *uid != document_uid);
    }
}

fn render_plot(workspace: &mut Workspace, document_uid: u64, variable: String, action: PanelAction) {
    if workspace.running {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        workspace.running = true;
        workspace.r_status = format!("Rendering {variable}...");
        let session = workspace.session.clone();
        let tx = workspace._tx.clone();
        let ctx = workspace.repaint_ctx.clone();
        let worker_variable = variable.clone();
        let message_variable = variable.clone();
        let render_epoch = workspace.render_epoch.get(&document_uid).copied().unwrap_or_default();
        let message_action = action.clone();
        let (width_mm, height_mm) = match &action {
            PanelAction::Create { width_mm, height_mm, .. }
            | PanelAction::Refresh { width_mm, height_mm, .. }
            | PanelAction::Reflow { width_mm, height_mm, .. }
            | PanelAction::Graph { width_mm, height_mm, .. }
            | PanelAction::Relink { width_mm, height_mm, .. } => (*width_mm, *height_mm),
        };
        let small_figure_typography = match &action {
            PanelAction::Create { .. } => true,
            PanelAction::Refresh { panel_id, .. }
            | PanelAction::Reflow { panel_id, .. }
            | PanelAction::Graph { panel_id, .. }
            | PanelAction::Relink { panel_id, .. } => workspace
                .linked_panels
                .iter()
                .find(|panel| panel.document_uid == document_uid && panel.panel_id == *panel_id)
                .is_some_and(|panel| panel.small_figure_typography),
        };
        let graph_style = match &action {
            PanelAction::Graph { style, .. } => style.clone(),
            PanelAction::Create { .. } => GraphStyle::default(),
            PanelAction::Refresh { panel_id, .. } | PanelAction::Reflow { panel_id, .. } | PanelAction::Relink { panel_id, .. } => workspace
                .linked_panels
                .iter()
                .find(|panel| panel.document_uid == document_uid && panel.panel_id == *panel_id)
                .map(|panel| panel.graph_style.clone())
                .unwrap_or_default(),
        };
        // Keep stored ggplot controls for a future relink, but render other plot kinds without them.
        let graph_style = if workspace.plots.iter().any(|plot| plot.variable == variable && plot.plot_kind == "ggplot") {
            graph_style
        } else {
            GraphStyle::default()
        };
        if let Err(error) = std::thread::Builder::new().name("r-plot-render".into()).spawn(move || {
            let result = session
                .lock()
                .map_err(|error| format!("R session lock failed: {error}"))
                .and_then(|mut session| {
                    session.render_plot_svg_with_graph_style(&worker_variable, width_mm, height_mm, small_figure_typography, &graph_style)
                })
                .map(|(svg, plots)| RenderedPlot { variable: worker_variable, svg, plots, action });
            let _ = tx.send(RMessage::Rendered(document_uid, render_epoch, message_variable, message_action, result));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        }) {
            workspace.running = false;
            workspace.r_status = format!("Could not start plot renderer: {error}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (document_uid, variable, action);
        workspace.r_status = "R execution is available in the native desktop app only".into();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn detect_r() -> Result<String, String> {
    let executable = crate::r_session::find_rscript()?;
    let mut command = std::process::Command::new(&executable);
    command.arg("--version");
    #[cfg(windows)]
    crate::r_session::configure_background_r_command(&mut command);
    let output = command.output().map_err(|error| format!("Could not run {}: {error}", executable.display()))?;
    if output.status.success() {
        let version = if output.stdout.is_empty() { &output.stderr } else { &output.stdout };
        Ok(format!("{} ({})", String::from_utf8_lossy(version).trim(), executable.display()))
    } else {
        Err("Rscript was found but did not report a version".into())
    }
}

#[cfg(target_arch = "wasm32")]
fn detect_r() -> Result<String, String> {
    Err("R execution is available in the native desktop app only".into())
}

fn poll_messages(app: &mut crate::VectorcraftApp) {
    let mut rendered = Vec::new();
    loop {
        match app.scientific._rx.try_recv() {
            Ok(RMessage::Started(label)) => {
                app.scientific.r_status = format!("{label} running...");
            }
            Ok(RMessage::ConsoleChunk(chunk)) => append_console_output(&mut app.scientific, &chunk),
            Ok(RMessage::ConsoleInputParsed(Ok(crate::r_session::ParseState::Incomplete))) => {
                app.scientific.running = false;
                app.scientific.session_active = true;
                app.scientific.console_continuation = true;
                app.scientific.r_status = "Waiting for continuation input".into();
            }
            Ok(RMessage::ConsoleInputParsed(Ok(crate::r_session::ParseState::Complete))) => {
                app.scientific.running = false;
                app.scientific.session_active = true;
                app.scientific.console_continuation = false;
                let code = std::mem::take(&mut app.scientific.console_pending_lines).join("\n");
                record_history(&mut app.scientific, &code);
                run_r(&mut app.scientific, code, false);
            }
            Ok(RMessage::ConsoleInputParsed(Ok(crate::r_session::ParseState::Invalid(error)))) => {
                app.scientific.running = false;
                app.scientific.session_active = true;
                app.scientific.console_continuation = false;
                app.scientific.console_pending_lines.clear();
                append_console_output(&mut app.scientific, &format!("Error: {error}\n"));
                app.scientific.r_status = "R syntax error".into();
                app.scientific.console_error = true;
            }
            Ok(RMessage::ConsoleInputParsed(Err(error))) => {
                app.scientific.running = false;
                app.scientific.console_pending_lines.clear();
                app.scientific.console_continuation = false;
                append_console_output(&mut app.scientific, &format!("Error: {error}\n"));
                app.scientific.r_status = "R parser failed".into();
                app.scientific.console_error = true;
            }
            Ok(RMessage::Availability(Ok(version))) => {
                app.scientific.checking_r = false;
                app.scientific.availability_checked = true;
                app.scientific.r_status = format!("R detected: {version}");
            }
            Ok(RMessage::Availability(Err(error))) => {
                app.scientific.checking_r = false;
                app.scientific.availability_checked = true;
                app.scientific.r_status = error;
            }
            Ok(RMessage::Execution(Ok(execution), refresh_linked, definitions)) => {
                app.scientific.running = false;
                app.scientific.session_active = true;
                if let Some(wd) = execution.working_directory {
                    app.scientific.working_directory = Some(wd);
                }
                app.scientific.r_status = "R command completed".into();
                app.scientific.objects = execution.objects;
                app.scientific.plots = app.scientific.objects.iter().filter(|object| !object.plot_kind.is_empty()).cloned().collect();
                append_console_output(&mut app.scientific, &execution.output);
                if !execution.output.is_empty() && !execution.output.ends_with('\n') {
                    append_console_output(&mut app.scientific, "\n");
                }
                if let Some(error) = &execution.error {
                    app.scientific.source_origins.retain(|name, _| app.scientific.objects.iter().any(|obj| &obj.variable == name));
                    append_console_output(&mut app.scientific, &format!("Error: {error}\n"));
                    app.scientific.r_status = format!("R command failed: {error}");
                    app.scientific.console_error = true;
                } else {
                    app.scientific.console_error = false;
                    app.scientific.source_origins.retain(|name, _| app.scientific.objects.iter().any(|obj| &obj.variable == name));
                    if let Some((script_id, definitions, revision)) = definitions
                        && app.scientific.scripts.iter().any(|script| script.id == script_id && script.text == revision)
                    {
                        if refresh_linked {
                            app.scientific.source_origins.retain(|_, origin| origin.script_id != script_id);
                        }
                        for (name, line) in definitions {
                            app.scientific.source_origins.insert(name, SourceOrigin { script_id, line, source_revision: revision.clone() });
                        }
                    }
                }
                if execution.error.is_none() && refresh_linked {
                    let document_uid = app.session.active().map(|doc| doc.uid).unwrap_or_default();
                    app.scientific.pending_refresh =
                        app.scientific.linked_panels.iter().filter(|panel| panel.document_uid == document_uid).cloned().collect();
                }
                dispatch_next_queued_command(&mut app.scientific);
            }
            Ok(RMessage::Execution(Err(error), _, _)) => {
                app.scientific.running = false;
                append_console_output(&mut app.scientific, &format!("Error: {error}\n"));
                let discarded = app.scientific.execution_queue.len();
                app.scientific.execution_queue.clear();
                if discarded > 0 {
                    app.scientific.r_status = format!("R session failed: {error} ({discarded} queued commands discarded)");
                    append_console_output(&mut app.scientific, &format!("[Discarded {discarded} queued R command(s) due to session termination]\n"));
                } else {
                    app.scientific.r_status = format!("R command failed: {error}");
                }
                app.scientific.console_error = true;
            }
            Ok(RMessage::SourceDefinitions(variable, Ok(candidates))) => {
                app.scientific.session_active = true;
                match candidates.as_slice() {
                    [] => app.scientific.r_status = format!("No open script defines {variable}; the object may have been created in the Console"),
                    [(id, line, revision)] => {
                        if let Some(script) = app.scientific.scripts.iter().find(|script| script.id == *id) {
                            if script.text == *revision {
                                if let Some(index) =
                                    line_start_byte(&script.text, *line).and_then(|byte| script.text.get(..byte)).map(|prefix| prefix.chars().count())
                                {
                                    navigate_source(&mut app.scientific, *id, index);
                                    app.scientific.r_status = format!("Opened candidate definition for {variable}; execution origin is unknown");
                                }
                            } else {
                                app.scientific.r_status = format!("Source for {variable} changed during lookup; retry");
                            }
                        } else {
                            app.scientific.r_status = format!("Source for {variable} was closed during lookup");
                        }
                    }
                    _ => app.scientific.r_status = format!("Multiple open definitions of {variable}; source is ambiguous"),
                }
            }
            Ok(RMessage::SourceDefinitions(variable, Err(error))) => {
                app.scientific.r_status = format!("Could not find source for {variable}: {error}")
            }
            Ok(RMessage::Session(Ok(message))) => {
                app.scientific.running = false;
                app.scientific.console_error = false;
                app.scientific.session_active = message.contains("started") || message.contains("already running");
                if message.contains("restarted") || message.contains("stopped") {
                    app.scientific.objects.clear();
                    app.scientific.plots.clear();
                    app.scientific.source_origins.clear();
                    app.scientific.viewer = None;
                    app.scientific.execution_queue.clear();
                    app.scientific.pending_rdata_import = None;
                }
                app.scientific.r_status = message;
            }
            Ok(RMessage::Session(Err(error))) => {
                app.scientific.running = false;
                app.scientific.session_active = false;
                app.scientific.execution_queue.clear();
                app.scientific.pending_rdata_import = None;
                app.scientific.r_status = error;
            }
            Ok(RMessage::Rendered(document_uid, render_epoch, variable, action, result)) => {
                app.scientific.running = false;
                rendered.push((document_uid, render_epoch, variable, action, result));
            }
            Ok(RMessage::DataPage(name, Ok(page))) => {
                app.scientific.running = false;
                app.scientific.session_active = true;
                if let Some(tab) = app.scientific.viewer_tabs.iter_mut().find(|t| t.name == name) {
                    tab.loading = false;
                    tab.page = Some(page.clone());
                    tab.error = None;
                }
                app.scientific.viewer = Some((name.clone(), page));
                app.scientific.r_status = format!("Data page loaded: {name}");
            }
            Ok(RMessage::DataPage(name, Err(error))) => {
                app.scientific.running = false;
                if let Some(tab) = app.scientific.viewer_tabs.iter_mut().find(|t| t.name == name) {
                    tab.loading = false;
                    tab.error = Some(error.clone());
                }
                app.scientific.r_status = format!("Data viewer: {error}");
                app.scientific.console_error = true;
            }
            Ok(RMessage::EnvironmentAction(Ok(execution))) => {
                app.scientific.running = false;
                app.scientific.session_active = true;
                if let Some(wd) = execution.working_directory {
                    app.scientific.working_directory = Some(wd);
                }
                app.scientific.objects = execution.objects;
                app.scientific.plots = app.scientific.objects.iter().filter(|object| !object.plot_kind.is_empty()).cloned().collect();
                app.scientific.source_origins.retain(|name, _| app.scientific.objects.iter().any(|obj| &obj.variable == name));
                append_console_output(&mut app.scientific, &execution.output);
                if let Some(error) = execution.error {
                    app.scientific.r_status = format!("R environment operation failed: {error}");
                    app.scientific.console_error = true;
                } else {
                    app.scientific.console_error = false;
                    app.scientific.r_status = "R environment updated".into();
                }
            }
            Ok(RMessage::EnvironmentAction(Err(error))) => {
                app.scientific.running = false;
                app.scientific.r_status = format!("R environment operation failed: {error}");
                app.scientific.console_error = true;
            }
            Ok(RMessage::Help(Ok(text))) => {
                app.scientific.running = false;
                app.scientific.session_active = true;
                app.scientific.help_text = text;
                app.scientific.r_status = format!("Help loaded for {}", app.scientific.help_query);
            }
            Ok(RMessage::Help(Err(error))) => {
                app.scientific.running = false;
                app.scientific.help_text = error.clone();
                app.scientific.r_status = format!("R help lookup failed: {error}");
            }
            Ok(RMessage::Packages(Ok(packages))) => {
                app.scientific.running = false;
                app.scientific.session_active = true;
                app.scientific.packages = packages;
                app.scientific.packages_loaded = true;
                app.scientific.r_status = "Installed R packages loaded".into();
            }
            Ok(RMessage::Packages(Err(error))) => {
                app.scientific.running = false;
                app.scientific.r_status = format!("R package query failed: {error}");
            }
            Ok(RMessage::CompletionMetadata { script_id, source_revision, object_columns, namespace_items }) => {
                app.scientific.completion_request_in_flight = false;
                if let Some((obj, cols)) = object_columns {
                    app.scientific.completion_object_columns.insert(obj, cols);
                }
                if let Some((ns_key, items)) = namespace_items {
                    app.scientific.completion_namespace_cache.insert(ns_key, items);
                }
                let maybe_active_context = app.scientific.completion.as_ref().and_then(|active| {
                    if active.script_id == script_id && active.source_revision == source_revision { Some(active.context.clone()) } else { None }
                });
                if let Some(context) = maybe_active_context {
                    let candidates = source_editor::build_completion_candidates(&app.scientific, script_id, &source_revision, &context);
                    if let Some(active) = app.scientific.completion.as_mut()
                        && !candidates.is_empty()
                    {
                        active.candidates = candidates;
                        active.selected = active.selected.min(active.candidates.len().saturating_sub(1));
                    }
                }
            }
            Ok(RMessage::RDataInspected { path, policy_override, inspection }) => match inspection {
                Ok(inspection) => {
                    app.scientific.session_active = true;
                    if let Some(policy) = policy_override {
                        app.scientific.running = false;
                        apply_rdata_import_async(app, path, policy);
                    } else if inspection.collisions.is_empty() {
                        app.scientific.running = false;
                        apply_rdata_import_async(app, path, crate::r_session::ImportCollisionPolicy::Overwrite);
                    } else {
                        app.scientific.running = false;
                        let n = inspection.collisions.len();
                        app.scientific.r_status = format!("Found {n} collision(s) in RData import: choose Overwrite, Skip, or Cancel");
                        app.scientific.pending_rdata_import = Some(inspection);
                    }
                }
                Err(error) => {
                    app.scientific.running = false;
                    app.scientific.console_error = true;
                    append_console_output(&mut app.scientific, &format!("Error: {error}\n"));
                    app.scientific.r_status = format!("RData inspection failed: {error}");
                }
            },
            Ok(RMessage::RDataImported { path, result }) => {
                app.scientific.running = false;
                match result {
                    Ok((execution, imported_count, skipped_count)) => {
                        app.scientific.session_active = true;
                        app.scientific.console_error = false;
                        if let Some(wd) = execution.working_directory {
                            app.scientific.working_directory = Some(wd);
                        }
                        app.scientific.objects = execution.objects;
                        app.scientific.plots = app.scientific.objects.iter().filter(|object| !object.plot_kind.is_empty()).cloned().collect();
                        app.scientific.source_origins.retain(|name, _| app.scientific.objects.iter().any(|obj| &obj.variable == name));
                        let file_name = std::path::Path::new(&path).file_name().and_then(|f| f.to_str()).unwrap_or(&path);
                        let summary = if skipped_count > 0 {
                            format!("Imported {imported_count} object(s) from {file_name} ({skipped_count} skipped)")
                        } else {
                            format!("Imported {imported_count} object(s) from {file_name}")
                        };
                        app.scientific.r_status = summary.clone();
                        append_console_output(&mut app.scientific, &format!("> load(\"{file_name}\") # {summary}\n"));
                        if !execution.output.is_empty() {
                            append_console_output(&mut app.scientific, &execution.output);
                            if !execution.output.ends_with('\n') {
                                append_console_output(&mut app.scientific, "\n");
                            }
                        }
                    }
                    Err(error) => {
                        app.scientific.console_error = true;
                        append_console_output(&mut app.scientific, &format!("Error: {error}\n"));
                        app.scientific.r_status = format!("RData import failed: {error}");
                    }
                }
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
        }
    }
    for (document_uid, render_epoch, variable, action, result) in rendered {
        // Zero represents the no-document state; Create may open its artboard on completion.
        if app.session.active().map_or(0, |document| document.uid) != document_uid {
            continue;
        }
        if app.scientific.render_epoch.get(&document_uid).copied().unwrap_or_default() != render_epoch {
            continue;
        }
        let key = match &action {
            PanelAction::Graph { panel_id, generated_id, .. }
            | PanelAction::Refresh { panel_id, generated_id, .. }
            | PanelAction::Reflow { panel_id, generated_id, .. } => app
                .scientific
                .linked_panels
                .iter()
                .find(|panel| {
                    panel.document_uid == document_uid
                        && panel.panel_id == *panel_id
                        && panel.generated_id == *generated_id
                        && panel.r_variable == variable
                })
                .map(|panel| (panel.document_uid, panel.panel_id)),
            PanelAction::Relink { panel_id, generated_id, .. } => app
                .scientific
                .linked_panels
                .iter()
                .find(|panel| panel.document_uid == document_uid && panel.panel_id == *panel_id && panel.generated_id == *generated_id)
                .map(|panel| (panel.document_uid, panel.panel_id)),
            PanelAction::Create { .. } => None,
        };
        if !matches!(action, PanelAction::Create { .. }) && key.is_none() {
            continue;
        }
        let pending_key = key.filter(|key| app.scientific.graph_pending.contains_key(key));
        let stale = match &action {
            PanelAction::Graph { .. } => {
                pending_key.and_then(|key| app.scientific.graph_pending.get(&key)).is_none_or(|request| !request.matches(&action))
            }
            PanelAction::Refresh { .. } | PanelAction::Reflow { .. } | PanelAction::Relink { .. } => pending_key.is_some(),
            PanelAction::Create { .. } => false,
        };
        if stale {
            continue;
        }
        match result {
            Ok(rendered) => {
                app.scientific.session_active = true;
                app.scientific.console_error = false;
                apply_rendered_plot(app, rendered);
            }
            Err(error) => {
                app.scientific.console_error = true;
                app.scientific.r_status = format!("Plot render failed; existing artwork was kept: {error}");
                if let PanelAction::Refresh { panel_id, .. } | PanelAction::Relink { panel_id, .. } = &action {
                    app.scientific.panel_states.insert(*panel_id, LinkedPanelState::Error);
                }
            }
        }
        if matches!(action, PanelAction::Graph { .. })
            && let Some(key) = pending_key
        {
            app.scientific.graph_pending.remove(&key);
        }
    }
    start_next_graph(app);
    start_next_refresh(app);
}

#[cfg(test)]
mod tests {
    use super::{
        BottomTab, PanelAction, QueuedRCommand, RMessage, ScriptDocument, ScriptEncoding, ScriptFormat, ScriptLineEnding, SourceOrigin, Workspace,
        append_console_code, append_console_output, append_eof_newline, choose_script_encoding, clear_console_transcript, code_for_char_range,
        consume_key_no_repeat, control_r_session, go_to_plot_source, history_next, history_previous, line_start_byte, linked_panel_metadata,
        linked_panel_status, navigate_source, object_metadata_summary, object_metadata_tooltip, open_script_path, poll_messages, record_history,
        remove_console_enter_newline, render_plot, request_close_script, run_r, save_script_for, source_active_script, submit_console,
        update_all_panels,
    };

    #[test]
    fn r_status_indicator_tracks_session_state_with_explicit_colors() {
        let mut workspace = Workspace::default();
        assert_eq!(super::r_indicator(&workspace), super::RIndicator::Disconnected);
        workspace.checking_r = true;
        assert_eq!(super::r_indicator(&workspace), super::RIndicator::Checking);
        workspace.checking_r = false;
        workspace.session_active = true;
        assert_eq!(super::r_indicator(&workspace), super::RIndicator::Ready);
        assert_eq!(super::RIndicator::Ready.color(), egui::Color32::from_rgb(49, 166, 105));
        workspace.running = true;
        assert_eq!(super::r_indicator(&workspace), super::RIndicator::Running);
        assert_eq!(super::RIndicator::Running.color(), egui::Color32::from_rgb(220, 73, 73));
        workspace.running = false;
        workspace.console_error = true;
        assert_eq!(super::r_indicator(&workspace), super::RIndicator::Error);
        assert_ne!(super::RIndicator::Error.color(), super::RIndicator::Running.color());
    }

    #[test]
    fn source_navigation_uses_character_positions_and_preserves_script_text() {
        let mut workspace = Workspace::default();
        let script_id = workspace.new_script();
        let text = "# 한글\nplot_B <- 2\n".to_owned();
        workspace.scripts.iter_mut().find(|script| script.id == script_id).unwrap().text = text.clone();
        workspace.source_origins.insert("plot_B".into(), SourceOrigin { script_id, line: 1, source_revision: text.clone() });
        go_to_plot_source(&mut workspace, "plot_B");
        assert_eq!(workspace.active_script_id, script_id);
        assert_eq!(workspace.active_script().unwrap().pending_cursor_index, Some("# 한글\n".chars().count()));
        assert_eq!(workspace.active_script().unwrap().text, text);
        assert_eq!(line_start_byte(&text, 1), Some("# 한글\n".len()));
        assert!(!navigate_source(&mut workspace, u64::MAX, 0));
        workspace.active_script_mut().unwrap().text.push_str("# changed\n");
        go_to_plot_source(&mut workspace, "plot_B");
        assert!(workspace.r_status.contains("changed"));
        workspace.close_script_now(script_id);
        go_to_plot_source(&mut workspace, "plot_B");
        assert!(workspace.r_status.contains("closed"));
    }

    #[test]
    fn eof_newline_is_single_dirty_undoable_edit_and_keeps_format() {
        let mut document = ScriptDocument::new(1, "x <- 1".into(), None, None);
        document.source_format = ScriptFormat { encoding: ScriptEncoding::Utf8Bom, line_ending: ScriptLineEnding::CrLf };
        let ctx = egui::Context::default();
        let id = egui::Id::unique("source-undo-test");
        let mut state = egui::text_edit::TextEditState::default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(6))));
        state.store(&ctx, id);
        append_eof_newline(&mut document, Some(&ctx), Some(id));
        append_eof_newline(&mut document, Some(&ctx), Some(id));
        assert_eq!(document.text, "x <- 1\n");
        assert!(document.is_dirty());
        assert_eq!(document.source_format.line_ending, ScriptLineEnding::CrLf);
        let state = egui::TextEdit::load_state(&ctx, id).unwrap();
        let mut undoer = state.undoer();
        let cursor = egui::text::CCursorRange::one(egui::text::CCursor::new(7));
        assert_eq!(undoer.undo(&(cursor, document.text.clone())).map(|(_, text)| text.as_str()), Some("x <- 1"));
    }

    #[test]
    fn current_expression_advances_across_comments_and_adds_one_eof_line() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let source = "x <- 10\n\n# explanation\ny <- x * 2";
        app.scientific.active_script_mut().unwrap().text = source.into();
        let ctx = egui::Context::default();
        let editor_id = egui::Id::unique("current-expression-navigation");
        let mut state = egui::text_edit::TextEditState::default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(0))));
        state.store(&ctx, editor_id);
        app.scientific.repaint_ctx = Some(ctx);
        app.scientific.editor_widget_ids.insert(1, editor_id);
        super::run_current_expression(&mut app.scientific, editor_id);
        wait_until_idle(&mut app);
        assert_eq!(app.scientific.active_script().unwrap().text, source);
        assert_eq!(app.scientific.active_script().unwrap().pending_cursor_index, Some("x <- 10\n\n# explanation\n".len()));
        let mut state = egui::TextEdit::load_state(app.scientific.repaint_ctx.as_ref().unwrap(), editor_id).unwrap();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new("x <- 10\n\n# explanation\n".len()))));
        state.store(app.scientific.repaint_ctx.as_ref().unwrap(), editor_id);
        super::run_current_expression(&mut app.scientific, editor_id);
        wait_until_idle(&mut app);
        assert_eq!(app.scientific.active_script().unwrap().text, format!("{source}\n"));
        assert_eq!(app.scientific.active_script().unwrap().pending_cursor_index, Some(format!("{source}\n").chars().count()));
        let mut state = egui::TextEdit::load_state(app.scientific.repaint_ctx.as_ref().unwrap(), editor_id).unwrap();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(format!("{source}\n").chars().count()))));
        state.store(app.scientific.repaint_ctx.as_ref().unwrap(), editor_id);
        super::run_current_expression(&mut app.scientific, editor_id);
        wait_until_idle(&mut app);
        assert_eq!(app.scientific.active_script().unwrap().text, format!("{source}\n"), "blank EOF must not create another line");
        let result = app.scientific.session.lock().unwrap().execute("stopifnot(x == 10, y == 20)").unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
    }

    #[test]
    fn slow_current_expression_advances_caret_immediately_and_queues_next_expression() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        assert!(app.scientific.session.lock().unwrap().execute("invisible(NULL)").unwrap().error.is_none());
        app.scientific.active_script_mut().unwrap().text = "Sys.sleep(1)\nnext_value <- 42\n".into();
        let ctx = egui::Context::default();
        let editor_id = egui::Id::unique("slow-current-expression-navigation");
        let mut state = egui::text_edit::TextEditState::default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(0))));
        state.store(&ctx, editor_id);
        app.scientific.repaint_ctx = Some(ctx);
        app.scientific.editor_widget_ids.insert(1, editor_id);

        let started = std::time::Instant::now();
        super::run_current_expression(&mut app.scientific, editor_id);
        let first_latency = started.elapsed();
        assert_eq!(app.scientific.active_script().unwrap().pending_cursor_index, Some("Sys.sleep(1)\n".chars().count()));
        assert!(first_latency < std::time::Duration::from_millis(500), "first caret moved after {first_latency:?}");
        assert!(app.scientific.running, "R must be evaluating Sys.sleep(1)");

        let started_second = std::time::Instant::now();
        super::run_current_expression(&mut app.scientific, editor_id);
        let second_latency = started_second.elapsed();
        assert_eq!(app.scientific.active_script().unwrap().pending_cursor_index, Some("Sys.sleep(1)\nnext_value <- 42\n".chars().count()));
        assert!(second_latency < std::time::Duration::from_millis(500), "second caret moved after {second_latency:?}");
        assert_eq!(app.scientific.execution_queue.len(), 1, "next_value must be queued");
        assert_eq!(app.scientific.execution_queue[0].code, "next_value <- 42");

        wait_until_idle(&mut app);
        assert!(!app.scientific.running);
        assert!(app.scientific.execution_queue.is_empty());
        let result = app.scientific.session.lock().unwrap().execute("stopifnot(next_value == 42)").unwrap();
        assert!(result.error.is_none(), "{:?}", result.error);
    }

    #[test]
    fn console_submit_removes_only_the_newline_inserted_by_enter() {
        let before = "square <- function(x) {\n  x * x\n}";
        let mut pasted = format!("{before}\n");
        remove_console_enter_newline(&mut pasted, before);
        assert_eq!(pasted, "square <- function(x) {\n  x * x\n}");

        let before_with_blank_line = "x <- 1\n";
        let mut pasted_with_blank_line = format!("{before_with_blank_line}\n");
        remove_console_enter_newline(&mut pasted_with_blank_line, before_with_blank_line);
        assert_eq!(pasted_with_blank_line, "x <- 1\n");

        let before_with_cursor_in_middle = "a <- 1\nx <- 2";
        let mut submitted_from_middle = "a <- 1\n\nx <- 2".to_owned();
        remove_console_enter_newline(&mut submitted_from_middle, before_with_cursor_in_middle);
        assert_eq!(submitted_from_middle, before_with_cursor_in_middle);
    }

    #[test]
    fn object_metadata_uses_ascii_separators_and_keeps_full_metadata() {
        let object = crate::r_session::RObject {
            variable: "한글_변수".into(),
            classes: vec!["data.frame".into(), "tbl_df".into(), "tbl".into(), "extra_class".into()],
            data_type: "list".into(),
            dimensions: "4x5".into(),
            length: 5,
            preview: "한글 값".into(),
            category: "Data".into(),
            description: "4 obs. of 5 variables".into(),
            plot_kind: String::new(),
        };
        let before = object.clone();
        assert_eq!(object_metadata_summary("Data", &object.classes, &object.data_type), "Data | data.frame");
        assert_eq!(super::environment_metadata_summary(&object), "4 obs. of 5 variables");
        assert!(super::environment_metadata_tooltip(&object).contains("한글 값"));
        assert_eq!(object_metadata_tooltip(&object.classes, &object.data_type), "Classes: data.frame/tbl_df/tbl/extra_class | Type: list");
        assert_eq!(object, before, "formatting must not alter R metadata");
        assert_eq!(object.variable, "한글_변수");
        assert_eq!(object.preview, "한글 값");
        let plot_classes = vec!["ggplot2::ggplot".to_owned(), "gg".to_owned(), "S7_object".to_owned()];
        assert_eq!(object_metadata_summary("Plot", &plot_classes, "object"), "Plot | ggplot");
        assert_eq!(object_metadata_tooltip(&plot_classes, "object"), "Classes: ggplot2::ggplot/gg/S7_object | Type: object");
        assert_eq!(linked_panel_metadata("plot_A", 42), "plot_A | ID 42");
        assert_eq!(linked_panel_status(true, true), "Connected");
        assert_eq!(linked_panel_status(false, true), "R object is no longer a supported plot | artwork kept");
        assert_eq!(linked_panel_status(false, false), "Missing R variable | artwork kept");
    }

    #[test]
    fn scientific_source_has_no_known_corrupted_display_codepoints() {
        let source = include_str!("scientific.rs");
        for codepoint in [0xCA0C, 0xD69E, 0xFFFD] {
            let Some(corrupt_character) = char::from_u32(codepoint) else { continue };
            assert!(!source.contains(corrupt_character), "unexpected corrupted codepoint U+{codepoint:04X}");
        }
    }

    #[test]
    fn focused_console_keeps_shortcuts_out_of_canvas_routing() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.scientific.enabled = true;
        let ctx = egui::Context::default();
        let input_id = egui::Id::unique("focused-console-input-test");
        app.scientific.console_input_id = Some(input_id);
        ctx.memory_mut(|memory| memory.request_focus(input_id));
        assert!(super::handle_shortcuts(&mut app, &ctx));
    }

    #[test]
    fn clearing_console_transcript_keeps_pending_r_input_and_history() {
        let mut workspace = Workspace::studio_default();
        workspace.console_output = "> fn <- function() {\n".into();
        workspace.console_input = "  1".into();
        workspace.console_pending_lines = vec!["fn <- function() {".into()];
        workspace.console_continuation = true;
        workspace.console_history = vec!["old_command()".into()];
        workspace.console_error = true;
        clear_console_transcript(&mut workspace);
        assert!(workspace.console_output.is_empty());
        assert!(!workspace.console_error);
        assert!(workspace.console_scroll_at_bottom);
        assert_eq!(workspace.console_input, "  1");
        assert_eq!(workspace.console_pending_lines, vec!["fn <- function() {".to_owned()]);
        assert!(workspace.console_continuation);
        assert_eq!(workspace.console_history, vec!["old_command()".to_owned()]);
    }

    #[test]
    fn console_transcript_keeps_prompts_repeated_history_and_restores_the_draft() {
        let mut workspace = Workspace::studio_default();
        append_console_code(&mut workspace, "x <- 1\nx + 1");
        append_console_code(&mut workspace, "x <- 1\nx + 1");
        assert!(workspace.console_history.is_empty(), "editor echoes must not pollute interactive history");
        record_history(&mut workspace, "x <- 1\nx + 1");
        record_history(&mut workspace, "x <- 1\nx + 1");
        workspace.console_input = "draft text".into();
        history_previous(&mut workspace);
        assert_eq!(workspace.console_input, "x <- 1\nx + 1");
        history_previous(&mut workspace);
        assert_eq!(workspace.console_input, "x <- 1\nx + 1");
        history_next(&mut workspace);
        history_next(&mut workspace);
        assert_eq!(workspace.console_input, "draft text");
        assert!(workspace.console_output.contains("> x <- 1\n+ x + 1\n"));
    }

    #[test]
    fn multiline_console_source_is_echoed_with_continuation_prompts() {
        let mut workspace = Workspace::studio_default();
        super::append_console_input(&mut workspace, "square <- function(x) {\n  x * x\n}", "> ");
        assert_eq!(workspace.console_output, "> square <- function(x) {\n+   x * x\n+ }\n");
    }

    #[test]
    fn pasted_multiline_console_expression_executes_once_in_the_persistent_session() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let source = "console_square <- function(x) {\n  x * x\n}\nconsole_square(6)";
        app.scientific.console_input = format!("{source}\n");
        remove_console_enter_newline(&mut app.scientific.console_input, source);
        submit_console(&mut app.scientific);
        wait_until_idle(&mut app);
        assert!(app.scientific.console_output.contains("[1] 36"), "{}", app.scientific.console_output);
        assert_eq!(app.scientific.console_history, vec![source]);
        let result = app.scientific.session.lock().unwrap().execute("stopifnot(console_square(7) == 49)");
        assert!(result.as_ref().is_ok_and(|execution| execution.error.is_none()), "{result:?}");
    }

    #[test]
    fn console_output_is_bounded_for_large_transcripts() {
        let mut workspace = Workspace::studio_default();
        append_console_output(&mut workspace, &"x".repeat(super::CONSOLE_MAX_BYTES + 100_000));
        assert!(workspace.console_output.len() <= super::CONSOLE_MAX_BYTES + 128);
        assert!(workspace.console_output.starts_with("Info: Earlier console output was trimmed.\n"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn r_version_detection_captures_output_from_the_background_process() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let version = super::detect_r().unwrap();
        assert!(version.starts_with("Rscript (R) version"), "{version}");
        assert!(version.contains("Rscript.exe") || version.contains("Rscript"), "{version}");
    }

    #[test]
    fn script_documents_keep_independent_buffers_and_cycle_in_tab_order() {
        let mut workspace = Workspace::studio_default();
        let first_id = workspace.active_script_id;
        if let Some(script) = workspace.active_script_mut() {
            script.text = "shared <- 1".into();
        }
        let second_id = workspace.new_script();
        if let Some(script) = workspace.active_script_mut() {
            script.text = "plot_A <- shared".into();
        }
        workspace.new_script();
        workspace.new_script();
        assert_eq!(workspace.scripts.len(), 4);
        assert_ne!(workspace.scripts[0].id, second_id);
        assert!(workspace.scripts.iter().any(|script| script.id == first_id && script.text == "shared <- 1" && script.is_dirty()));
        assert!(workspace.scripts.iter().any(|script| script.id == second_id && script.text == "plot_A <- shared" && script.is_dirty()));
        workspace.activate_script(first_id);
        workspace.cycle_script(1);
        assert_eq!(workspace.active_script_id, second_id);
        workspace.cycle_script(-1);
        assert_eq!(workspace.active_script_id, first_id);
    }

    #[test]
    fn modified_script_close_requests_confirmation_and_closing_last_opens_a_blank_tab() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let id = app.scientific.active_script_id;
        if let Some(script) = app.scientific.active_script_mut() {
            script.text.push_str("x <- 1\n");
        }
        request_close_script(&mut app, id);
        assert_eq!(app.scientific.pending_close_script_id, Some(id));
        assert!(app.scientific.scripts.iter().any(|script| script.id == id));
        app.scientific.pending_close_script_id = None;
        request_close_script(&mut app, id);
        assert_eq!(app.scientific.scripts.len(), 1);
        app.scientific.close_script_now(id);
        assert_eq!(app.scientific.scripts.len(), 1);
        assert_ne!(app.scientific.active_script_id, id);
        assert_eq!(app.scientific.active_script().map(|script| script.text.as_str()), Some(""));
    }

    #[test]
    fn duplicate_script_paths_are_case_insensitive_on_windows() {
        #[cfg(windows)]
        assert!(super::same_script_path("C:\\work\\analysis.R", "c:/WORK/analysis.r"));
        #[cfg(not(windows))]
        assert!(!super::same_script_path("/work/analysis.R", "/work/analysis.r"));
    }

    #[test]
    fn opening_a_duplicate_path_activates_the_existing_script_instead_of_adding_a_tab() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let contents = std::sync::Arc::new(std::sync::Mutex::new(b"data <- 42\n".to_vec()));
        let reader = contents.clone();
        app.services.read = Some(Box::new(move |_| Ok(reader.lock().unwrap().clone())));
        let path = "C:\\vectorcraft-test-path\\analysis.R";
        open_script_path(&mut app, path);
        assert_eq!(app.scientific.scripts.len(), 2);
        let opened_id = app.scientific.active_script_id;
        assert_eq!(app.scientific.active_script().map(|script| script.text.as_str()), Some("data <- 42\n"));
        app.scientific.new_script();
        let duplicate_path = if cfg!(windows) { "c:/VECTORCRAFT-TEST-PATH/analysis.r" } else { "C:/vectorcraft-test-path/analysis.R" };
        open_script_path(&mut app, duplicate_path);
        assert_eq!(app.scientific.scripts.len(), 3);
        assert_eq!(app.scientific.active_script_id, opened_id);
        let duplicate_path = path.replace('\\', "/");
        open_script_path(&mut app, &duplicate_path);
        assert_eq!(app.scientific.scripts.len(), 3);
        assert_eq!(app.scientific.active_script_id, opened_id);
    }

    #[test]
    fn save_updates_one_document_and_refuses_to_overwrite_external_changes() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let disk = std::sync::Arc::new(std::sync::Mutex::new(b"value <- 1\n".to_vec()));
        let reader = disk.clone();
        app.services.read = Some(Box::new(move |_| Ok(reader.lock().unwrap().clone())));
        let writer = disk.clone();
        app.services.write = Some(Box::new(move |_, bytes| {
            *writer.lock().unwrap() = bytes.to_vec();
            Ok(())
        }));
        let script_id = app.scientific.active_script_id;
        if let Some(script) = app.scientific.active_script_mut() {
            script.path = Some("analysis.R".into());
            script.saved_bytes = Some(b"value <- 1\n".to_vec());
            script.text = "value <- 2\n".into();
        }
        save_script_for(&mut app, script_id, false, false);
        assert_eq!(*disk.lock().unwrap(), b"value <- 2\n");
        assert!(!app.scientific.active_script().is_some_and(super::ScriptDocument::is_dirty));

        *disk.lock().unwrap() = b"external <- TRUE\n".to_vec();
        if let Some(script) = app.scientific.active_script_mut() {
            script.text = "value <- 3\n".into();
        }
        save_script_for(&mut app, script_id, false, false);
        assert_eq!(*disk.lock().unwrap(), b"external <- TRUE\n");
        assert!(app.scientific.active_script().is_some_and(super::ScriptDocument::is_dirty));
        assert!(app.scientific.r_status.contains("changed outside FigureIDE"));
    }

    #[test]
    fn imported_r_scripts_decode_execute_and_round_trip_supported_encodings() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut probe = crate::r_session::RSession::default();
        let Ok(ggplot2) = probe.execute("if (!requireNamespace('ggplot2', quietly=TRUE)) stop('missing ggplot2')") else { return };
        if ggplot2.error.is_some() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let test_directory = std::env::temp_dir().join(format!(
            "vectorcraft-r-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&test_directory).unwrap();
        app.services.read = Some(Box::new(|path| std::fs::read(path).map_err(|error| error.to_string())));
        app.services.write = Some(Box::new(|path, bytes| std::fs::write(path, bytes).map_err(|error| error.to_string())));

        let fixtures = [
            ("utf8-lf", ScriptEncoding::Utf8, ScriptLineEnding::Lf),
            ("utf8-bom-crlf", ScriptEncoding::Utf8Bom, ScriptLineEnding::CrLf),
            ("utf16le-crlf", ScriptEncoding::Utf16Le, ScriptLineEnding::CrLf),
            ("utf16be-lf", ScriptEncoding::Utf16Be, ScriptLineEnding::Lf),
            ("cp949-crlf", ScriptEncoding::WindowsKorean, ScriptLineEnding::CrLf),
            ("cr-only", ScriptEncoding::Utf8, ScriptLineEnding::Cr),
        ];
        for (index, (label, encoding, line_ending)) in fixtures.into_iter().enumerate() {
            let variable = format!("imported_value_{index}");
            let escaped = format!("imported_escaped_{index}");
            let plot = format!("imported_plot_{index}");
            let source = format!(
                "# 한글 주석\n{variable} <- \"한글\"\n`한글 변수` <- \"Unicode identifier\"\nwindows_path <- \"C:\\\\Users\\\\rugha\\\\Documents\\\\plot.R\"\nraw_path <- r\"(C:\\Users\\rugha\\Documents\\plot.R)\"\n{escaped} <- \"say \\\"hi\\\" and \\\\ path\"\n{plot} <- ggplot2::ggplot(data.frame(x=1:3, y=4:6), ggplot2::aes(x, y)) +\n  ggplot2::geom_point()\n"
            );
            let fixture_format = ScriptFormat { encoding, line_ending };
            let bytes = encode_script_fixture(&source, fixture_format);
            let path = test_directory.join(format!("{label}.R")).to_string_lossy().into_owned();
            std::fs::write(&path, bytes).unwrap();

            open_script_path(&mut app, &path);
            if app.scientific.pending_script_decode.is_some() {
                choose_script_encoding(&mut app, encoding);
            }
            let document = app.scientific.active_script().unwrap();
            assert_eq!(document.text, source, "editor buffer for {label}");
            assert_eq!(document.source_format, fixture_format, "detected file format for {label}");
            let id = document.id;
            source_active_script(&mut app.scientific);
            wait_until_idle(&mut app);
            assert!(
                app.scientific.plots.iter().any(|item| item.variable == plot && item.classes.iter().any(|class| class == "ggplot")),
                "Plot Registry for {label}; status: {}; console: {}",
                app.scientific.r_status,
                app.scientific.console_output
            );
            let verification = app
                .scientific
                .session
                .lock()
                .unwrap()
                .execute(&format!(
                    "stopifnot({variable} == '한글', `한글 변수` == 'Unicode identifier', windows_path == 'C:\\\\Users\\\\rugha\\\\Documents\\\\plot.R', raw_path == 'C:\\\\Users\\\\rugha\\\\Documents\\\\plot.R', {escaped} == 'say \\\"hi\\\" and \\\\ path')"
                ));
            assert!(verification.as_ref().is_ok_and(|result| result.error.is_none()), "R execution for {label}: {verification:?}");

            if let Some(document) = app.scientific.scripts.iter_mut().find(|document| document.id == id) {
                document.text.push_str("# round trip\n");
            }
            save_script_for(&mut app, id, false, false);
            let saved_bytes = std::fs::read(&path).unwrap();
            let round_tripped = if encoding == ScriptEncoding::WindowsKorean {
                super::decode_script_as(&saved_bytes, encoding).unwrap().0
            } else {
                super::decode_script_bytes(&saved_bytes).unwrap().0
            };
            assert_eq!(round_tripped, format!("{source}# round trip\n"), "round trip for {label}");
            app.scientific.close_script_now(id);
            open_script_path(&mut app, &path);
            if app.scientific.pending_script_decode.is_some() {
                choose_script_encoding(&mut app, encoding);
            }
            assert_eq!(app.scientific.active_script().unwrap().text, format!("{source}# round trip\n"), "reopened buffer for {label}");
        }
        drop(app);
        std::fs::remove_dir_all(test_directory).unwrap();
    }

    #[test]
    fn malformed_r_script_bytes_are_not_replaced_and_offer_encoding_selection() {
        let bytes = vec![0x61, 0xFF, 0x62];
        assert!(super::decode_script_bytes(&bytes).is_err());
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let contents = std::sync::Arc::new(std::sync::Mutex::new(bytes.clone()));
        let reader = contents.clone();
        app.services.read = Some(Box::new(move |_| Ok(reader.lock().unwrap().clone())));
        let initial_count = app.scientific.scripts.len();
        super::open_script_path(&mut app, "broken.R");
        assert_eq!(app.scientific.scripts.len(), initial_count, "invalid bytes must not populate a script tab");
        assert!(app.scientific.pending_script_decode.is_some());
        assert!(app.scientific.r_status.contains("broken.R") && app.scientific.r_status.contains("byte offset"));
    }

    #[test]
    fn valid_legacy_korean_encoding_requires_an_explicit_selection() {
        let legacy_bytes = encoding_rs::EUC_KR.encode("# 한글 주석\n값 <- 1\n").0.into_owned();
        assert!(super::decode_script_bytes(&legacy_bytes).is_err());
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let file_bytes = std::sync::Arc::new(legacy_bytes);
        let reader = file_bytes.clone();
        app.services.read = Some(Box::new(move |_| Ok(reader.as_ref().clone())));
        super::open_script_path(&mut app, "legacy.R");
        assert!(app.scientific.pending_script_decode.is_some());
        super::choose_script_encoding(&mut app, ScriptEncoding::WindowsKorean);
        assert_eq!(app.scientific.active_script().unwrap().text, "# 한글 주석\n값 <- 1\n");
        assert!(app.scientific.pending_script_decode.is_none());
    }

    #[test]
    fn empty_and_unterminated_r_scripts_import_without_inventing_source_bytes() {
        assert_eq!(super::decode_script_bytes(b"").unwrap().0, "");
        let raw = b"value_without_final_newline <- 7";
        let (decoded, format) = super::decode_script_bytes(raw).unwrap();
        assert_eq!(decoded, "value_without_final_newline <- 7");
        assert_eq!(super::encode_script_bytes(&decoded, format).unwrap(), raw);
    }

    #[test]
    fn crlf_text_is_invalid_to_r_parse_until_the_file_import_normalizes_it() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let raw = b"imported_crlf_value <- 73\r\n";
        let mut session = crate::r_session::RSession::default();
        assert!(matches!(session.parse_input(std::str::from_utf8(raw).unwrap()), Ok(crate::r_session::ParseState::Invalid(_))));
        let (normalized, format) = super::decode_script_bytes(raw).unwrap();
        assert_eq!(format.line_ending, ScriptLineEnding::CrLf);
        assert_eq!(normalized, "imported_crlf_value <- 73\n");
        assert_eq!(session.parse_input(&normalized).unwrap(), crate::r_session::ParseState::Complete);
        assert!(session.execute(&normalized).unwrap().error.is_none());
        assert_eq!(session.execute("imported_crlf_value").unwrap().output.trim(), "[1] 73");
    }

    #[test]
    fn test_r_crlf_file_from_the_reproduction_opens_and_sources_without_token_errors() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut probe = crate::r_session::RSession::default();
        let Ok(installed) = probe.execute("requireNamespace('Seurat', quietly=TRUE)") else { return };
        if !installed.output.contains("TRUE") {
            return;
        }
        let path = std::path::Path::new("test.R");
        if !path.is_file() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.services.read = Some(Box::new(|path| std::fs::read(path).map_err(|error| error.to_string())));
        let path = path.to_string_lossy().into_owned();
        open_script_path(&mut app, &path);
        assert_eq!(app.scientific.active_script().unwrap().text, "library(Seurat)\n");
        source_active_script(&mut app.scientific);
        wait_until_idle(&mut app);
        assert!(!app.scientific.console_output.contains("unexpected invalid token"), "{}", app.scientific.console_output);
        let loaded = app.scientific.session.lock().unwrap().execute("stopifnot('Seurat' %in% loadedNamespaces())");
        assert!(loaded.as_ref().is_ok_and(|result| result.error.is_none()), "Seurat should load from the imported CRLF file: {loaded:?}");
    }

    #[test]
    fn file_import_manual_source_and_native_rscript_produce_the_same_result() {
        let Ok(rscript) = crate::r_session::find_rscript() else { return };
        let directory = std::env::temp_dir().join(format!(
            "vectorcraft-r-differential-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("differential.R");
        let source = "differential_value <- 11\nprint(differential_value + 5)\n";
        std::fs::write(&path, source.as_bytes()).unwrap();
        let native = std::process::Command::new(rscript).arg("--vanilla").arg(&path).output().unwrap();
        assert!(native.status.success(), "native Rscript failed: {}", String::from_utf8_lossy(&native.stderr));
        assert!(String::from_utf8_lossy(&native.stdout).contains("[1] 16"));

        let path_text = path.to_string_lossy().into_owned();
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.services.read = Some(Box::new(|path| std::fs::read(path).map_err(|error| error.to_string())));
        open_script_path(&mut app, &path_text);
        assert_eq!(app.scientific.active_script().unwrap().text, source);
        source_active_script(&mut app.scientific);
        wait_until_idle(&mut app);
        let imported_transcript = app.scientific.console_output.clone();
        assert!(imported_transcript.contains("[1] 16"), "{imported_transcript}");
        let source_marker = "> # Source: ";
        let imported_section = imported_transcript.rsplit(source_marker).next().unwrap();
        let imported_output = imported_section.split_once('\n').map(|(_, output)| output).unwrap();

        app.scientific.new_script();
        if let Some(script) = app.scientific.active_script_mut() {
            script.text = source.to_owned();
        }
        source_active_script(&mut app.scientific);
        wait_until_idle(&mut app);
        let manual_transcript = app.scientific.console_output.clone();
        assert!(manual_transcript.ends_with("[1] 16\n"), "{manual_transcript}");
        let manual_section = manual_transcript.rsplit(source_marker).next().unwrap();
        let manual_output = manual_section.split_once('\n').map(|(_, output)| output).unwrap();
        assert_eq!(imported_output, manual_output);
        let value = app.scientific.session.lock().unwrap().execute("stopifnot(differential_value == 11)");
        assert!(value.as_ref().is_ok_and(|result| result.error.is_none()), "{value:?}");
        drop(app);
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn encode_script_fixture(source: &str, format: super::ScriptFormat) -> Vec<u8> {
        let newline = source.replace('\n', format.line_ending.as_str());
        match format.encoding {
            super::ScriptEncoding::Utf8 => newline.into_bytes(),
            super::ScriptEncoding::Utf8Bom => [vec![0xEF, 0xBB, 0xBF], newline.into_bytes()].concat(),
            super::ScriptEncoding::Utf16Le | super::ScriptEncoding::Utf16Be => {
                let mut bytes = if format.encoding == super::ScriptEncoding::Utf16Le { vec![0xFF, 0xFE] } else { vec![0xFE, 0xFF] };
                for unit in newline.encode_utf16() {
                    bytes.extend_from_slice(&if format.encoding == super::ScriptEncoding::Utf16Le { unit.to_le_bytes() } else { unit.to_be_bytes() });
                }
                bytes
            }
            super::ScriptEncoding::WindowsKorean => encoding_rs::EUC_KR.encode(&newline).0.into_owned(),
        }
    }

    #[test]
    fn source_documents_execute_in_the_same_persistent_r_session() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let first_id = app.scientific.active_script_id;
        if let Some(script) = app.scientific.active_script_mut() {
            script.text = "shared_value <- 41".into();
        }
        run_r(&mut app.scientific, "shared_value <- 41".into(), true);
        wait_until_idle(&mut app);
        let second_id = app.scientific.new_script();
        if let Some(script) = app.scientific.active_script_mut() {
            script.text = "shared_result <- shared_value + 1".into();
        }
        run_r(&mut app.scientific, "shared_result <- shared_value + 1".into(), true);
        wait_until_idle(&mut app);
        assert_ne!(first_id, second_id);
        let result = app
            .scientific
            .session
            .lock()
            .map_err(|error| error.to_string())
            .and_then(|mut session| session.execute("stopifnot(shared_result == 42)"));
        assert!(result.is_ok(), "the second source tab must see values created by the first: {result:?}");
    }

    #[test]
    fn bottom_utility_tabs_have_the_required_order_and_persisted_keys() {
        assert_eq!(BottomTab::ALL.map(BottomTab::label), ["Console", "Environment", "History", "Help", "Packages"]);
        for tab in BottomTab::ALL {
            assert_eq!(BottomTab::from_key(tab.key()), tab);
        }
        assert_eq!(BottomTab::from_key("unknown"), BottomTab::Console);
        assert_eq!(BottomTab::from_key("plots"), BottomTab::Console);
        assert_eq!(BottomTab::from_key("linked"), BottomTab::Console);
        let settings = crate::state::UiState {
            r_studio_tab: BottomTab::Packages.key().into(),
            r_studio_editor_fraction: 0.71,
            r_studio_workspace_width: 512.0,
            ..Default::default()
        };
        let saved = serde_json::to_vec(&settings).unwrap();
        let loaded = crate::state::UiState::read_preferences(&saved).unwrap().ui;
        assert_eq!(loaded.r_studio_tab, "packages");
        assert_eq!(loaded.r_studio_editor_fraction, 0.71);
        assert_eq!(loaded.r_studio_workspace_width, 512.0);
        for old in ["plots", "linked"] {
            let legacy = crate::state::UiState { r_studio_tab: old.into(), ..Default::default() };
            let loaded = crate::state::UiState::read_preferences(&serde_json::to_vec(&legacy).unwrap()).unwrap().ui.sanitized();
            assert_eq!(loaded.r_studio_tab, "console");
            assert_eq!(loaded.dock_tab, crate::state::DockTab::Graph);
        }
    }

    #[test]
    fn studio_workspace_renders_without_reentering_egui_context_input() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.scientific.enabled = true;
        app.session.execute("file.new", &serde_json::json!({"width": 400, "height": 300})).unwrap();
        app.sync_views();
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        for _ in 0..3 {
            app.begin_host_iteration();
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
                app.tick(ui.ctx());
                app.ui(ui);
            });
            output.textures_delta.clear();
        }
        assert!(app.scientific.enabled);
        assert!(!app.ui.status.starts_with("Internal error:"), "{}", app.ui.status);
        let canvas = app.canvas_rect.expect("the existing canvas is present in the split workspace");
        assert!(canvas.width() > 150.0 && canvas.height() > 100.0, "canvas rect: {canvas:?}");
    }

    #[test]
    fn focused_r_editor_ctrl_n_does_not_create_a_vector_document() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.scientific.enabled = true;
        app.run("file.new", serde_json::json!({"width": 400, "height": 300})).unwrap();
        let document_uid = app.session.active().map(|document| document.uid);
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        for _ in 0..3 {
            app.begin_host_iteration();
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
                app.tick(ui.ctx());
                app.ui(ui);
            });
            output.textures_delta.clear();
        }
        let active_id = app.scientific.active_script_id;
        let Some(editor_id) = app.scientific.editor_widget_ids.get(&active_id).copied() else {
            panic!("the active source editor must have a persistent widget ID");
        };
        ctx.memory_mut(|memory| memory.request_focus(editor_id));
        app.begin_host_iteration();
        let event = egui::Event::Key { key: egui::Key::N, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND };
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events: vec![event], ..Default::default() }, |ui| {
            app.tick(ui.ctx());
            app.ui(ui);
        });
        output.textures_delta.clear();
        assert_eq!(app.scientific.scripts.len(), 2);
        assert_eq!(app.session.active().map(|document| document.uid), document_uid);
        let new_id = app.scientific.active_script_id;
        let new_editor_id = app.scientific.editor_widget_ids.get(&new_id).copied();
        assert!(new_editor_id.is_some_and(|id| ctx.memory(|memory| memory.focused()) == Some(id)));
        app.begin_host_iteration();
        let event = egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        };
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events: vec![event], ..Default::default() }, |ui| {
            app.tick(ui.ctx());
            app.ui(ui);
        });
        output.textures_delta.clear();
        assert_eq!(app.scientific.active_script_id, active_id);
        let prior_editor_id = app.scientific.editor_widget_ids.get(&active_id).copied();
        assert!(prior_editor_id.is_some_and(|id| ctx.memory(|memory| memory.focused()) == Some(id)));
    }

    #[test]
    fn workspace_splitter_keeps_the_full_drag_distance_after_pointer_stops() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.scientific.enabled = true;
        app.ui.r_studio_workspace_width = 420.0;
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        let start = egui::pos2(434.0, 400.0);
        let button =
            |pressed| egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        let frame = |app: &mut crate::VectorcraftApp, events| {
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| super::show(app, ui));
            output.textures_delta.clear();
        };

        frame(&mut app, Vec::new());
        frame(&mut app, vec![egui::Event::PointerMoved(start), button(true)]);
        frame(&mut app, vec![egui::Event::PointerMoved(start + egui::vec2(30.0, 0.0))]);
        frame(&mut app, vec![egui::Event::PointerMoved(start + egui::vec2(60.0, 0.0))]);
        frame(&mut app, Vec::new());

        assert_eq!(app.ui.r_studio_workspace_width, 480.0);
        frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: start + egui::vec2(60.0, 0.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
    }

    #[test]
    fn selected_code_is_returned_when_the_range_is_nonempty() {
        assert_eq!(code_for_char_range("x <- 1\ny <- 2", 7, 13), "y <- 2");
    }

    #[test]
    fn an_empty_range_returns_its_current_line_and_handles_unicode() {
        assert_eq!(code_for_char_range("# 쨉\nx <- 1\ny <- 2", 5, 5), "x <- 1");
        assert_eq!(code_for_char_range("x <- 1\ny <- 2", 99, 99), "y <- 2");
    }

    #[test]
    fn workspace_opens_in_studio_layout_by_default() {
        assert!(Workspace::default().enabled);
    }

    #[test]
    fn imported_svg_plot_content_is_pasted_as_editable_art() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", serde_json::json!({"width": 400, "height": 300})).unwrap();
        let imported = app
            .run(
                "clipboard.importSvg",
                serde_json::json!({"svg": "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"20\"><rect width=\"40\" height=\"20\" fill=\"#2670b8\"/></svg>"}),
            )
            .unwrap();
        assert_eq!(imported["count"], 1);
        let pasted = app.run("edit.pasteInPlace", serde_json::json!({})).unwrap();
        assert!(pasted["ids"].as_array().is_some_and(|ids| !ids.is_empty()));
    }

    fn column_plot_svg() -> String {
        let width = super::DEFAULT_PLOT_WIDTH_MM * super::DOC_POINTS_PER_MM;
        let height = super::DEFAULT_PLOT_HEIGHT_MM * super::DOC_POINTS_PER_MM;
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.6}pt\" height=\"{height:.6}pt\" viewBox=\"0 0 {width:.6} {height:.6}\"><rect width=\"{width:.6}\" height=\"{height:.6}\" fill=\"white\"/><circle cx=\"30\" cy=\"30\" r=\"12\" fill=\"#377eb8\"/></svg>"
        )
    }

    #[test]
    fn new_column_plot_aligns_to_a4_left_edge_and_reuses_the_artboard() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", serde_json::json!({"width": "210 mm", "height": "297 mm", "backgroundContents": "white"})).unwrap();
        for label in ["A", "B"] {
            super::apply_rendered_plot(
                &mut app,
                super::RenderedPlot {
                    variable: "column_plot".into(),
                    svg: column_plot_svg(),
                    plots: Vec::new(),
                    action: PanelAction::Create { label: label.into(), width_mm: 70.0, height_mm: 50.0 },
                },
            );
            assert!(app.scientific.r_status.starts_with("Added "), "{}", app.scientific.r_status);
        }
        let state = app.session.doc().unwrap();
        assert_eq!(state.doc.artboards.len(), 1);
        let board = state.doc.artboards[0].rect;
        assert!((board.width() / super::DOC_POINTS_PER_MM - 210.0).abs() < 0.01);
        assert_eq!(app.scientific.linked_panels.len(), 2);
        let first = &app.scientific.linked_panels[0];
        let panel = state.doc.bounds_of(&[vectorcraft_doc::NodeId(first.panel_id)], false).unwrap();
        let graphic = state.doc.bounds_of(&[vectorcraft_doc::NodeId(first.generated_id)], false).unwrap();
        assert!((panel.x0 - board.x0).abs() < 0.1, "panel left {panel:?}, board {board:?}");
        assert!((graphic.width() / super::DOC_POINTS_PER_MM - 70.0).abs() < 0.1, "graphic width: {graphic:?}");
        assert!(state.selection.contains(vectorcraft_doc::NodeId(app.scientific.linked_panels[1].panel_id)));
        assert!(app.scientific.linked_panels.iter().all(|panel| panel.small_figure_typography));
        let first_id = app.scientific.linked_panels[0].panel_id;
        let second_id = app.scientific.linked_panels[1].panel_id;
        app.run("edit.undo", serde_json::json!({})).unwrap();
        let undone = app.session.doc().unwrap();
        assert!(undone.doc.node(vectorcraft_doc::NodeId(first_id)).is_some());
        assert!(undone.doc.node(vectorcraft_doc::NodeId(second_id)).is_none(), "one undo should remove only the second insertion");
        assert_eq!(undone.doc.artboards.len(), 1);
        app.run("edit.redo", serde_json::json!({})).unwrap();
        assert!(app.session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(second_id)).is_some());
        app.scientific.linked_panels.clear();
        super::restore_linked_panels(&mut app);
        assert_eq!(app.scientific.linked_panels.len(), 2);
        assert!(app.scientific.linked_panels.iter().all(|panel| panel.small_figure_typography));
        let old_name = format!("RVS_PANEL|v2|A|{}|177.800000|127.000000|reflow", super::encode_name("column_plot"));
        app.run("object.setName", serde_json::json!({"id": first_id, "name": old_name})).unwrap();
        app.scientific.linked_panels.clear();
        super::restore_linked_panels(&mut app);
        let old = app.scientific.linked_panels.iter().find(|panel| panel.panel_id == first_id).unwrap();
        assert_eq!((old.width_mm, old.height_mm), (177.8, 127.0));
        assert!(!old.small_figure_typography);
    }

    #[test]
    fn completed_plot_render_cannot_cross_documents_or_link_identities() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", serde_json::json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        super::apply_rendered_plot(
            &mut app,
            super::RenderedPlot {
                variable: "plot_A".into(),
                svg: column_plot_svg(),
                plots: Vec::new(),
                action: PanelAction::Create { label: "A".into(), width_mm: 70.0, height_mm: 50.0 },
            },
        );
        let panel = app.scientific.linked_panels[0].clone();
        app.scientific.pending_refresh.push_back(panel.clone());
        app.scientific.linked_panels[0].width_mm = 100.0;
        let queued = super::take_next_refresh(&mut app.scientific, panel.document_uid).unwrap();
        assert_eq!(queued.width_mm, 100.0, "Update All must use the panel's current dimensions");
        app.scientific.linked_panels[0].width_mm = panel.width_mm;
        let action = PanelAction::Refresh { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm: 70.0, height_mm: 50.0 };
        let rendered = |action: PanelAction| Ok(super::RenderedPlot { variable: "plot_A".into(), svg: column_plot_svg(), plots: Vec::new(), action });
        let original_document = app.session.doc().unwrap().doc.clone();
        let wrong_child = PanelAction::Refresh { panel_id: panel.panel_id, generated_id: panel.generated_id + 1, width_mm: 70.0, height_mm: 50.0 };
        app.scientific._tx.send(RMessage::Rendered(panel.document_uid, 0, "plot_A".into(), wrong_child.clone(), rendered(wrong_child))).unwrap();
        app.scientific._tx.send(RMessage::Rendered(panel.document_uid, 0, "other_plot".into(), action.clone(), rendered(action.clone()))).unwrap();
        poll_messages(&mut app);
        assert_eq!(app.session.doc().unwrap().doc, original_document);

        app.run("object.move", serde_json::json!({"ids": [panel.panel_id], "dx": 3, "dy": 0})).unwrap();
        app.run("edit.undo", serde_json::json!({})).unwrap();
        assert_eq!(app.scientific.render_epoch.get(&panel.document_uid), Some(&1));
        let after_undo = app.session.doc().unwrap().doc.clone();
        app.scientific._tx.send(RMessage::Rendered(panel.document_uid, 0, "plot_A".into(), action.clone(), rendered(action.clone()))).unwrap();
        poll_messages(&mut app);
        assert_eq!(app.session.doc().unwrap().doc, after_undo);

        app.run("file.new", serde_json::json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        let other_uid = app.session.active().unwrap().uid;
        assert_ne!(panel.document_uid, other_uid);
        let other_document = app.session.doc().unwrap().doc.clone();
        app.scientific.pending_refresh.push_back(panel.clone());
        super::start_next_refresh(&mut app);
        assert!(!app.scientific.running);
        assert_eq!(app.scientific.pending_refresh.len(), 1, "another document's refresh must remain queued");
        app.scientific._tx.send(RMessage::Rendered(panel.document_uid, 0, "plot_A".into(), action.clone(), rendered(action.clone()))).unwrap();
        poll_messages(&mut app);
        assert_eq!(app.session.doc().unwrap().doc, other_document);
    }

    #[test]
    fn completed_plot_render_can_create_the_first_document() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        assert!(app.session.active().is_none());
        let action = PanelAction::Create { label: "A".into(), width_mm: 70.0, height_mm: 50.0 };
        app.scientific
            ._tx
            .send(RMessage::Rendered(
                0,
                0,
                "plot_A".into(),
                action.clone(),
                Ok(super::RenderedPlot { variable: "plot_A".into(), svg: column_plot_svg(), plots: Vec::new(), action }),
            ))
            .unwrap();
        poll_messages(&mut app);
        assert_eq!(app.scientific.linked_panels.len(), 1, "{}", app.scientific.r_status);
        assert_eq!(app.session.active().unwrap().uid, app.scientific.linked_panels[0].document_uid);
    }

    #[test]
    fn advanced_graph_style_restores_from_the_linked_panel_name() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", serde_json::json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        super::apply_rendered_plot(
            &mut app,
            super::RenderedPlot {
                variable: "plot_A".into(),
                svg: column_plot_svg(),
                plots: Vec::new(),
                action: PanelAction::Create { label: "A".into(), width_mm: 70.0, height_mm: 50.0 },
            },
        );
        let mut panel = app.scientific.linked_panels[0].clone();
        panel.graph_style = super::graph_style::GraphStyle {
            title_details: super::graph_style::TextDetails {
                family: Some("Noto Sans KR".into()),
                face: Some(super::graph_style::FontFace::Bold),
                colour: Some("#335577".into()),
                ..Default::default()
            },
            minor_grid_x_visible: Some(false),
            panel_background: Some("ivory".into()),
            plot_margin_mm: Some([2.0, 3.0, 4.0, 5.0]),
            ..Default::default()
        };
        let name = super::persistent_panel_name(&panel, panel.width_mm, panel.height_mm, super::ResizeMode::Reflow);
        app.run("object.setName", serde_json::json!({"id": panel.panel_id, "name": name})).unwrap();
        app.scientific.linked_panels.clear();
        super::restore_linked_panels(&mut app);
        assert_eq!(app.scientific.linked_panels[0].graph_style, panel.graph_style);
    }

    #[test]
    fn inserting_into_no_document_creates_one_white_a4_artboard_and_selects_the_plot() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        super::apply_rendered_plot(
            &mut app,
            super::RenderedPlot {
                variable: "column_plot".into(),
                svg: column_plot_svg(),
                plots: Vec::new(),
                action: PanelAction::Create { label: "A".into(), width_mm: 70.0, height_mm: 50.0 },
            },
        );
        assert!(app.scientific.r_status.starts_with("Added "), "{}", app.scientific.r_status);
        let state = app.session.doc().unwrap();
        assert_eq!(state.doc.artboards.len(), 1);
        let board = state.doc.artboards[0].rect;
        assert!((board.width() / super::DOC_POINTS_PER_MM - 210.0).abs() < 0.01);
        assert!((board.height() / super::DOC_POINTS_PER_MM - 297.0).abs() < 0.01);
        assert_eq!(state.doc.setup.background, vectorcraft_doc::Background::White);
        let panel = &app.scientific.linked_panels[0];
        let panel_id = panel.panel_id;
        assert!(state.selection.contains(vectorcraft_doc::NodeId(panel.panel_id)));
        assert!((state.doc.bounds_of(&[vectorcraft_doc::NodeId(panel.panel_id)], false).unwrap().x0 - board.x0).abs() < 0.1);
        app.run("edit.undo", serde_json::json!({})).unwrap();
        let undone = app.session.doc().unwrap();
        assert!(undone.doc.node(vectorcraft_doc::NodeId(panel_id)).is_none());
        // file.new establishes the first artboard as the new document's base state.
        // The engine keeps that base artboard when the grouped plot edit is undone.
        assert_eq!(undone.doc.artboards.len(), 1);
        app.run("edit.redo", serde_json::json!({})).unwrap();
        let redone = app.session.doc().unwrap();
        assert_eq!(redone.doc.artboards.len(), 1);
        assert!(redone.doc.node(vectorcraft_doc::NodeId(panel_id)).is_some());
    }

    #[test]
    fn invalid_plot_svg_does_not_create_an_artboard() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        super::apply_rendered_plot(
            &mut app,
            super::RenderedPlot {
                variable: "bad_plot".into(),
                svg: "not svg".into(),
                plots: Vec::new(),
                action: PanelAction::Create { label: "A".into(), width_mm: 70.0, height_mm: 50.0 },
            },
        );
        assert!(app.scientific.r_status.starts_with("Could not add"));
        assert!(app.scientific.linked_panels.is_empty());
        assert!(app.session.active().is_none_or(|state| state.doc.artboards.is_empty()));
    }

    #[test]
    fn multiple_linked_artboard_instances_keep_distinct_panel_ids_for_one_plot() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", serde_json::json!({"width": 400, "height": 300})).unwrap();
        let script = "plot_A <- ggplot2::ggplot(mtcars, ggplot2::aes(wt, mpg)) + ggplot2::geom_point()";
        run_r(&mut app.scientific, script.into(), false);
        wait_until_idle(&mut app);
        for label in ["A", "B"] {
            render_plot(
                &mut app.scientific,
                app.session.active().unwrap().uid,
                "plot_A".into(),
                PanelAction::Create { label: label.into(), width_mm: 177.8, height_mm: 127.0 },
            );
            wait_until_idle(&mut app);
        }
        let links: Vec<_> = app.scientific.linked_panels.iter().filter(|panel| panel.r_variable == "plot_A").collect();
        assert_eq!(links.len(), 2);
        assert_ne!(links[0].panel_id, links[1].panel_id);
        for link in links {
            assert!(app.session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(link.panel_id)).is_some());
        }
    }

    #[test]
    fn real_ggplot_registry_entry_renders_and_pastes_as_graph_not_source() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", serde_json::json!({"width": 600, "height": 400})).unwrap();
        let script = "library(ggplot2)\ndf <- mtcars\ncommon_theme <- theme_classic(base_size = 11)\nplot_A <- ggplot(df, aes(wt, mpg)) + geom_point(color = '#377EB8', size = 3) + common_theme\nplot_B <- ggplot(df, aes(factor(cyl), mpg)) + geom_boxplot(fill = '#4DAF4A') + common_theme\nplot_C <- ggplot(df, aes(factor(gear), hp)) + geom_violin(fill = '#984EA3') + common_theme\nplot_D <- ggplot(df, aes(wt, hp)) + geom_point(color = '#E41A1C') + geom_smooth(method = 'lm', se = FALSE) + common_theme";
        if let Some(document) = app.scientific.active_script_mut() {
            document.text = script.into();
        }
        run_r(&mut app.scientific, script.into(), true);
        wait_until_idle(&mut app);
        assert!(app.scientific.plots.iter().any(|plot| plot.variable == "plot_A" && plot.classes.contains(&"ggplot".to_owned())));
        assert_eq!(app.scientific.plots.len(), 4, "one shared script discovers all four named plots");
        let document_uid = app.session.active().unwrap().uid;
        for (index, variable) in ["plot_A", "plot_B", "plot_C", "plot_D"].into_iter().enumerate() {
            render_plot(
                &mut app.scientific,
                app.session.active().unwrap().uid,
                variable.into(),
                PanelAction::Create {
                    label: super::panel_label(index),
                    width_mm: super::DEFAULT_PLOT_WIDTH_MM,
                    height_mm: super::DEFAULT_PLOT_HEIGHT_MM,
                },
            );
            wait_until_idle(&mut app);
        }
        assert_eq!(app.scientific.linked_panels.len(), 4);
        let original_ids: Vec<_> = app.scientific.linked_panels.iter().map(|panel| panel.panel_id).collect();
        let original_labels: Vec<_> = app.scientific.linked_panels.iter().map(|panel| panel.label.clone()).collect();
        assert_eq!(original_labels, ["A", "B", "C", "D"]);
        let panel_a = app.scientific.linked_panels[0].clone();
        let panel_b = app.scientific.linked_panels[1].clone();
        render_plot(
            &mut app.scientific,
            panel_a.document_uid,
            "plot_A".into(),
            PanelAction::Reflow { panel_id: panel_a.panel_id, generated_id: panel_a.generated_id, width_mm: 100.0, height_mm: 80.0 },
        );
        wait_until_idle(&mut app);
        let reflowed = app.scientific.linked_panels.iter().find(|panel| panel.panel_id == panel_a.panel_id).unwrap();
        assert_eq!((reflowed.width_mm, reflowed.height_mm), (100.0, 80.0));
        let plot_bounds = app.session.doc().unwrap().doc.oriented_bounds(&[vectorcraft_doc::NodeId(reflowed.generated_id)], false).unwrap().rect;
        assert!(((plot_bounds.x1 - plot_bounds.x0) / super::DOC_POINTS_PER_MM - 100.0).abs() < 0.01);
        assert!(((plot_bounds.y1 - plot_bounds.y0) / super::DOC_POINTS_PER_MM - 80.0).abs() < 0.01);
        let panel_a_before = app.session.doc().unwrap().doc.bounds_of(&[vectorcraft_doc::NodeId(panel_a.panel_id)], false).unwrap();
        app.run("object.move", serde_json::json!({"ids": [panel_b.panel_id], "dx": 35, "dy": 22})).unwrap();
        app.run("object.scale", serde_json::json!({"ids": [panel_b.panel_id], "sx": 115, "sy": 110})).unwrap();
        let panel_b_before = app.session.doc().unwrap().doc.oriented_bounds(&[vectorcraft_doc::NodeId(panel_b.panel_id)], false).unwrap();
        app.run("select.set", serde_json::json!({"ids": [panel_b.panel_id]})).unwrap();
        super::update_selected_panel(&mut app);
        wait_until_idle(&mut app);
        assert_eq!(app.scientific.linked_panels.len(), 4, "Add/Update never duplicates an existing panel");
        let annotation = app.run("text.create", serde_json::json!({"x": 550, "y": 390, "text": "Figure note"})).unwrap()["id"].as_u64().unwrap();

        let updated_script = script.replace("fill = '#4DAF4A'", "fill = '#00A0C0'");
        if let Some(document) = app.scientific.active_script_mut() {
            document.text = updated_script.clone();
        }
        run_r(&mut app.scientific, updated_script, true);
        wait_until_idle(&mut app);
        assert_eq!(app.scientific.linked_panels.len(), 4, "Run Script refreshes existing links without duplicating panels");
        assert_eq!(app.scientific.linked_panels.iter().map(|panel| panel.panel_id).collect::<Vec<_>>(), original_ids);
        assert_rect_close(app.session.doc().unwrap().doc.bounds_of(&[vectorcraft_doc::NodeId(panel_a.panel_id)], false).unwrap(), panel_a_before);
        let panel_b_after = app.session.doc().unwrap().doc.oriented_bounds(&[vectorcraft_doc::NodeId(panel_b.panel_id)], false).unwrap();
        assert!((panel_b_after.angle - panel_b_before.angle).abs() < 1e-8);
        assert_rect_close(panel_b_after.rect, panel_b_before.rect);
        assert!(
            app.session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(annotation)).is_some(),
            "ordinary annotations outside linked panels remain intact"
        );

        update_all_panels(&mut app);
        wait_until_idle(&mut app);
        assert_eq!(app.scientific.linked_panels.len(), 4);
        let panel_a_bounds = app.session.doc().unwrap().doc.bounds_of(&[vectorcraft_doc::NodeId(panel_a.panel_id)], false).unwrap();
        run_r(&mut app.scientific, "plot_A <- list(no_longer_a_ggplot = TRUE)".into(), false);
        wait_until_idle(&mut app);
        render_plot(
            &mut app.scientific,
            panel_a.document_uid,
            "plot_A".into(),
            PanelAction::Refresh {
                panel_id: panel_a.panel_id,
                generated_id: panel_a.generated_id,
                width_mm: panel_a.width_mm,
                height_mm: panel_a.height_mm,
            },
        );
        wait_until_idle(&mut app);
        assert_rect_close(app.session.doc().unwrap().doc.bounds_of(&[vectorcraft_doc::NodeId(panel_a.panel_id)], false).unwrap(), panel_a_bounds);
        assert!(app.scientific.r_status.contains("not a supported plot"), "{}", app.scientific.r_status);
        assert_eq!(app.scientific.linked_panels.iter().filter(|panel| panel.document_uid == document_uid).count(), 4);

        let mut paths = 0usize;
        let mut text = Vec::new();
        app.session.doc().unwrap().doc.walk(|node| match &node.kind {
            vectorcraft_doc::NodeKind::Path { .. } | vectorcraft_doc::NodeKind::Compound { .. } => paths += 1,
            vectorcraft_doc::NodeKind::Text(value) => text.push(value.plain_text().to_owned()),
            _ => {}
        });
        assert!(paths > 10, "rendered plot should import vector marks, axes, and labels");
        assert!(text.iter().all(|label| !label.contains("ggplot") && !label.contains("plot_A <-")));
        assert!(!app.scientific.r_status.contains("raw SVG"));
    }

    #[test]
    fn pheatmap_registry_link_refresh_and_reflow_keep_vector_panel() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", serde_json::json!({"width": 600, "height": 400})).unwrap();
        let source = "if (!requireNamespace('pheatmap', quietly=TRUE)) stop('missing pheatmap')\nmat <- matrix(seq(-2, 2, length.out=100), 10)\nheatmap_plot <- pheatmap::pheatmap(mat, silent=TRUE, color=grDevices::colorRampPalette(c('blue','white','red'))(20))";
        run_r(&mut app.scientific, source.into(), false);
        wait_until_idle(&mut app);
        if app.scientific.console_error && app.scientific.r_status.contains("missing pheatmap") {
            return;
        }
        assert!(!app.scientific.console_error, "{}", app.scientific.r_status);
        assert!(app.scientific.plots.iter().any(|object| object.variable == "heatmap_plot" && object.plot_kind == "pheatmap"));
        render_plot(
            &mut app.scientific,
            app.session.active().unwrap().uid,
            "heatmap_plot".into(),
            PanelAction::Create { label: "A".into(), width_mm: 140.0, height_mm: 100.0 },
        );
        wait_until_idle(&mut app);
        assert_eq!(app.scientific.linked_panels.len(), 1, "{}", app.scientific.r_status);
        let panel = app.scientific.linked_panels[0].clone();
        render_plot(
            &mut app.scientific,
            panel.document_uid,
            "heatmap_plot".into(),
            PanelAction::Reflow { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm: 180.0, height_mm: 110.0 },
        );
        wait_until_idle(&mut app);
        let reflowed = &app.scientific.linked_panels[0];
        assert_eq!((reflowed.panel_id, reflowed.width_mm, reflowed.height_mm), (panel.panel_id, 180.0, 110.0));
        app.run("select.set", serde_json::json!({"ids": [panel.panel_id]})).unwrap();
        super::update_selected_panel(&mut app);
        wait_until_idle(&mut app);
        assert_eq!(app.scientific.linked_panels.len(), 1);
        assert_eq!(app.scientific.linked_panels[0].panel_id, panel.panel_id);
        let mut paths = 0usize;
        app.session.doc().unwrap().doc.walk(|node| {
            if matches!(&node.kind, vectorcraft_doc::NodeKind::Path { .. } | vectorcraft_doc::NodeKind::Compound { .. }) {
                paths += 1;
            }
        });
        assert!(paths > 10, "pheatmap must remain editable vector artwork");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn original_airway_pca_and_ma_plots_keep_independent_graph_panels() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", serde_json::json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        let original = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../airway_complete_rnaseq_analysis.R")
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let execution = app.scientific.session.lock().unwrap().execute(&format!("setwd(tempdir()); source('{original}', local=.GlobalEnv)")).unwrap();
        assert!(execution.error.is_none(), "original airway script failed: {:?}", execution.error);
        app.scientific.plots = execution.objects.iter().filter(|object| !object.plot_kind.is_empty()).cloned().collect();
        for (control, style) in [
            ("axis width", super::graph_style::GraphStyle { axis_x_line_mm: Some(1.0), ..Default::default() }),
            ("axis visibility", super::graph_style::GraphStyle { axis_x_visible: Some(true), ..Default::default() }),
            ("grid visibility", super::graph_style::GraphStyle { major_grid_visible: Some(true), ..Default::default() }),
            ("title font", super::graph_style::GraphStyle { plot_title_pt: Some(14.0), ..Default::default() }),
            ("legend title", super::graph_style::GraphStyle { legend_title_visible: Some(false), ..Default::default() }),
            ("legend font", super::graph_style::GraphStyle { legend_text_pt: Some(12.0), ..Default::default() }),
            (
                "inside legend",
                super::graph_style::GraphStyle {
                    legend_position: Some(super::graph_style::LegendPosition::Inside),
                    inside_x: Some(0.6),
                    inside_y: Some(0.6),
                    ..Default::default()
                },
            ),
            ("legend key", super::graph_style::GraphStyle { legend_key_mm: Some(6.0), ..Default::default() }),
        ] {
            let result = app.scientific.session.lock().unwrap().render_plot_svg_with_graph_style("ma_plot", 70.0, 50.0, true, &style);
            assert!(result.is_ok(), "MA {control}: {result:?}");
        }
        for variable in ["pca_plot", "ma_plot"] {
            let plot = app.scientific.plots.iter().find(|plot| plot.variable == variable).unwrap();
            assert_eq!(plot.plot_kind, "ggplot", "{variable}: {:?}", plot.classes);
            render_plot(
                &mut app.scientific,
                app.session.active().unwrap().uid,
                variable.into(),
                PanelAction::Create { label: variable.into(), width_mm: 70.0, height_mm: 50.0 },
            );
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
            while app.scientific.running && std::time::Instant::now() < deadline {
                poll_messages(&mut app);
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            poll_messages(&mut app);
            assert!(!app.scientific.running, "{variable} render timed out");
            assert!(app.scientific.r_status.starts_with("Added "), "{variable}: {}", app.scientific.r_status);
        }
        assert_eq!(app.scientific.linked_panels.len(), 2);
        let pca = app.scientific.linked_panels.iter().find(|panel| panel.r_variable == "pca_plot").unwrap().clone();
        let ma = app.scientific.linked_panels.iter().find(|panel| panel.r_variable == "ma_plot").unwrap().clone();
        assert_ne!(pca.panel_id, ma.panel_id);
        let ma_position = app.session.doc().unwrap().doc.position(vectorcraft_doc::NodeId(ma.panel_id));
        let pca_style = super::graph_style::GraphStyle {
            plot_title_pt: Some(16.0),
            title_details: super::graph_style::TextDetails { colour: Some("#BB1122".into()), ..Default::default() },
            ..Default::default()
        };
        super::request_graph_apply(&mut app.scientific, &pca, pca_style.clone(), 70.0, 50.0).unwrap();
        super::start_next_graph(&mut app);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while app.scientific.running && std::time::Instant::now() < deadline {
            poll_messages(&mut app);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        poll_messages(&mut app);
        assert_eq!(app.scientific.linked_panels.iter().find(|panel| panel.panel_id == pca.panel_id).unwrap().graph_style, pca_style);
        let ma_style = super::graph_style::GraphStyle {
            tick_x_visible: Some(false),
            legend_position: Some(super::graph_style::LegendPosition::Bottom),
            major_grid_x_visible: Some(false),
            panel_border_visible: Some(true),
            panel_border_colour: Some("#336699".into()),
            ..Default::default()
        };
        super::request_graph_apply(&mut app.scientific, &ma, ma_style.clone(), 100.0, 80.0).unwrap();
        super::start_next_graph(&mut app);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while app.scientific.running && std::time::Instant::now() < deadline {
            poll_messages(&mut app);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        poll_messages(&mut app);
        let updated_ma = app.scientific.linked_panels.iter().find(|panel| panel.panel_id == ma.panel_id).unwrap();
        assert_eq!(updated_ma.graph_style, ma_style, "MA Graph status: {}", app.scientific.r_status);
        assert_eq!((updated_ma.width_mm, updated_ma.height_mm), (100.0, 80.0));
        assert_eq!(app.session.doc().unwrap().doc.position(vectorcraft_doc::NodeId(ma.panel_id)), ma_position);
        let frame = app.session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(ma.generated_id)).unwrap().visual_bounds().unwrap();
        assert!((frame.width() / super::DOC_POINTS_PER_MM - 100.0).abs() < 0.05);
        assert!((frame.height() / super::DOC_POINTS_PER_MM - 80.0).abs() < 0.05);
        assert_eq!(app.scientific.linked_panels.iter().find(|panel| panel.panel_id == pca.panel_id).unwrap().graph_style, pca_style);
        let pca_art = app.session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(pca.generated_id)).unwrap().clone();
        let changed = app.scientific.session.lock().unwrap().execute("ma_plot <- ma_plot + ggplot2::labs(subtitle='Updated in R')").unwrap();
        assert!(changed.error.is_none(), "{:?}", changed.error);
        app.run("select.set", serde_json::json!({"ids": [ma.panel_id]})).unwrap();
        super::update_selected_panel(&mut app);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while app.scientific.running && std::time::Instant::now() < deadline {
            poll_messages(&mut app);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        poll_messages(&mut app);
        let updated_ma = app.scientific.linked_panels.iter().find(|panel| panel.panel_id == ma.panel_id).unwrap();
        assert_eq!((updated_ma.width_mm, updated_ma.height_mm, &updated_ma.graph_style), (100.0, 80.0, &ma_style));
        assert_eq!(app.session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(pca.generated_id)), Some(&pca_art));
        let last_valid_ma_art = app.session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(ma.generated_id)).unwrap().clone();
        let removed = app.scientific.session.lock().unwrap().execute("rm(ma_plot)").unwrap();
        assert!(removed.error.is_none(), "{:?}", removed.error);
        super::request_graph_apply(&mut app.scientific, &ma, super::graph_style::GraphStyle { plot_title_pt: Some(18.0), ..ma_style }, 100.0, 80.0)
            .unwrap();
        super::start_next_graph(&mut app);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while app.scientific.running && std::time::Instant::now() < deadline {
            poll_messages(&mut app);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        poll_messages(&mut app);
        assert!(app.scientific.r_status.contains("failed"), "{}", app.scientific.r_status);
        assert_eq!(app.session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(ma.generated_id)), Some(&last_valid_ma_art));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn original_airway_ma_plot_alone_reflows_at_committed_size() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", serde_json::json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        let original = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../airway_complete_rnaseq_analysis.R")
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let execution = app.scientific.session.lock().unwrap().execute(&format!("setwd(tempdir()); source('{original}', local=.GlobalEnv)")).unwrap();
        assert!(execution.error.is_none(), "original airway script failed: {:?}", execution.error);
        app.scientific.plots = execution.objects.iter().filter(|object| !object.plot_kind.is_empty()).cloned().collect();
        let plot = app.scientific.plots.iter().find(|plot| plot.variable == "ma_plot").unwrap();
        assert_eq!(plot.plot_kind, "ggplot");
        let snapshot = app.scientific.session.lock().unwrap().execute(".rvs_ma_before <- serialize(ma_plot, NULL)").unwrap();
        assert!(snapshot.error.is_none(), "{:?}", snapshot.error);
        render_plot(
            &mut app.scientific,
            app.session.active().unwrap().uid,
            "ma_plot".into(),
            PanelAction::Create { label: "A".into(), width_mm: 70.0, height_mm: 50.0 },
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while app.scientific.running && std::time::Instant::now() < deadline {
            poll_messages(&mut app);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        poll_messages(&mut app);
        assert_eq!(app.scientific.linked_panels.len(), 1, "{}", app.scientific.r_status);
        let panel = app.scientific.linked_panels[0].clone();
        let style = super::graph_style::GraphStyle {
            plot_title_pt: Some(15.0),
            legend_position: Some(super::graph_style::LegendPosition::Bottom),
            ..Default::default()
        };
        super::request_graph_apply(&mut app.scientific, &panel, style.clone(), 100.0, 80.0).unwrap();
        super::start_next_graph(&mut app);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while app.scientific.running && std::time::Instant::now() < deadline {
            poll_messages(&mut app);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        poll_messages(&mut app);
        let updated = &app.scientific.linked_panels[0];
        assert_eq!((updated.panel_id, updated.r_variable.as_str()), (panel.panel_id, "ma_plot"));
        assert_eq!((updated.width_mm, updated.height_mm, &updated.graph_style), (100.0, 80.0, &style));
        let frame = app.session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(panel.generated_id)).unwrap().visual_bounds().unwrap();
        assert!((frame.width() / super::DOC_POINTS_PER_MM - 100.0).abs() < 0.05);
        assert!((frame.height() / super::DOC_POINTS_PER_MM - 80.0).abs() < 0.05);
        let unchanged = app.scientific.session.lock().unwrap().execute("stopifnot(identical(.rvs_ma_before, serialize(ma_plot, NULL)))").unwrap();
        assert!(unchanged.error.is_none(), "Graph rendering modified ma_plot in .GlobalEnv: {:?}", unchanged.error);
        app.run("object.scale", serde_json::json!({"ids": [panel.panel_id], "sx": 120, "sy": 112.5})).unwrap();
        let resized_style = super::graph_style::GraphStyle { tick_x_visible: Some(false), ..style };
        super::request_graph_apply(&mut app.scientific, &panel, resized_style.clone(), 120.0, 90.0).unwrap();
        super::start_next_graph(&mut app);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while app.scientific.running && std::time::Instant::now() < deadline {
            poll_messages(&mut app);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        poll_messages(&mut app);
        assert_eq!(
            (app.scientific.linked_panels[0].width_mm, app.scientific.linked_panels[0].height_mm),
            (120.0, 90.0),
            "{}",
            app.scientific.r_status
        );
        assert_eq!(app.scientific.linked_panels[0].graph_style, resized_style);
        let unchanged = app.scientific.session.lock().unwrap().execute("stopifnot(identical(.rvs_ma_before, serialize(ma_plot, NULL)))").unwrap();
        assert!(unchanged.error.is_none(), "Reflow modified ma_plot in .GlobalEnv: {:?}", unchanged.error);
    }

    fn wait_until_idle(app: &mut crate::VectorcraftApp) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while (app.scientific.running || !app.scientific.execution_queue.is_empty()) && std::time::Instant::now() < deadline {
            poll_messages(app);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        poll_messages(app);
        assert!(!app.scientific.running, "R worker timed out: {}", app.scientific.r_status);
        assert!(app.scientific.execution_queue.is_empty(), "R queue timed out: {} items remain", app.scientific.execution_queue.len());
    }

    #[test]
    fn current_expression_steps_multiline_semicolons_comments_and_eof_without_errors() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let source = "first_val <- 10 +\r\n  # comment inside multiline\r\n  5\r\na <- 2; b <- 3\r\n# trailing comment\r\n\r\n";
        app.scientific.active_script_mut().unwrap().text = source.into();
        let ctx = egui::Context::default();
        let editor_id = egui::Id::unique("multiline-semicolon-eof-test");
        let mut state = egui::text_edit::TextEditState::default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(0))));
        state.store(&ctx, editor_id);
        app.scientific.repaint_ctx = Some(ctx.clone());
        app.scientific.editor_widget_ids.insert(1, editor_id);

        // Step 1: multiline expression with CRLF and embedded comment advances immediately to `a <- 2; b <- 3`.
        super::run_current_expression(&mut app.scientific, editor_id);
        wait_until_idle(&mut app);
        let expected_script = "first_val <- 10 +\n  # comment inside multiline\n  5\na <- 2; b <- 3\n# trailing comment\n\n";
        assert_eq!(app.scientific.active_script().unwrap().text, expected_script);
        let second_expr_offset = "first_val <- 10 +\n  # comment inside multiline\n  5\n".chars().count();
        assert_eq!(
            egui::TextEdit::load_state(&ctx, editor_id).and_then(|s| s.cursor.char_range()).map(|r| usize::from(r.primary.index)),
            Some(second_expr_offset),
            "navigate_source must update TextEditState immediately without requiring a UI frame"
        );
        assert!(!app.scientific.console_error, "status: {} | console: {}", app.scientific.r_status, app.scientific.console_output);

        // Step 2: semicolon-separated expression on one line advances past that line instead of trapping the caret.
        super::run_current_expression(&mut app.scientific, editor_id);
        wait_until_idle(&mut app);
        assert_eq!(
            egui::TextEdit::load_state(&ctx, editor_id).and_then(|s| s.cursor.char_range()).map(|r| usize::from(r.primary.index)),
            Some(expected_script.chars().count())
        );
        assert!(!app.scientific.console_error, "status: {} | console: {}", app.scientific.r_status, app.scientific.console_output);

        // Step 3: pressing Ctrl+Enter on trailing comments/blank lines or at EOF is a clean no-op without setting console_error.
        let cursor_on_trailing_comment = "first_val <- 10 +\n  # comment inside multiline\n  5\na <- 2; b <- 3\n".chars().count();
        navigate_source(&mut app.scientific, 1, cursor_on_trailing_comment);
        super::run_current_expression(&mut app.scientific, editor_id);
        assert!(!app.scientific.running);
        assert!(!app.scientific.console_error, "status: {} | console: {}", app.scientific.r_status, app.scientific.console_output);
        assert_eq!(
            egui::TextEdit::load_state(&ctx, editor_id).and_then(|s| s.cursor.char_range()).map(|r| usize::from(r.primary.index)),
            Some(expected_script.chars().count())
        );
        super::run_current_expression(&mut app.scientific, editor_id);
        assert!(!app.scientific.running);
        assert!(!app.scientific.console_error, "status: {} | console: {}", app.scientific.r_status, app.scientific.console_output);
        assert!(!app.scientific.console_output.contains("Error:"));
        assert_eq!(app.scientific.active_script().unwrap().text, expected_script);

        let verify = app.scientific.session.lock().unwrap().execute("stopifnot(first_val == 15, a == 2, b == 3)").unwrap();
        assert!(verify.error.is_none(), "{:?}", verify.error);
    }

    #[test]
    fn go_to_source_persists_across_multiple_expressions_and_scripts_including_eof_newline() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let ctx = egui::Context::default();
        app.scientific.repaint_ctx = Some(ctx.clone());

        let first_id = app.scientific.active_script_id;
        let first_editor_id = egui::Id::unique("origin-script-1");
        app.scientific.editor_widget_ids.insert(first_id, first_editor_id);
        app.scientific.active_script_mut().unwrap().text = "plot_one <- 101\nplot_two <- 202".into();
        navigate_source(&mut app.scientific, first_id, 0);

        super::run_current_expression(&mut app.scientific, first_editor_id);
        wait_until_idle(&mut app);
        super::run_current_expression(&mut app.scientific, first_editor_id);
        wait_until_idle(&mut app);
        assert_eq!(
            app.scientific.active_script().unwrap().text,
            "plot_one <- 101\nplot_two <- 202\n",
            "status: {} | console: {}",
            app.scientific.r_status,
            app.scientific.console_output
        );

        let second_id = app.scientific.new_script();
        let second_editor_id = egui::Id::unique("origin-script-2");
        app.scientific.editor_widget_ids.insert(second_id, second_editor_id);
        app.scientific.active_script_mut().unwrap().text = "# header\nplot_three <- 303".into();
        navigate_source(&mut app.scientific, second_id, 0);
        super::run_current_expression(&mut app.scientific, second_editor_id);
        wait_until_idle(&mut app);

        // Both scripts' origins remain valid even after EOF newline insertion on each script's final expression.
        go_to_plot_source(&mut app.scientific, "plot_one");
        assert_eq!(app.scientific.active_script_id, first_id);
        assert_eq!(app.scientific.active_script().unwrap().pending_cursor_index, Some(0));
        assert_eq!(app.scientific.r_status, "Opened source for plot_one");

        go_to_plot_source(&mut app.scientific, "plot_two");
        assert_eq!(app.scientific.active_script_id, first_id);
        assert_eq!(app.scientific.active_script().unwrap().pending_cursor_index, Some("plot_one <- 101\n".chars().count()));
        assert_eq!(app.scientific.r_status, "Opened source for plot_two");

        go_to_plot_source(&mut app.scientific, "plot_three");
        assert_eq!(app.scientific.active_script_id, second_id);
        assert_eq!(app.scientific.active_script().unwrap().pending_cursor_index, Some("# header\n".chars().count()));
        assert_eq!(app.scientific.r_status, "Opened source for plot_three");
    }

    #[test]
    fn r_indicator_recovers_from_errors_and_preserves_available_state_across_file_actions() {
        let mut workspace = Workspace { availability_checked: true, r_status: "R detected: Rscript 4.5.2".into(), ..Workspace::default() };
        assert_eq!(super::r_indicator(&workspace), super::RIndicator::Checking);

        workspace.r_status = "Opened C:/test/analysis.R".into();
        assert_eq!(super::r_indicator(&workspace), super::RIndicator::Checking);
        workspace.r_status = "Saved C:/test/analysis.R".into();
        assert_eq!(super::r_indicator(&workspace), super::RIndicator::Checking);
        workspace.r_status = "No R expression at or after the cursor".into();
        assert_eq!(super::r_indicator(&workspace), super::RIndicator::Checking);

        workspace.console_continuation = true;
        assert_eq!(super::r_indicator(&workspace), super::RIndicator::Ready);
        workspace.console_continuation = false;

        workspace.session_active = true;
        workspace.console_error = true;
        assert_eq!(super::r_indicator(&workspace), super::RIndicator::Error);

        // Starting or completing a valid command clears the error indicator.
        workspace.console_error = false;
        workspace.running = true;
        assert_eq!(super::r_indicator(&workspace), super::RIndicator::Running);
        workspace.running = false;
        assert_eq!(super::r_indicator(&workspace), super::RIndicator::Ready);
    }

    #[test]
    fn r_queue_executes_three_or_more_expressions_fifo_order_and_advances_caret() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        assert!(app.scientific.session.lock().unwrap().execute("invisible(NULL)").unwrap().error.is_none());
        let script_text = "q1 <- 10\nq2 <- q1 * 2\nq3 <- q2 + 5\n";
        app.scientific.active_script_mut().unwrap().text = script_text.into();
        let ctx = egui::Context::default();
        let editor_id = egui::Id::unique("fifo-queue-test");
        let mut state = egui::text_edit::TextEditState::default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(0))));
        state.store(&ctx, editor_id);
        app.scientific.repaint_ctx = Some(ctx.clone());
        app.scientific.editor_widget_ids.insert(1, editor_id);

        // Press Ctrl+Enter on line 1 (q1 <- 10)
        super::run_current_expression(&mut app.scientific, editor_id);
        assert_eq!(app.scientific.active_script().unwrap().pending_cursor_index, Some("q1 <- 10\n".chars().count()));
        assert!(app.scientific.running);

        // Press Ctrl+Enter on line 2 (q2 <- q1 * 2) while q1 is running
        super::run_current_expression(&mut app.scientific, editor_id);
        assert_eq!(app.scientific.active_script().unwrap().pending_cursor_index, Some("q1 <- 10\nq2 <- q1 * 2\n".chars().count()));
        assert_eq!(app.scientific.execution_queue.len(), 1);
        assert_eq!(app.scientific.execution_queue[0].code, "q2 <- q1 * 2");

        // Press Ctrl+Enter on line 3 (q3 <- q2 + 5) while q1 is running
        super::run_current_expression(&mut app.scientific, editor_id);
        assert_eq!(app.scientific.active_script().unwrap().pending_cursor_index, Some(script_text.chars().count()));
        assert_eq!(app.scientific.execution_queue.len(), 2);
        assert_eq!(app.scientific.execution_queue[1].code, "q3 <- q2 + 5");

        // Drains in FIFO order
        wait_until_idle(&mut app);
        assert!(!app.scientific.running);
        assert!(app.scientific.execution_queue.is_empty());
        let check = app.scientific.session.lock().unwrap().execute("stopifnot(q1 == 10, q2 == 20, q3 == 25)").unwrap();
        assert!(check.error.is_none(), "{:?}", check.error);
    }

    #[test]
    fn r_queue_multiline_expression_and_selected_code() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        assert!(app.scientific.session.lock().unwrap().execute("invisible(NULL)").unwrap().error.is_none());
        let source = "ml_res <- 10 +\n  # comment\n  20\nignore_me <- 0; sel_val <- 77; ignore_too <- 0\n";
        app.scientific.active_script_mut().unwrap().text = source.into();
        let ctx = egui::Context::default();
        let editor_id = egui::Id::unique("multiline-and-selection-queue-test");
        let mut state = egui::text_edit::TextEditState::default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(0))));
        state.store(&ctx, editor_id);
        app.scientific.repaint_ctx = Some(ctx.clone());
        app.scientific.editor_widget_ids.insert(1, editor_id);

        // Run multiline expression at line 0
        super::run_selection_or_current_expression(&mut app.scientific);
        let second_line_offset = "ml_res <- 10 +\n  # comment\n  20\n".chars().count();
        assert_eq!(app.scientific.active_script().unwrap().pending_cursor_index, Some(second_line_offset));

        // Select exact slice "sel_val <- 77" and run
        let start_sel = source.find("sel_val <- 77").unwrap();
        let end_sel = start_sel + "sel_val <- 77".len();
        let mut sel_state = egui::TextEdit::load_state(&ctx, editor_id).unwrap();
        sel_state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(start_sel), egui::text::CCursor::new(end_sel))));
        sel_state.store(&ctx, editor_id);

        super::run_selection_or_current_expression(&mut app.scientific);
        assert_eq!(app.scientific.active_script().unwrap().pending_cursor_index, Some(end_sel));

        wait_until_idle(&mut app);
        let check = app.scientific.session.lock().unwrap().execute("stopifnot(ml_res == 30, sel_val == 77)").unwrap();
        assert!(check.error.is_none(), "{:?}", check.error);
    }

    #[test]
    fn r_queue_immutable_snapshot_ignores_later_text_edits() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        assert!(app.scientific.session.lock().unwrap().execute("invisible(NULL)").unwrap().error.is_none());
        app.scientific.active_script_mut().unwrap().text = "Sys.sleep(0.3)\nfrozen_val <- 100\n".into();
        let ctx = egui::Context::default();
        let editor_id = egui::Id::unique("immutable-snapshot-test");
        let mut state = egui::text_edit::TextEditState::default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(0))));
        state.store(&ctx, editor_id);
        app.scientific.repaint_ctx = Some(ctx.clone());
        app.scientific.editor_widget_ids.insert(1, editor_id);

        // Submit Sys.sleep(0.3)
        super::run_current_expression(&mut app.scientific, editor_id);
        assert!(app.scientific.running);

        // Submit frozen_val <- 100 while sleeping
        super::run_current_expression(&mut app.scientific, editor_id);
        assert_eq!(app.scientific.execution_queue.len(), 1);
        assert_eq!(app.scientific.execution_queue[0].code, "frozen_val <- 100");

        // Immediately edit script text to 999
        app.scientific.active_script_mut().unwrap().text = "Sys.sleep(0.3)\nfrozen_val <- 999\n".into();

        // Queued command should run the captured snapshot (100), not the edited text (999)
        wait_until_idle(&mut app);
        let check = app.scientific.session.lock().unwrap().execute("stopifnot(frozen_val == 100)").unwrap();
        assert!(check.error.is_none(), "{:?}", check.error);
    }

    #[test]
    fn r_queue_script_switching_preserves_queued_execution() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        assert!(app.scientific.session.lock().unwrap().execute("invisible(NULL)").unwrap().error.is_none());
        let s1_id = app.scientific.active_script_id;
        let s1_editor_id = egui::Id::unique("script-1-editor");
        app.scientific.active_script_mut().unwrap().text = "Sys.sleep(0.2)\nfrom_s1 <- 99\n".into();
        let ctx = egui::Context::default();
        let mut state = egui::text_edit::TextEditState::default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(0))));
        state.store(&ctx, s1_editor_id);
        app.scientific.repaint_ctx = Some(ctx.clone());
        app.scientific.editor_widget_ids.insert(s1_id, s1_editor_id);

        super::run_current_expression(&mut app.scientific, s1_editor_id);
        super::run_current_expression(&mut app.scientific, s1_editor_id);
        assert_eq!(app.scientific.execution_queue.len(), 1);

        // Switch to a new second script
        let s2_id = app.scientific.new_script();
        let s2_editor_id = egui::Id::unique("script-2-editor");
        app.scientific.editor_widget_ids.insert(s2_id, s2_editor_id);
        app.scientific.active_script_mut().unwrap().text = "# script 2 text untouched".into();
        let mut s2_state = egui::text_edit::TextEditState::default();
        s2_state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(0))));
        s2_state.store(&ctx, s2_editor_id);

        wait_until_idle(&mut app);
        assert_eq!(app.scientific.active_script_id, s2_id);
        assert_eq!(app.scientific.active_script().unwrap().text, "# script 2 text untouched");
        let check = app.scientific.session.lock().unwrap().execute("stopifnot(from_s1 == 99)").unwrap();
        assert!(check.error.is_none(), "{:?}", check.error);
    }

    #[test]
    fn r_queue_ordinary_error_continues_queue_without_duplicate() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        assert!(app.scientific.session.lock().unwrap().execute("invisible(NULL)").unwrap().error.is_none());
        app.scientific.active_script_mut().unwrap().text = "ok_first <- 1\nstop('deliberate error')\nok_second <- 2\n".into();
        let ctx = egui::Context::default();
        let editor_id = egui::Id::unique("error-queue-test");
        let mut state = egui::text_edit::TextEditState::default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(0))));
        state.store(&ctx, editor_id);
        app.scientific.repaint_ctx = Some(ctx);
        app.scientific.editor_widget_ids.insert(1, editor_id);

        super::run_current_expression(&mut app.scientific, editor_id);
        super::run_current_expression(&mut app.scientific, editor_id);
        super::run_current_expression(&mut app.scientific, editor_id);

        wait_until_idle(&mut app);
        assert!(app.scientific.console_output.contains("deliberate error"));
        let check = app.scientific.session.lock().unwrap().execute("stopifnot(ok_first == 1, ok_second == 2)").unwrap();
        assert!(check.error.is_none(), "{:?}", check.error);
    }

    #[test]
    fn r_queue_stop_and_restart_clears_pending_jobs() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.scientific.running = true;
        app.scientific.execution_queue.push_back(QueuedRCommand {
            id: 1,
            script_id: Some(1),
            code: "x <- 1".into(),
            source_revision: None,
            start_line: None,
            refresh_linked: false,
            is_selection: false,
        });
        app.scientific.execution_queue.push_back(QueuedRCommand {
            id: 2,
            script_id: Some(1),
            code: "y <- 2".into(),
            source_revision: None,
            start_line: None,
            refresh_linked: false,
            is_selection: false,
        });
        assert_eq!(app.scientific.execution_queue.len(), 2);

        // Stop clears queue
        control_r_session(&mut app.scientific, false);
        assert!(app.scientific.execution_queue.is_empty());
        assert!(app.scientific.r_status.contains("2 queued command(s) cancelled"));

        // Restart clears queue
        app.scientific.execution_queue.push_back(QueuedRCommand {
            id: 3,
            script_id: Some(1),
            code: "z <- 3".into(),
            source_revision: None,
            start_line: None,
            refresh_linked: false,
            is_selection: false,
        });
        control_r_session(&mut app.scientific, true);
        assert!(app.scientific.execution_queue.is_empty());
        assert!(app.scientific.r_status.contains("1 queued command(s) cancelled"));
    }

    #[test]
    fn r_queue_crash_discards_jobs_without_replaying() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.scientific.running = true;
        app.scientific.execution_queue.push_back(QueuedRCommand {
            id: 1,
            script_id: Some(1),
            code: "bad <- 1".into(),
            source_revision: None,
            start_line: None,
            refresh_linked: false,
            is_selection: false,
        });
        app.scientific.execution_queue.push_back(QueuedRCommand {
            id: 2,
            script_id: Some(1),
            code: "bad <- 2".into(),
            source_revision: None,
            start_line: None,
            refresh_linked: false,
            is_selection: false,
        });

        // Simulate crash message from worker
        let _ = app.scientific._tx.send(RMessage::Execution(Err("Process terminated unexpectedly".into()), false, None));
        poll_messages(&mut app);
        assert!(!app.scientific.running);
        assert!(app.scientific.execution_queue.is_empty(), "queued jobs must be discarded after crash");
        assert!(app.scientific.r_status.contains("2 queued commands discarded"));
    }

    #[test]
    fn r_queue_capacity_limit_and_key_repeat_suppression() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.scientific.running = true;
        for i in 0..super::MAX_QUEUE_CAPACITY {
            super::enqueue_or_execute(
                &mut app.scientific,
                QueuedRCommand {
                    id: i as u64,
                    script_id: Some(1),
                    code: format!("c_{i} <- {i}"),
                    source_revision: None,
                    start_line: None,
                    refresh_linked: false,
                    is_selection: false,
                },
            );
        }
        assert_eq!(app.scientific.execution_queue.len(), super::MAX_QUEUE_CAPACITY);

        // 101st command exceeds capacity
        super::enqueue_or_execute(
            &mut app.scientific,
            QueuedRCommand {
                id: 999,
                script_id: Some(1),
                code: "overflow <- 1".into(),
                source_revision: None,
                start_line: None,
                refresh_linked: false,
                is_selection: false,
            },
        );
        assert_eq!(app.scientific.execution_queue.len(), super::MAX_QUEUE_CAPACITY);
        assert!(app.scientific.r_status.contains("queue is full"));

        // Verify key repeat suppression
        let mut input = egui::InputState::default();
        input.events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        });
        assert!(consume_key_no_repeat(&mut input, egui::Modifiers::COMMAND, egui::Key::Enter), "initial press must trigger");
        assert!(input.events.is_empty(), "event must be consumed");

        input.events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: true,
            modifiers: egui::Modifiers::COMMAND,
        });
        assert!(!consume_key_no_repeat(&mut input, egui::Modifiers::COMMAND, egui::Key::Enter), "repeat must NOT trigger");
        assert!(input.events.is_empty(), "repeat event must still be consumed");
    }

    #[test]
    fn data_viewer_tab_lifecycle_focus_and_closing_fallback() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.scientific.enabled = true;
        let script_1 = app.scientific.active_script_id;
        assert_eq!(app.scientific.tab_order, vec![super::CentralTabId::Script(script_1)]);
        assert!(app.scientific.active_viewer_tab.is_none());

        // 1. Open first viewer tab `df`
        app.scientific.open_data_viewer("df");
        assert_eq!(app.scientific.active_viewer_tab.as_deref(), Some("df"));
        assert_eq!(app.scientific.viewer_tabs.len(), 1);
        assert_eq!(app.scientific.viewer_tabs[0].name, "df");
        assert!(app.scientific.viewer_tabs[0].loading);
        assert_eq!(app.scientific.tab_order, vec![super::CentralTabId::Script(script_1), super::CentralTabId::DataViewer("df".into())]);

        // 2. Re-opening existing viewer focuses existing tab rather than duplicating
        app.scientific.open_data_viewer("df");
        assert_eq!(app.scientific.viewer_tabs.len(), 1);
        assert_eq!(app.scientific.tab_order.len(), 2);
        assert_eq!(app.scientific.active_viewer_tab.as_deref(), Some("df"));

        // 3. Open second viewer tab `matrix_A`
        app.scientific.open_data_viewer("matrix_A");
        assert_eq!(app.scientific.viewer_tabs.len(), 2);
        assert_eq!(app.scientific.active_viewer_tab.as_deref(), Some("matrix_A"));
        assert_eq!(
            app.scientific.tab_order,
            vec![
                super::CentralTabId::Script(script_1),
                super::CentralTabId::DataViewer("df".into()),
                super::CentralTabId::DataViewer("matrix_A".into()),
            ]
        );

        // 4. Switching freely between scripts and viewer tabs
        app.scientific.activate_script(script_1);
        assert!(app.scientific.active_viewer_tab.is_none());
        assert_eq!(app.scientific.active_script_id, script_1);
        // Viewer tabs stay open and cached
        assert_eq!(app.scientific.viewer_tabs.len(), 2);

        // Switch to `df`
        app.scientific.active_viewer_tab = Some("df".into());
        assert_eq!(app.scientific.active_viewer_tab.as_deref(), Some("df"));

        // 5. Closing active viewer tab `df` falls back to next tab (`matrix_A`)
        app.scientific.close_data_viewer("df");
        assert_eq!(app.scientific.viewer_tabs.len(), 1);
        assert_eq!(app.scientific.active_viewer_tab.as_deref(), Some("matrix_A"));
        assert_eq!(app.scientific.tab_order, vec![super::CentralTabId::Script(script_1), super::CentralTabId::DataViewer("matrix_A".into())]);

        // 6. Closing last viewer tab `matrix_A` falls back to the active script
        app.scientific.close_data_viewer("matrix_A");
        assert_eq!(app.scientific.viewer_tabs.len(), 0);
        assert!(app.scientific.active_viewer_tab.is_none());
        assert_eq!(app.scientific.active_script_id, script_1);
        assert_eq!(app.scientific.scripts.len(), 1);
    }

    #[test]
    fn data_viewer_tabs_do_not_participate_in_script_save_or_execution() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.scientific.enabled = true;
        let script_id = app.scientific.active_script_id;

        // With script active: save, run, source enabled
        assert!(super::command_enabled(&app, "r.script.save").unwrap_or(false));
        assert!(super::command_enabled(&app, "r.run").unwrap_or(false));
        assert!(super::command_enabled(&app, "r.source").unwrap_or(false));

        // Open viewer tab
        app.scientific.open_data_viewer("airway_counts");
        assert_eq!(app.scientific.active_viewer_tab.as_deref(), Some("airway_counts"));

        // When viewer tab is active: save, run, source must be disabled
        assert!(!super::command_enabled(&app, "r.script.save").unwrap_or(true));
        assert!(!super::command_enabled(&app, "r.script.saveAs").unwrap_or(true));
        assert!(!super::command_enabled(&app, "r.run").unwrap_or(true));
        assert!(!super::command_enabled(&app, "r.source").unwrap_or(true));

        // Close is enabled and closes the viewer tab
        assert!(super::command_enabled(&app, "r.script.close").unwrap_or(false));
        let _ = super::run_command(&mut app, "r.script.close", &serde_json::Value::Null);
        assert!(app.scientific.active_viewer_tab.is_none());
        assert_eq!(app.scientific.active_script_id, script_id);
    }

    #[test]
    fn data_viewer_response_caching_and_error_handling() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.scientific.open_data_viewer("test_df");

        let sample_page = crate::r_session::DataPage {
            total_rows: 100,
            columns: vec![("gene".into(), "character".into()), ("count".into(), "integer".into())],
            rows: vec![vec!["BRCA1".into(), "42".into()], vec!["TP53".into(), "NA".into()]],
        };

        // Deliver DataPage response
        let (tx, rx) = std::sync::mpsc::channel();
        app.scientific._tx = tx.clone();
        app.scientific._rx = rx;
        tx.send(super::RMessage::DataPage("test_df".into(), Ok(sample_page.clone()))).unwrap();
        super::poll_messages(&mut app);

        let tab = app.scientific.viewer_tabs.iter().find(|t| t.name == "test_df").unwrap();
        assert!(!tab.loading);
        assert!(tab.error.is_none());
        assert_eq!(tab.page.as_ref().unwrap().total_rows, 100);
        assert_eq!(tab.page.as_ref().unwrap().rows.len(), 2);

        // Test error delivery (e.g. object not found or unsupported class)
        tx.send(super::RMessage::DataPage("test_df".into(), Err("Object 'test_df' of class 'function' cannot be viewed as a table".into()))).unwrap();
        super::poll_messages(&mut app);

        let tab = app.scientific.viewer_tabs.iter().find(|t| t.name == "test_df").unwrap();
        assert!(!tab.loading);
        assert!(tab.error.as_deref().unwrap().contains("cannot be viewed as a table"));
    }

    #[test]
    fn rdata_import_messages_and_plot_registry_discovery() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let (tx, rx) = std::sync::mpsc::channel();
        app.scientific._tx = tx.clone();
        app.scientific._rx = rx;

        let inspection = crate::r_session::RDataInspection {
            file_path: "experiment.RData".into(),
            total_objects: 3,
            object_names: vec!["p1".into(), "plot_data".into(), "counts".into()],
            collisions: vec!["counts".into()],
        };
        tx.send(super::RMessage::RDataInspected { path: "experiment.RData".into(), policy_override: None, inspection: Ok(inspection) }).unwrap();
        super::poll_messages(&mut app);

        assert!(app.scientific.pending_rdata_import.is_some());
        let pending = app.scientific.pending_rdata_import.as_ref().unwrap();
        assert_eq!(pending.total_objects, 3);
        assert_eq!(pending.collisions, vec!["counts"]);
        assert!(app.scientific.r_status.contains("1 collision(s)"));

        super::cancel_rdata_import(&mut app.scientific);
        assert!(app.scientific.pending_rdata_import.is_none());
        assert_eq!(app.scientific.r_status, "RData import cancelled");

        let imported_objects = vec![
            crate::r_session::RObject {
                variable: "p1".into(),
                classes: vec!["gg".into(), "ggplot".into()],
                data_type: "list".into(),
                dimensions: "".into(),
                length: 9,
                preview: "".into(),
                category: "Values".into(),
                description: "ggplot2::ggplot".into(),
                plot_kind: "ggplot".into(),
            },
            crate::r_session::RObject {
                variable: "p_heat".into(),
                classes: vec!["pheatmap".into()],
                data_type: "list".into(),
                dimensions: "".into(),
                length: 5,
                preview: "".into(),
                category: "Values".into(),
                description: "pheatmap::pheatmap".into(),
                plot_kind: "pheatmap".into(),
            },
            crate::r_session::RObject {
                variable: "plot_summary".into(),
                classes: vec!["data.frame".into()],
                data_type: "list".into(),
                dimensions: "10x4".into(),
                length: 4,
                preview: "".into(),
                category: "Data".into(),
                description: "10 obs. of 4 variables".into(),
                plot_kind: "".into(),
            },
            crate::r_session::RObject {
                variable: "counts".into(),
                classes: vec!["data.frame".into()],
                data_type: "list".into(),
                dimensions: "50x2".into(),
                length: 2,
                preview: "".into(),
                category: "Data".into(),
                description: "50 obs. of 2 variables".into(),
                plot_kind: "".into(),
            },
        ];

        let exec = crate::r_session::Execution { output: "".into(), objects: imported_objects, error: None, working_directory: None };

        tx.send(super::RMessage::RDataImported { path: "C:/data/experiment.RData".into(), result: Ok((exec, 4, 0)) }).unwrap();
        super::poll_messages(&mut app);

        assert_eq!(app.scientific.objects.len(), 4);
        assert_eq!(app.scientific.plots.len(), 2);
        let plot_vars: Vec<_> = app.scientific.plots.iter().map(|p| &p.variable).collect();
        assert!(plot_vars.contains(&&"p1".to_string()));
        assert!(plot_vars.contains(&&"p_heat".to_string()));
        assert!(!plot_vars.contains(&&"plot_summary".to_string()), "Variables named 'plot' that are not real plots must NOT be in plot registry");
        assert!(app.scientific.linked_panels.is_empty(), "Importing plots must not automatically insert them into artboard");
        assert!(app.scientific.r_status.contains("Imported 4 object(s) from experiment.RData"));
        assert!(app.scientific.console_output.contains("> load(\"experiment.RData\") # Imported 4 object(s) from experiment.RData"));
    }

    #[test]
    fn rdata_import_error_handling() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        let (tx, rx) = std::sync::mpsc::channel();
        app.scientific._tx = tx.clone();
        app.scientific._rx = rx;

        tx.send(super::RMessage::RDataInspected {
            path: "bad.RData".into(),
            policy_override: None,
            inspection: Err("Failed to load RData file: bad magic number".into()),
        })
        .unwrap();
        super::poll_messages(&mut app);

        assert!(!app.scientific.running);
        assert!(app.scientific.console_error);
        assert!(app.scientific.r_status.contains("RData inspection failed"));
        assert!(app.scientific.console_output.contains("Error: Failed to load RData file: bad magic number"));

        tx.send(super::RMessage::RDataImported { path: "bad.RData".into(), result: Err("Failed to read objects from RData".into()) }).unwrap();
        super::poll_messages(&mut app);

        assert!(!app.scientific.running);
        assert!(app.scientific.console_error);
        assert!(app.scientific.r_status.contains("RData import failed"));
    }

    #[test]
    fn rdata_commands_enabled_and_execution() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());

        assert!(super::command_enabled(&app, "r.environment.importRData").unwrap_or(false));
        assert!(!super::command_enabled(&app, "r.environment.resolveImport").unwrap_or(true));

        app.scientific.pending_rdata_import = Some(crate::r_session::RDataInspection {
            file_path: "foo.RData".into(),
            total_objects: 1,
            object_names: vec!["x".into()],
            collisions: vec!["x".into()],
        });
        assert!(super::command_enabled(&app, "r.environment.resolveImport").unwrap_or(false));

        let cancel_res = super::run_command(&mut app, "r.environment.resolveImport", &serde_json::json!({ "policy": "cancel" }));
        assert!(cancel_res.is_some_and(|r| r.is_ok()));
        assert!(app.scientific.pending_rdata_import.is_none());
        assert_eq!(app.scientific.r_status, "RData import cancelled");
    }

    fn assert_rect_close(actual: vectorcraft_geom::Rect, expected: vectorcraft_geom::Rect) {
        for (actual, expected) in [(actual.x0, expected.x0), (actual.y0, expected.y0), (actual.x1, expected.x1), (actual.y1, expected.y1)] {
            assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
        }
    }
}
