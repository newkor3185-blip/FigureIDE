use super::Workspace;

pub(super) fn show_plot_history(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    let workspace: &mut Workspace = &mut app.scientific;
    egui_extras::install_image_loaders(ui.ctx());
    ui.horizontal(|ui| {
        let previous = ui.add_enabled(workspace.selected_plot_history > 0, egui::Button::new("‹ Previous"));
        if previous.clicked() {
            workspace.selected_plot_history = workspace.selected_plot_history.saturating_sub(1);
        }
        let next = ui.add_enabled(workspace.selected_plot_history + 1 < workspace.plot_history.len(), egui::Button::new("Next ›"));
        if next.clicked() {
            workspace.selected_plot_history = (workspace.selected_plot_history + 1).min(workspace.plot_history.len().saturating_sub(1));
        }
        ui.label(if workspace.plot_history.is_empty() {
            "No plots".into()
        } else {
            format!("{} / {}", workspace.selected_plot_history + 1, workspace.plot_history.len())
        });
        if ui.button("Clear").clicked() {
            workspace.plot_history.clear();
            workspace.selected_plot_history = 0;
        }
    });
    ui.separator();
    let Some(svg) = workspace.plot_history.get(workspace.selected_plot_history) else {
        ui.centered_and_justified(|ui| ui.label("Run plot() or ggplot code in the Console or Source editor to see it here."));
        return;
    };
    let size = ui.available_size().max(egui::vec2(80.0, 80.0));
    let mut content_hash = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(svg, &mut content_hash);
    let uri = format!("bytes://figureide-r-plot/{}.svg", std::hash::Hasher::finish(&content_hash));
    ui.add(egui::Image::from_bytes(uri, svg.as_bytes().to_vec()).fit_to_exact_size(size).maintain_aspect_ratio(true));
}
