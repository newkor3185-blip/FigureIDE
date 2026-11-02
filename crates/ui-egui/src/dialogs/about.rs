//! Help → About FigureIDE.

use crate::VectorcraftApp;
use crate::theme::{self, Tokens};

/// The About window, while `UiState::about` is set.
pub(super) fn show(app: &mut VectorcraftApp, ctx: &egui::Context) {
    if !app.ui.about {
        return;
    }
    let mut open = true;
    egui::Window::new(tl!("About FigureIDE"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(18)))
        .show(ctx, |ui| {
            // Tabs: About · Credits · Third-Party Licenses
            let tab_id = egui::Id::unique("about_tab");
            let mut tab = ui.data_mut(|d| d.get_temp::<u8>(tab_id)).unwrap_or(0);
            ui.horizontal(|ui| {
                for (i, label) in [tl!("About"), tl!("Credits"), tl!("Third-Party Licenses")].into_iter().enumerate() {
                    let i = i as u8;
                    if ui.selectable_label(tab == i, label).clicked() {
                        tab = i;
                    }
                }
            });
            ui.data_mut(|d| d.insert_temp(tab_id, tab));
            ui.separator();
            match tab {
                1 => credits_tab(app, ui),
                2 => licenses_tab(app, ui),
                _ => about_tab(app, ui),
            }
        });
    app.ui.about = open;
}

/// The About tab: mark, version, product description, project links, licences and graphics adapter.
fn about_tab(app: &mut VectorcraftApp, ui: &mut egui::Ui) {
    ui.set_width(420.0);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
        crate::brand::paint_mark(ui, r);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("FigureIDE").font(theme::semibold(22.0)));
            ui.label(crate::i18n::fmt(tl!("Version {version} — R and SVG scientific graphics editor."), &[("version", env!("CARGO_PKG_VERSION"))]));
        });
    });
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tl!("A scientific R analysis and publication figure editing environment.")).size(12.5));
    ui.add_space(12.0);
    crate::community::links(app, ui);
    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(tl!(
            "Based on VectorCraft. MIT OR Apache-2.0. Fonts: Source Sans 3, Inter, JetBrains Mono (OFL). Icons: Lucide (ISC) and FigureIDE."
        ))
        .size(11.0),
    );
    // The GPU the window renders with, for bug reports (Preferences › Performance picks it).
    if let Some(adapter) = &app.graphics_adapter {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(crate::i18n::fmt(tl!("Graphics: {adapter}"), &[("adapter", adapter.as_str())])).size(11.0));
    }
}

/// The Credits tab: FigureIDE maintainers and upstream VectorCraft attribution.
fn credits_tab(app: &mut VectorcraftApp, ui: &mut egui::Ui) {
    ui.set_width(480.0);
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        ui.label(egui::RichText::new(tl!("FigureIDE Project")).font(theme::semibold(15.0)).color(t.text));
        ui.add_space(4.0);
        ui.label(egui::RichText::new(tl!("FigureIDE is maintained by Hanguk Ryu (@newkor3185-blip).")).size(12.5));
        ui.label(
            egui::RichText::new(tl!(
                "Developed with AI pair programming assistance from Google DeepMind Antigravity and OpenAI Codex."
            ))
            .size(12.5),
        );
        ui.add_space(14.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label(egui::RichText::new(tl!("Based on VectorCraft")).font(theme::semibold(15.0)).color(t.text));
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(tl!(
                "FigureIDE is derived from VectorCraft, an open-source clean-room vector illustration application created by the ArtCraft team and community contributors."
            ))
            .size(12.5),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tl!("Upstream repository:")).size(12.5));
            if ui.link("https://github.com/storytold/vectorcraft").clicked() {
                app.open_url("https://github.com/storytold/vectorcraft");
            }
        });
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(tl!(
                "We gratefully acknowledge the architectural foundation and contributions of the original VectorCraft project authors and contributors."
            ))
            .size(12.0)
            .weak(),
        );
    });
}

/// The Third-Party Licenses tab: license notices and component attribution.
fn licenses_tab(_app: &mut VectorcraftApp, ui: &mut egui::Ui) {
    ui.set_width(480.0);
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        ui.label(egui::RichText::new(tl!("FigureIDE & VectorCraft")).font(theme::semibold(14.0)).color(t.text));
        ui.label(
            egui::RichText::new(
                "Dual-licensed under either of:\n• Apache License, Version 2.0 (LICENSE-APACHE)\n• MIT License (LICENSE-MIT)\nat your option.",
            )
            .size(12.0),
        );
        ui.add_space(10.0);
        ui.label(egui::RichText::new(tl!("Typography & Fonts")).font(theme::semibold(14.0)).color(t.text));
        ui.label(
            egui::RichText::new(
                "• Source Sans 3 (SIL Open Font License 1.1)\n• Inter (SIL Open Font License 1.1)\n• JetBrains Mono (SIL Open Font License 1.1)",
            )
            .size(12.0),
        );
        ui.add_space(10.0);
        ui.label(egui::RichText::new(tl!("Icons & Assets")).font(theme::semibold(14.0)).color(t.text));
        ui.label(egui::RichText::new("• Lucide Icons (ISC License)\n• FigureIDE application iconography").size(12.0));
        ui.add_space(10.0);
        ui.label(egui::RichText::new(tl!("Third-Party Dependencies")).font(theme::semibold(14.0)).color(t.text));
        ui.label(
            egui::RichText::new(
                "Built with Rust and open-source libraries including egui, wgpu, image, resvg, and others under permissive open-source licenses.",
            )
            .size(12.0),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_engine::Session;

    #[test]
    fn about_dialog_renders_when_opened() {
        let mut app = VectorcraftApp::new(Session::new(), Default::default());
        app.ui.about = true;
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(&mut app, ui.ctx());
        });
        output.textures_delta.clear();
        assert!(app.ui.about);
    }
}
