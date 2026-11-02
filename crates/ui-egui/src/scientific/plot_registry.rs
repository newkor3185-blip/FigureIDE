use super::{
    DEFAULT_PLOT_HEIGHT_MM, DEFAULT_PLOT_WIDTH_MM, PanelAction, compact_panel_style, object_metadata_tooltip, panel_action, panel_caption,
    panel_entry, panel_label, panel_name, render_plot,
};

pub(super) fn show_plot_registry(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    ui.scope(|ui| {
        compact_panel_style(ui);
        show_plot_registry_entries(app, ui);
    });
}

fn show_plot_registry_entries(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    let plots = app.scientific.plots.clone();
    if plots.is_empty() {
        panel_caption(ui, "No supported plot objects. Run R code to populate the registry.");
        return;
    }
    let document_uid = app.session.active().map(|doc| doc.uid).unwrap_or_default();
    for plot in plots {
        let instances =
            app.scientific.linked_panels.iter().filter(|panel| panel.document_uid == document_uid && panel.r_variable == plot.variable).count();
        let selected = app.scientific.selected_plot.as_deref() == Some(&plot.variable);
        panel_entry(ui, selected, |ui| {
            if panel_name(ui, &plot.variable, selected).clicked() {
                app.scientific.selected_plot = Some(plot.variable.clone());
            }
            panel_caption(ui, format!("{} | {instances} linked", plot.description))
                .on_hover_text(object_metadata_tooltip(&plot.classes, &plot.data_type));
            ui.horizontal_wrapped(|ui| {
                if panel_action(ui, "Select", true, false).clicked() {
                    app.scientific.selected_plot = Some(plot.variable.clone());
                }
                if panel_action(ui, "Add to Artboard", !app.scientific.running, false)
                    .on_hover_text("Create a separate R-linked instance. Repeating this action adds another instance.")
                    .clicked()
                {
                    let label = panel_label(app.scientific.linked_panels.iter().filter(|panel| panel.document_uid == document_uid).count());
                    render_plot(
                        &mut app.scientific,
                        document_uid,
                        plot.variable,
                        PanelAction::Create { label, width_mm: DEFAULT_PLOT_WIDTH_MM, height_mm: DEFAULT_PLOT_HEIGHT_MM },
                    );
                }
            });
        });
    }
}
