use super::Workspace;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RIndicator {
    Ready,
    Running,
    Checking,
    Error,
    Disconnected,
}

impl RIndicator {
    pub(super) fn color(self) -> egui::Color32 {
        match self {
            Self::Ready => egui::Color32::from_rgb(49, 166, 105),
            Self::Running => egui::Color32::from_rgb(220, 73, 73),
            Self::Checking => egui::Color32::from_rgb(209, 155, 59),
            Self::Error => egui::Color32::from_rgb(173, 54, 111),
            Self::Disconnected => egui::Color32::from_rgb(130, 136, 148),
        }
    }
}

pub(super) fn r_indicator(workspace: &Workspace) -> RIndicator {
    if workspace.running {
        RIndicator::Running
    } else if workspace.checking_r {
        RIndicator::Checking
    } else if workspace.console_error {
        RIndicator::Error
    } else if workspace.session_active || workspace.console_continuation {
        RIndicator::Ready
    } else if workspace.r_status.starts_with("R detected:")
        || (workspace.availability_checked
            && (workspace.r_status.starts_with("Opened ")
                || workspace.r_status.starts_with("Already open: ")
                || workspace.r_status.starts_with("Saved ")
                || workspace.r_status == "R input cancelled"
                || workspace.r_status == "No R expression at or after the cursor"))
    {
        RIndicator::Checking
    } else {
        RIndicator::Disconnected
    }
}

pub(super) fn workspace_status(workspace: &Workspace, _ui: &egui::Ui) -> (&'static str, egui::Color32) {
    let indicator = r_indicator(workspace);
    let label = match indicator {
        RIndicator::Running => "R: Running",
        RIndicator::Checking if workspace.checking_r => "R: Checking",
        RIndicator::Checking => "R: Available",
        RIndicator::Error => "R: Error",
        RIndicator::Ready => "R: Ready",
        RIndicator::Disconnected => "R: Disconnected",
    };
    (label, indicator.color())
}

pub(super) fn show_status_indicator(app: &crate::VectorcraftApp, ui: &mut egui::Ui) -> egui::Response {
    let (status, color) = workspace_status(&app.scientific, ui);
    ui.label(egui::RichText::new(format!("● {status}")).color(color)).on_hover_text(&app.scientific.r_status)
}
