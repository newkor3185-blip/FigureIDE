//! Real panel layouts, accessibility rectangles, input and optional offscreen PNG captures.
use super::*;
use egui::{Rect, Shape, accesskit::Role, vec2};
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use serde_json::{Value, json};

const HEIGHT: f32 = 1000.0;
const WIDTHS: [f32; 4] = [320.0, 420.0, 560.0, 800.0];

fn fixture(count: usize) -> crate::VectorcraftApp {
    let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
    app.scientific.enabled = true;
    app.scientific.availability_checked = true;
    app.scientific.r_status = "Ready (layout fixture)".into();
    app.run("file.new", json!({"width": 800, "height": 600})).unwrap();
    app.scientific.objects = (0..count)
        .map(|index| {
            let (variable, classes, data_type, dimensions, preview) = match index {
                0 => ("df".into(), vec!["data.frame".into()], "list", "4x5", "4 rows, 5 columns"),
                1 => ("한글_변수".into(), vec!["character".into()], "character", "", "한글 값 / UTF-8 / café / 日本語"),
                2 => ("plot_A".into(), vec!["ggplot2::ggplot".into(), "ggplot".into(), "S7_object".into(), "gg".into()], "object", "", ""),
                3 => (
                    "한글_plot_B_with_a_long_variable_name_that_must_remain_readable".into(),
                    vec![
                        "ggplot2::ggplot_with_an_intentionally_long_class_name_for_layout_tests".into(),
                        "ggplot".into(),
                        "an_extraordinarily_long_class_name_that_should_not_displace_the_action_buttons".into(),
                        "S7_object".into(),
                    ],
                    "object",
                    "",
                    "",
                ),
                _ => (format!("numeric_{index:02}"), vec!["numeric".into()], "double", "", "1 2 3 4 5"),
            };
            crate::r_session::RObject {
                variable,
                classes,
                data_type: data_type.into(),
                dimensions: dimensions.into(),
                length: 5,
                preview: preview.into(),
                category: if index == 0 { "Data" } else { "Values" }.into(),
                description: if index == 0 { "4 obs. of 5 variables" } else { preview }.into(),
                plot_kind: if index == 2 || index == 3 { "ggplot" } else { "" }.into(),
            }
        })
        .collect();
    app.scientific.plots = app.scientific.objects.iter().filter(|object| !object.plot_kind.is_empty()).cloned().collect();
    for (index, plot) in app.scientific.plots.clone().into_iter().enumerate() {
        let shape = app.run("shape.rectangle", json!({"x": 20 + index * 100, "y": 30, "width": 80, "height": 60})).unwrap()["id"].as_u64().unwrap();
        let generated = app.run("object.group", json!({"ids": [shape]})).unwrap()["id"].as_u64().unwrap();
        let panel_id = app.run("object.group", json!({"ids": [generated]})).unwrap()["id"].as_u64().unwrap();
        let panel = RLinkedPanel {
            panel_id,
            generated_id: generated,
            label: panel_label(index),
            r_variable: plot.variable,
            width_mm: 100.0,
            height_mm: 80.0,
            resize_mode: ResizeMode::Reflow,
            graph_style: Default::default(),
            small_figure_typography: false,
            document_uid: app.session.active().unwrap().uid,
        };
        app.run("object.setName", json!({"id": panel_id, "name": persistent_panel_name(&panel, 100.0, 80.0, ResizeMode::Reflow)})).unwrap();
        app.scientific.linked_panels.push(panel);
    }
    app.run("select.none", json!({})).unwrap();
    app.scientific.console_output = "> mean(c(10, 20, 30))\n[1] 20\n> cat('한글 출력\\n')\n한글 출력\n".into();
    app.scientific.console_history = vec!["mean(c(10, 20, 30))".into()];
    app
}

fn harness(tab: BottomTab, width: f32, scale: f32, count: usize, tabs: bool) -> Harness<'static, crate::VectorcraftApp> {
    let mut app = fixture(count);
    app.ui.r_studio_tab = tab.key().into();
    let builder = Harness::builder()
        .with_size(vec2(width, HEIGHT))
        .with_pixels_per_point(scale)
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_context_setup(|ctx| {
            egui_extras::install_image_loaders(ctx);
            crate::theme::install_fonts(ctx);
            crate::theme::apply(ctx, crate::theme::Brightness::MediumDark);
        });
    let mut harness = builder.build_ui_state(
        move |ui, app: &mut crate::VectorcraftApp| {
            ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);
            if tabs {
                show_bottom_tabs(app, ui);
            } else if tab == BottomTab::Console {
                show_console(app, ui);
            } else {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match tab {
                    BottomTab::Environment => show_environment(app, ui),
                    BottomTab::PlotRegistry => show_plot_registry(app, ui),
                    BottomTab::LinkedPanels => show_linked_panels(app, ui),
                    _ => show_console(app, ui),
                });
            }
            crate::scientific::linked_plot_protection::show(app, ui.ctx());
            // Use the production font fallback, and finish its background lookup deterministically.
            app.ui_fonts.frame(ui.ctx());
            app.ui_fonts.finish(ui.ctx());
        },
        app,
    );
    harness.run_steps(5);
    harness
}

fn geometry(harness: &Harness<'_, crate::VectorcraftApp>, width: f32) -> Value {
    measured_geometry(harness, width, false)
}

fn measured_geometry(harness: &Harness<'_, crate::VectorcraftApp>, width: f32, source: bool) -> Value {
    let mut controls = vec![];
    let mut rectangles = vec![];
    for node in harness.root().children_recursive() {
        let data = node.accesskit_node();
        if data.is_hidden()
            || data.bounding_box().is_none()
            || !matches!(data.role(), Role::Button | Role::TextInput | Role::MultilineTextInput | Role::ComboBox | Role::CheckBox)
        {
            continue;
        }
        let mut rect = node.rect();
        // Source tabs intentionally extend along a horizontal scroll strip.
        // Measure the visible portion; activation/reachability is tested separately.
        if source && data.role() == Role::Button && rect.top() < 44.0 {
            rect = rect.intersect(Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(width, 44.0)));
            if !rect.is_positive() {
                continue;
            }
        }
        if source && matches!(data.role(), Role::TextInput | Role::MultilineTextInput) && rect.top() < 384.0 {
            rect = rect.intersect(Rect::from_min_max(egui::pos2(0.0, 44.0), egui::pos2(width, 384.0)));
        }
        let label = data.label().unwrap_or_default();
        assert!(rect.left() >= -1.0 && rect.right() <= width + 1.0, "horizontal overflow: {label:?} {rect:?} at {width}");
        for (other_label, other_rect) in &rectangles {
            let overlap: Rect = rect.intersect(*other_rect);
            assert!(
                overlap.width() <= 1.0 || overlap.height() <= 1.0,
                "overlapping controls {label:?} and {other_label:?}: {rect:?} / {other_rect:?}"
            );
        }
        rectangles.push((label.clone(), rect));
        controls.push(json!({"label": label, "role": format!("{:?}", data.role()), "rect": [rect.left(), rect.top(), rect.width(), rect.height()], "enabled": !data.is_disabled()}));
    }
    fn check_text(shape: &Shape, clip: Rect, controls: &[(String, Rect)]) {
        match shape {
            Shape::Text(text) => {
                if text.galley.text().is_empty() {
                    return;
                }
                let rect = text.visual_bounding_rect().intersect(clip);
                if !rect.is_positive() {
                    return;
                }
                for (label, button) in controls {
                    if button.expand(1.0).contains(rect.center()) {
                        continue;
                    } // This widget's own text.
                    let overlap = button.intersect(rect);
                    assert!(overlap.width() <= 2.0 || overlap.height() <= 2.0, "text {:?} overlaps control {label:?}", text.galley.text());
                }
            }
            Shape::Vec(shapes) => {
                for shape in shapes {
                    check_text(shape, clip, controls);
                }
            }
            _ => {}
        }
    }
    for shape in &harness.output().shapes {
        check_text(&shape.shape, shape.clip_rect, &rectangles);
    }
    let korean = harness.ctx.fonts_mut(|fonts| fonts.has_glyph(&egui::FontId::proportional(13.0), '한'));
    json!({"mode": "headless-egui", "width": width, "height": HEIGHT, "scale": harness.ctx.pixels_per_point(), "controls": controls,
        "clipRect": [0.0, 0.0, width, HEIGHT], "koreanGlyphAvailable": korean, "horizontalOverflow": false, "controlOverlap": false})
}

fn capture(harness: &mut Harness<'_, crate::VectorcraftApp>, tab: BottomTab, width: f32, scale: f32, scenario: &str, geometry: &Value) {
    let Some(root) = std::env::var_os("R_WORKSPACE_UI_ARTIFACTS").map(std::path::PathBuf::from) else { return };
    std::fs::create_dir_all(&root).unwrap();
    let stem = format!("{}-{width:.0}px-scale{:.0}-{scenario}", tab.key(), scale * 100.0);
    let mut measurements = geometry.clone();
    measurements["imageGenerated"] = json!(false);
    let image_path = root.join(format!("{stem}.png"));
    if image_path.is_file() {
        std::fs::remove_file(&image_path).unwrap();
    }
    // Measurements survive even if the optional renderer or native GPU driver fails.
    std::fs::write(root.join(format!("{stem}.json")), serde_json::to_vec_pretty(&measurements).unwrap()).unwrap();
    if std::env::var_os("R_WORKSPACE_UI_CAPTURE").is_some() {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| harness.render())) {
            Ok(Ok(image)) => {
                image.save(&image_path).unwrap();
                measurements["imageGenerated"] = json!(true);
            }
            result => {
                let error = match result {
                    Ok(Err(error)) => error,
                    _ => "renderer initialization panicked".into(),
                };
                measurements["captureError"] = json!(error);
                eprintln!("{stem}: offscreen rendering unavailable: {error}");
            }
        }
    }
    std::fs::write(root.join(format!("{stem}.json")), serde_json::to_vec_pretty(&measurements).unwrap()).unwrap();
}

#[test]
fn r_workspace_layout_matrix() {
    for width in WIDTHS {
        for scale in [1.0, 1.5] {
            for tab in [BottomTab::Environment, BottomTab::PlotRegistry, BottomTab::LinkedPanels, BottomTab::Console] {
                let mut harness = harness(tab, width, scale, 5, false);
                let measurements = geometry(&harness, width);
                capture(&mut harness, tab, width, scale, "normal-unicode-long-metadata", &measurements);
            }
        }
    }
}

#[test]
fn r_workspace_empty_many_objects_and_filter() {
    for width in WIDTHS {
        for count in [0, 25] {
            let mut harness = harness(BottomTab::Environment, width, 1.0, count, false);
            let measurements = geometry(&harness, width);
            capture(&mut harness, BottomTab::Environment, width, 1.0, if count == 0 { "empty" } else { "25-objects" }, &measurements);
            assert_eq!(harness.state().scientific.objects.len(), count);
            if count > 0 {
                harness.hover_at(egui::pos2(width / 2.0, HEIGHT - 50.0));
                harness.event(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0.0, -20_000.0),
                    modifiers: egui::Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                });
                harness.run_steps(4);
                let last = harness.get_by_label("numeric_24").rect();
                assert!(last.top() >= 0.0 && last.bottom() <= HEIGHT, "last object must be scroll-reachable: {last:?}");
            }
            let input = harness.get_by_role(Role::TextInput);
            input.focus();
            input.type_text("한글");
            harness.run_steps(3);
            assert_eq!(harness.state().scientific.environment_filter, "한글");
            if count > 0 {
                assert_eq!(harness.query_all_by_label("Remove").count(), 2);
                harness.query_all_by_label("Remove").next().unwrap().click();
                harness.run_steps(2);
                assert_eq!(harness.state().scientific.confirm_object_remove.as_deref(), Some("한글_변수"));
            }
        }
    }
}

#[test]
fn r_workspace_tabs_console_focus_and_canvas_commands() {
    let mut harness = harness(BottomTab::Console, 320.0, 1.0, 5, true);
    harness.get_by_label("Environment").click_accesskit();
    harness.run_steps(3);
    assert_eq!(harness.state().ui.r_studio_tab, "environment");
    let active_tab = harness.get_by_label("Environment").rect();
    assert!(active_tab.left() >= 0.0 && active_tab.right() <= 320.0, "selected tab is reachable at minimum width: {active_tab:?}");
    assert!(harness.query_by_label("Global Environment").is_some());
    assert!(harness.query_by_label("Data (1)").is_some());
    assert!(harness.query_by_label("Values (4)").is_some());
    assert!(harness.query_by_label("Functions (0)").is_some());
    harness.get_by_label("Console").click_accesskit();
    harness.run_steps(3);
    let id = harness.state().scientific.console_input_id.unwrap();
    harness.ctx.memory_mut(|memory| memory.request_focus(id));
    harness.event(egui::Event::Text("draft".into()));
    harness.run_steps(2);
    harness.key_press(egui::Key::ArrowUp);
    harness.run_steps(2);
    assert_eq!(harness.state().scientific.console_input, "mean(c(10, 20, 30))");
    harness.key_press(egui::Key::ArrowDown);
    harness.run_steps(2);
    assert_eq!(harness.state().scientific.console_input, "draft");
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::L);
    harness.run_steps(2);
    assert!(harness.state().scientific.console_output.is_empty());
    assert_eq!(harness.state().scientific.objects.len(), 5);
    let shape = harness.state_mut().run("shape.rectangle", json!({"x": 10, "y": 10, "width": 30, "height": 20})).unwrap()["id"].as_u64().unwrap();
    harness.state_mut().run("object.move", json!({"ids": [shape], "dx": 15, "dy": 10})).unwrap();
    assert!(harness.state().session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(shape)).is_some());
}

#[test]
fn r_workspace_linked_panel_actions_keep_drafts_and_artwork() {
    let mut harness = harness(BottomTab::LinkedPanels, 320.0, 1.0, 5, false);
    harness.state_mut().scientific.linked_panels.truncate(1);
    harness.run_steps(2);
    let panel_id = harness.state().scientific.linked_panels[0].panel_id;
    let original_variable = harness.state().scientific.linked_panels[0].r_variable.clone();
    assert!(harness.get_by_label("Update").accesskit_node().is_disabled());
    assert!(harness.query_by_role(Role::ComboBox).is_none(), "relink details stay compact until requested");
    harness.get_by_label("Relink").click();
    harness.run_steps(2);
    let measurements = geometry(&harness, 320.0);
    capture(&mut harness, BottomTab::LinkedPanels, 320.0, 1.0, "relink-expanded", &measurements);
    harness.get_by_role(Role::ComboBox).click();
    harness.run_steps(2);
    let other = harness.state().scientific.plots[1].variable.clone();
    harness.get_by_label(&other).click();
    harness.run_steps(4);
    assert_eq!(harness.state().scientific.panel_relink_open, Some(panel_id), "relink form stays open after selecting another variable");
    assert_eq!(harness.state().scientific.panel_relink_drafts.get(&panel_id), Some(&other), "selection survives later frames");
    harness.get_by_label("Confirm Relink").click();
    harness.run_steps(3);
    assert_eq!(harness.state().scientific.linked_panels[0].r_variable, original_variable, "the link changes after the asynchronous R render returns");
    assert_eq!(harness.state().scientific.panel_states.get(&panel_id), Some(&super::LinkedPanelState::Updating));
    assert!(harness.state().scientific.panel_relink_open.is_none());
    assert!(harness.state().session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(panel_id)).is_some());

    let mut delete = self::harness(BottomTab::LinkedPanels, 320.0, 1.0, 5, false);
    delete.state_mut().scientific.linked_panels.truncate(1);
    delete.run_steps(2);
    let panel_id = delete.state().scientific.linked_panels[0].panel_id;
    delete.get_by_label("Delete Artwork").click();
    delete.run_steps(2);
    assert!(delete.state().session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(panel_id)).is_some());
    delete.get_by_label("Confirm Delete").click();
    delete.run_steps(2);
    assert!(delete.state().scientific.linked_panels.is_empty());
    assert!(delete.state().session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(panel_id)).is_none());
}

#[test]
fn r_workspace_stage1_selection_and_destructive_actions_are_distinct() {
    for tab in [BottomTab::Environment, BottomTab::PlotRegistry, BottomTab::LinkedPanels] {
        let mut harness = harness(tab, 320.0, 1.0, 5, false);
        match tab {
            BottomTab::Environment => harness.get_by_label("df").click(),
            BottomTab::PlotRegistry => harness.query_all_by_label("Select").next().unwrap().click(),
            _ => harness.query_all_by_label("Select / Focus").next().unwrap().click(),
        }
        harness.run_steps(3);
        let tokens = crate::theme::Tokens::get(&harness.ctx);
        assert!(
            harness
                .output()
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape, Shape::Rect(rect) if rect.fill == tokens.row_selected && rect.rect.width() > 280.0)),
            "selected entry has a full-row highlight"
        );
        if tab == BottomTab::Environment || tab == BottomTab::LinkedPanels {
            let destructive = if tab == BottomTab::Environment { "Clear All" } else { "Delete Artwork" };
            assert!(
                harness.output().shapes.iter().any(|shape| match &shape.shape {
                    Shape::Text(text) =>
                        text.galley.text() == destructive && text.galley.job.sections.iter().any(|section| section.format.color == tokens.error),
                    _ => false,
                }),
                "destructive actions use the theme's error color"
            );
            if tab == BottomTab::Environment {
                assert!(
                    !harness.output().shapes.iter().any(|shape| matches!(&shape.shape, Shape::Text(text) if text.galley.text() == "Remove")),
                    "variable rows use icon-only action buttons instead of visible text buttons"
                );
            }
        }
        let measurements = geometry(&harness, 320.0);
        capture(&mut harness, tab, 320.0, 1.0, "selected-entry", &measurements);
    }
}

fn workspace_harness(width: f32, height: f32, scale: f32) -> Harness<'static, crate::VectorcraftApp> {
    let mut app = fixture(5);
    app.scientific.r_status = format!("R detected: C:/{} Rscript.exe", "very_long_installation_path/".repeat(20));
    let mut h = Harness::builder()
        .with_size(vec2(width, height))
        .with_pixels_per_point(scale)
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_context_setup(|ctx| {
            egui_extras::install_image_loaders(ctx);
            crate::theme::install_fonts(ctx);
            crate::theme::apply(ctx, crate::theme::Brightness::MediumDark);
        })
        .build_ui_state(
            |ui, app: &mut crate::VectorcraftApp| {
                crate::chrome::app_bar(app, ui);
                show(app, ui);
                crate::scientific::linked_plot_protection::show(app, ui.ctx());
                app.ui_fonts.frame(ui.ctx());
                app.ui_fonts.finish(ui.ctx());
            },
            app,
        );
    h.run_steps(6);
    h
}

fn linked_workspace_harness(width: f32, height: f32, scale: f32) -> Harness<'static, crate::VectorcraftApp> {
    let mut h = workspace_harness(width, height, scale);
    let app = h.state_mut();
    let shape = app.run("shape.rectangle", json!({"x":20,"y":30,"width":100.0*DOC_POINTS_PER_MM,"height":80.0*DOC_POINTS_PER_MM})).unwrap()["id"]
        .as_u64()
        .unwrap();
    let generated = app.run("object.group", json!({"ids":[shape]})).unwrap()["id"].as_u64().unwrap();
    let label = app.run("text.create", json!({"x":20,"y":15,"text":"A","size":14})).unwrap()["id"].as_u64().unwrap();
    app.run("object.setName", json!({"id":generated,"name":"RVS_GENERATED"})).unwrap();
    app.run("object.setName", json!({"id":label,"name":"RVS_LABEL"})).unwrap();
    let panel_id = app.run("object.group", json!({"ids":[generated,label]})).unwrap()["id"].as_u64().unwrap();
    let panel = RLinkedPanel {
        panel_id,
        generated_id: generated,
        label: "A".into(),
        r_variable: "plot_A".into(),
        width_mm: 100.0,
        height_mm: 80.0,
        resize_mode: ResizeMode::Reflow,
        graph_style: Default::default(),
        small_figure_typography: false,
        document_uid: app.session.active().unwrap().uid,
    };
    app.run("object.setName", json!({"id":panel_id,"name":persistent_panel_name(&panel,100.0,80.0,ResizeMode::Reflow)})).unwrap();
    app.scientific.linked_panels.push(panel);
    app.run("select.none", json!({})).unwrap();
    h.run_steps(4);
    h
}

fn workspace_measurements(h: &Harness<'_, crate::VectorcraftApp>, width: f32, height: f32) -> Value {
    let g = h.ctx.data(|d| d.get_temp::<WorkspaceGeometry>(workspace_geometry_id())).unwrap();
    let left = g.workspace.unwrap();
    let right = g.vector.unwrap();
    let divider = g.divider.unwrap();
    let canvas = h.state().canvas_rect.unwrap();
    assert!(left.width() > 150.0 && right.width() > 200.0 && canvas.width() > 0.0);
    for r in [left, right, divider, canvas] {
        assert!(r.left() >= -1.0 && r.right() <= width + 1.0 && r.bottom() <= height + 1.0, "workspace bounds {r:?} outside {width}x{height}");
    }
    assert!(left.intersect(right).width() <= 1.0);
    assert!((divider.left() - left.right()).abs() < 2.0);
    assert!(g.toolbar.unwrap().height() < 110.0, "status path must not expand the toolbar");
    let rect = |r: Rect| json!([r.left(), r.top(), r.width(), r.height()]);
    json!({"mode":"headless-full-workspace", "width":width,"height":height,"scale":h.ctx.pixels_per_point(),
        "workspaceRect":rect(left),"vectorRect":rect(right),"canvasRect":rect(canvas),"toolbarRect":rect(g.toolbar.unwrap()),
        "horizontalOverflow":false,"controlOverlap":false})
}

#[test]
fn r_workspace_stage3_full_workspace_matrix() {
    for (width, height) in [(640.0, 720.0), (900.0, 720.0), (1280.0, 900.0), (1600.0, 1000.0)] {
        for scale in [1.0, 1.5] {
            let mut h = workspace_harness(width, height, scale);
            let measurements = workspace_measurements(&h, width, height);
            capture(&mut h, BottomTab::Console, width, scale, "stage3-main-workspace", &measurements);
        }
    }
}

fn drag_splitter(h: &mut Harness<'_, crate::VectorcraftApp>, label: &str, delta: egui::Vec2) {
    let start = h.get_by_label(label).rect().center();
    h.hover_at(start);
    h.run_steps(1);
    h.drag_at(start);
    h.run_steps(1);
    for step in 1..=6 {
        h.hover_at(start + delta * (step as f32 / 6.0));
        h.run_steps(1);
    }
    h.drop_at(start + delta);
    h.run_steps(4);
}

#[test]
fn r_workspace_stage3_splitters_drag_keyboard_and_restore() {
    let prefs = crate::state::UiState { r_studio_workspace_width: 1000.0, ..Default::default() };
    let restored: crate::state::UiState = serde_json::from_slice(&serde_json::to_vec(&prefs).unwrap()).unwrap();
    assert_eq!(restored.sanitized().r_studio_workspace_width, 1000.0, "saved wide workspaces must survive startup sanitization");
    let mut h = workspace_harness(1280.0, 900.0, 1.0);
    drag_splitter(&mut h, "Resize R workspace", vec2(80.0, 0.0));
    assert!((h.state().ui.r_studio_workspace_width - 500.0).abs() < 2.0);
    h.get_by_label("Resize R workspace").click();
    h.run_steps(2);
    let ctx = h.ctx.clone();
    assert!(handle_shortcuts(h.state_mut(), &ctx), "divider focus must keep arrow keys out of canvas routing");
    h.key_press(egui::Key::ArrowRight);
    h.run_steps(3);
    assert!((h.state().ui.r_studio_workspace_width - 516.0).abs() < 2.0);
    let before = h.ctx.read_response(h.state().scientific.editor_widget_ids[&1]).unwrap().interact_rect.height();
    drag_splitter(&mut h, "Resize Source and Console", vec2(0.0, -60.0));
    let after = h.ctx.read_response(h.state().scientific.editor_widget_ids[&1]).unwrap().interact_rect.height();
    assert!((before - after - 60.0).abs() < 3.0, "vertical movement must match drag: {before} -> {after}");
    let saved = h.state().ui.r_studio_workspace_width;
    h.set_size(vec2(640.0, 720.0));
    h.run_steps(6);
    workspace_measurements(&h, 640.0, 720.0);
    assert_eq!(h.state().ui.r_studio_workspace_width, saved, "window clamping must retain the preferred width");
    h.set_size(vec2(1280.0, 900.0));
    h.run_steps(5);
    let measurements = workspace_measurements(&h, 1280.0, 900.0);
    capture(&mut h, BottomTab::Console, 1280.0, 1.0, "stage3-dividers-resized", &measurements);
    h.get_by_label("Window").click();
    h.run_steps(2);
    h.get_by_label("R Studio").hover();
    h.run_steps(2);
    h.get_by_label_contains("Reset R Pane Sizes").click();
    h.run_steps(4);
    assert_eq!(h.state().ui.r_studio_workspace_width, 420.0);
    assert_eq!(h.state().ui.r_studio_editor_fraction, 0.62);
    h.get_by_label("Maximize editor").click();
    h.run_steps(4);
    assert!(h.state().ui.r_studio_editor_maximized);
    assert!(h.query_by_label("Resize Source and Console").is_none());
    h.get_by_label("Restore editor").click();
    h.run_steps(4);
    assert!(!h.state().ui.r_studio_editor_maximized);
    assert!(h.query_by_label("Resize Source and Console").is_some());
}

#[test]
fn r_workspace_stage3_menus_busy_state_and_tools() {
    let mut h = workspace_harness(1280.0, 900.0, 1.0);
    h.get_by_label("File").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("New R Script").is_some());
    assert!(h.query_by_label_contains("Open R Script…").is_some());
    assert!(h.query_by_label("Save R Script ").is_some());
    assert!(h.query_by_label_contains("Save R Script As…").is_some());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    h.get_by_label("R").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("Update All Panels").is_some());
    assert!(h.get_by_label_contains("Stop R").accesskit_node().is_disabled());
    h.state_mut().scientific.running = true;
    h.run_steps(2);
    assert!(
        !h.get_by_label_contains("Run Selection or Expression").accesskit_node().is_disabled(),
        "R selections can queue while an execution is running"
    );
    assert!(h.get_by_label_contains("Source Active Script").accesskit_node().is_disabled());
    assert!(h.get_by_label_contains("Update All Panels").accesskit_node().is_disabled());
    assert!(h.get_by_label_contains("Restart R").accesskit_node().is_disabled());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    h.state_mut().scientific.running = false;
    h.state_mut().run("window.toolbarColumns", json!({"double":true})).unwrap();
    h.run_steps(4);
    assert_eq!(crate::toolbar::embedded_width(h.state()), 88.0);
    let measurements = workspace_measurements(&h, 1280.0, 900.0);
    capture(&mut h, BottomTab::Console, 1280.0, 1.0, "stage3-double-column-tools", &measurements);
    h.state_mut().run("window.panel.float", json!({"panel":"tools","x":600,"y":150})).unwrap();
    h.run_steps(4);
    assert_eq!(crate::toolbar::embedded_width(h.state()), 0.0);
    assert!(h.ctx.data(|d| d.get_temp::<WorkspaceGeometry>(workspace_geometry_id())).unwrap().tools.is_none());
    let measurements = workspace_measurements(&h, 1280.0, 900.0);
    capture(&mut h, BottomTab::Console, 1280.0, 1.0, "stage3-floating-tools", &measurements);
    h.state_mut().run("window.panel.dock", json!({"panel":"tools"})).unwrap();
    h.run_steps(3);
    assert_eq!(crate::toolbar::embedded_width(h.state()), 88.0);
}

#[test]
fn r_workspace_stage3_toolbar_density_and_status() {
    for width in WIDTHS {
        for scale in [1.0, 1.5] {
            let mut app = fixture(0);
            app.scientific.r_status = format!("R detected: {}", "C:/한글/very-long-directory/".repeat(100));
            let mut h = Harness::builder()
                .with_size(vec2(width, HEIGHT))
                .with_pixels_per_point(scale)
                .allow_missing_glyphs()
                .with_accessibility_check(false)
                .with_context_setup(|ctx| {
                    crate::theme::install_fonts(ctx);
                    crate::theme::apply(ctx, crate::theme::Brightness::MediumDark);
                })
                .build_ui_state(
                    |ui, app: &mut crate::VectorcraftApp| {
                        ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);
                        let rect = ui
                            .horizontal(|ui| {
                                panel_caption(ui, "Source");
                                if ui.small_button(if app.ui.r_studio_editor_maximized { "Restore editor" } else { "Maximize editor" }).clicked() {
                                    app.ui.r_studio_editor_maximized = !app.ui.r_studio_editor_maximized;
                                }
                                show_status_indicator(app, ui);
                            })
                            .response
                            .rect;
                        assert!(rect.height() < 150.0);
                        app.ui_fonts.frame(ui.ctx());
                        app.ui_fonts.finish(ui.ctx());
                    },
                    app,
                );
            h.run_steps(5);
            assert!(h.query_by_label("● R: Available").is_some());
            let measurements = geometry(&h, width);
            capture(&mut h, BottomTab::Console, width, scale, "stage3-toolbar-density", &measurements);
        }
    }
}

#[test]
fn r_workspace_stage4_inspector_identifies_link_and_preserves_vector_controls() {
    for (width, height) in [(900.0, 720.0), (1280.0, 900.0)] {
        for scale in [1.0, 1.5] {
            let mut h = linked_workspace_harness(width, height, scale);
            assert!(h.query_by_label("R-linked plot · Panel A").is_none());
            let panel = h.state().scientific.linked_panels[0].panel_id;
            h.state_mut().run("select.set", json!({"ids":[panel]})).unwrap();
            h.run_steps(5);
            h.state_mut().run("window.panel.activate", json!({"panel": "properties"})).unwrap();
            h.run_steps(2);
            assert!(h.query_by_label("R-linked plot · Panel A").is_some(), "linked selection appears in native Inspector");
            for label in ["plot_A", "R plot size · mm", "Update from R", "Apply Resize", "Vector transform (document points)"] {
                assert!(h.query_all_by_label(label).next().is_some(), "missing Inspector control {label}");
            }
            let geometry = workspace_measurements(&h, width, height);
            capture(&mut h, BottomTab::Console, width, scale, "stage4-inspector-linked-selection", &geometry);
            h.state_mut().run("select.none", json!({})).unwrap();
            h.run_steps(3);
            assert!(h.query_by_label("R-linked plot · Panel A").is_none());
            let ordinary = h.state_mut().run("shape.rectangle", json!({"x":300,"y":200,"width":40,"height":30})).unwrap()["id"].as_u64().unwrap();
            h.state_mut().run("select.set", json!({"ids":[ordinary]})).unwrap();
            h.run_steps(3);
            assert!(h.query_by_label("R-linked plot · Panel A").is_none(), "ordinary artwork retains normal Inspector");
        }
    }
}

#[test]
fn r_workspace_stage4_legacy_scale_metadata_loads_as_reflow() {
    let mut h = linked_workspace_harness(1280.0, 900.0, 1.0);
    let panel = h.state().scientific.linked_panels[0].panel_id;
    let old_name = format!("RVS_PANEL|v3|A|{}|120.000000|90.000000|scale|original", encode_name("plot_A"));
    h.state_mut().run("object.setName", json!({"id": panel, "name": old_name})).unwrap();
    restore_linked_panels(h.state_mut());
    let link = &h.state().scientific.linked_panels[0];
    assert_eq!(link.panel_id, panel);
    assert_eq!((link.width_mm, link.height_mm, link.resize_mode), (120.0, 90.0, ResizeMode::Reflow));
    assert_eq!(link.r_variable, "plot_A");
    assert!(h.state().session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(panel)).is_some());
    h.state_mut().run("select.set", json!({"ids":[panel]})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().scientific.properties_width_mm, 120.0, "reselection reloads stored dimensions");
    assert!(h.query_by_label("Scale").is_none());
}

#[test]
fn graph_style_metadata_restores_with_panel_identity_and_reflow_dimensions() {
    let mut h = linked_workspace_harness(1280.0, 900.0, 1.0);
    let mut panel = h.state().scientific.linked_panels[0].clone();
    panel.graph_style.plot_title_pt = Some(18.0);
    panel.graph_style.legend_position = Some(crate::scientific::graph_style::LegendPosition::Left);
    h.state_mut()
        .run("object.setName", json!({"id": panel.panel_id, "name": persistent_panel_name(&panel, 120.0, 90.0, ResizeMode::Reflow)}))
        .unwrap();
    h.state_mut().scientific.linked_panels.clear();
    restore_linked_panels(h.state_mut());
    let restored = &h.state().scientific.linked_panels[0];
    assert_eq!((restored.panel_id, restored.generated_id), (panel.panel_id, panel.generated_id));
    assert_eq!((restored.width_mm, restored.height_mm), (120.0, 90.0));
    assert_eq!(restored.graph_style, panel.graph_style);
    assert_eq!(restored.resize_mode, ResizeMode::Reflow);
    let document = &h.state().session.doc().unwrap().doc;
    let reopened = vectorcraft_format::load_file(&vectorcraft_format::save_file(document)).unwrap();
    let saved_name = reopened.doc.node(vectorcraft_doc::NodeId(panel.panel_id)).unwrap().name.as_deref().unwrap();
    assert!(saved_name.starts_with("RVS_PANEL|v4|"));
    assert!(saved_name.contains(&encode_name(&serde_json::to_string(&panel.graph_style).unwrap())));
}

#[test]
fn graph_dock_shows_panel_controls_and_empty_state_at_narrow_width() {
    let mut h = linked_workspace_harness(900.0, 720.0, 1.0);
    let panel = h.state().scientific.linked_panels[0].panel_id;
    h.state_mut().run("select.set", json!({"ids": [panel]})).unwrap();
    h.run_steps(5);
    assert!(crate::panel_docking::showing(&h.state().ui, "graph"), "selecting a linked plot focuses Graph");
    for label in [
        "Plot Registry (2)",
        "Linked Panels (1)",
        "Selected Graph Settings",
        "Add to Artboard",
        "Update All",
        "Select / Focus",
        "Go to Source",
        "Relink",
        "Unlink",
        "Delete Artwork",
        "Figure Size",
        "Axes",
        "Typography",
        "Legend",
        "Reset Graph Styles to R",
        "X-axis tick labels",
        "X-axis title",
    ] {
        assert!(h.query_all_by_label(label).next().is_some(), "missing Graph control {label}");
    }
    assert!(h.query_by_label("Linked Plot Panel").is_none());
    assert!(h.query_by_label("Scale").is_none());
    h.state_mut().run("select.none", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Select an R-linked plot on the artboard to edit its graph settings.").is_some());
}

#[test]
fn graph_visibility_control_and_apply_queue_keep_font_size() {
    let mut h = linked_workspace_harness(900.0, 720.0, 1.0);
    let panel = h.state().scientific.linked_panels[0].panel_id;
    let key = (h.state().scientific.linked_panels[0].document_uid, panel);
    h.state_mut().scientific.graph_drafts.insert(key, crate::scientific::graph_style::GraphStyle { tick_x_pt: Some(15.0), ..Default::default() });
    h.state_mut().run("select.set", json!({"ids": [panel]})).unwrap();
    // Keep the fixture's synthetic R plot from starting a real render.
    h.state_mut().scientific.running = true;
    h.run_steps(5);
    h.get_by_label("X-axis tick labels").click_accesskit();
    h.run_steps(2);
    assert!(h.query_by_role_and_label(Role::ComboBox, "X-axis tick labels visibility").is_some());
    let panel = h.state().scientific.linked_panels[0].clone();
    let mut style = h.state().scientific.graph_drafts.get(&key).unwrap().clone();
    style.tick_x_visible = Some(false);
    request_graph_apply(&mut h.state_mut().scientific, &panel, style, panel.width_mm, panel.height_mm).unwrap();
    h.run_steps(2);
    let state = h.state();
    let request = state.scientific.graph_pending.get(&key).unwrap_or_else(|| {
        panic!("visibility click should queue a render; status={}; draft={:?}", state.scientific.r_status, state.scientific.graph_drafts.get(&key))
    });
    assert_eq!(request.style.tick_x_visible, Some(false));
    assert_eq!(request.style.tick_x_pt, Some(15.0));
    assert!(state.scientific.running);
}

#[test]
fn graph_numeric_edit_waits_for_enter_before_queuing_render() {
    let mut h = linked_workspace_harness(900.0, 720.0, 1.0);
    let panel = h.state().scientific.linked_panels[0].panel_id;
    let key = (h.state().scientific.linked_panels[0].document_uid, panel);
    h.state_mut().scientific.graph_drafts.insert(key, crate::scientific::graph_style::GraphStyle { axis_x_line_mm: Some(0.5), ..Default::default() });
    h.state_mut().run("select.set", json!({"ids": [panel]})).unwrap();
    h.state_mut().scientific.running = true;
    h.run_steps(5);
    h.get_by_role_and_label(Role::SpinButton, "X line width").click();
    h.run_steps(2);
    let input = h.get_by_role_and_label(Role::SpinButton, "X line width");
    input.focus();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    input.type_text("1.7");
    h.run_steps(2);
    assert!(!h.state().scientific.graph_pending.contains_key(&key), "typing must not render");
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    let request = h.state().scientific.graph_pending.get(&key).expect("Enter should queue render");
    assert_eq!(request.style.axis_x_line_mm, Some(1.7));
    h.get_by_role_and_label(Role::SpinButton, "X line width").click();
    h.run_steps(2);
    let input = h.get_by_role_and_label(Role::SpinButton, "X line width");
    input.focus();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    input.type_text("2.1");
    h.run_steps(2);
    assert_eq!(h.state().scientific.graph_pending.get(&key).and_then(|request| request.style.axis_x_line_mm), Some(1.7));
    if let Some(focused) = h.ctx.memory(|memory| memory.focused()) {
        h.ctx.memory_mut(|memory| memory.surrender_focus(focused));
    }
    h.run_steps(2);
    assert_eq!(h.state().scientific.graph_pending.get(&key).and_then(|request| request.style.axis_x_line_mm), Some(2.1));
}

#[test]
fn graph_size_draft_stays_with_ma_panel_while_pca_is_selected() {
    let mut h = linked_workspace_harness(900.0, 720.0, 1.0);
    {
        let app = h.state_mut();
        let shape = app
            .run("shape.rectangle", json!({"x": 180, "y": 150, "width": 70.0 * DOC_POINTS_PER_MM, "height": 50.0 * DOC_POINTS_PER_MM}))
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let generated = app.run("object.group", json!({"ids": [shape]})).unwrap()["id"].as_u64().unwrap();
        let label = app.run("text.create", json!({"x": 180, "y": 140, "text": "B", "size": 14})).unwrap()["id"].as_u64().unwrap();
        app.run("object.setName", json!({"id": generated, "name": "RVS_GENERATED"})).unwrap();
        app.run("object.setName", json!({"id": label, "name": "RVS_LABEL"})).unwrap();
        let panel_id = app.run("object.group", json!({"ids": [generated, label]})).unwrap()["id"].as_u64().unwrap();
        let panel = RLinkedPanel {
            panel_id,
            generated_id: generated,
            label: "B".into(),
            r_variable: "plot_A".into(),
            width_mm: 70.0,
            height_mm: 50.0,
            resize_mode: ResizeMode::Reflow,
            graph_style: Default::default(),
            small_figure_typography: false,
            document_uid: app.session.active().unwrap().uid,
        };
        app.run("object.setName", json!({"id": panel_id, "name": persistent_panel_name(&panel, 70.0, 50.0, ResizeMode::Reflow)})).unwrap();
        app.scientific.linked_panels.push(panel);
    }
    h.run_steps(3);
    let pca = h.state().scientific.linked_panels[0].clone();
    let ma = h.state().scientific.linked_panels[1].clone();
    h.state_mut().run("select.set", json!({"ids": [ma.panel_id]})).unwrap();
    h.state_mut().scientific.running = true;
    h.run_steps(5);
    h.get_by_label("Plot Registry (2)").click_accesskit();
    h.get_by_label("Linked Panels (2)").click_accesskit();
    h.run_steps(3);
    let input = h.get_all_by_role_and_label(Role::SpinButton, "mm").next().expect("Graph width input");
    input.click();
    h.run_steps(2);
    let input = h.get_all_by_role_and_label(Role::SpinButton, "mm").next().unwrap();
    input.focus();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    input.type_text("120");
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    let ma_key = (ma.document_uid, ma.panel_id);
    assert_eq!(h.state().scientific.graph_pending.get(&ma_key).map(|request| request.width_mm), Some(120.0));
    h.state_mut().run("select.set", json!({"ids": [pca.panel_id]})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().scientific.properties_width_mm, pca.width_mm);
    h.state_mut().run("select.set", json!({"ids": [ma.panel_id]})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().scientific.properties_width_mm, 120.0, "MA's committed width must survive selection changes while rendering");
    let height = h.get_all_by_role_and_label(Role::SpinButton, "mm").nth(1).expect("Graph height input");
    height.click();
    h.run_steps(2);
    let height = h.get_all_by_role_and_label(Role::SpinButton, "mm").nth(1).unwrap();
    height.focus();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    height.type_text("80");
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    let request = h.state().scientific.graph_pending.get(&ma_key).unwrap();
    assert_eq!((request.width_mm, request.height_mm), (120.0, 80.0));
    assert!(!h.state().scientific.graph_pending.contains_key(&(pca.document_uid, pca.panel_id)));
}

#[test]
fn legacy_plot_tab_commands_open_graph_and_keep_bottom_console() {
    let mut h = linked_workspace_harness(900.0, 720.0, 1.0);
    for old in ["plots", "linked_panels"] {
        let result = h.state_mut().run("window.rTab", json!({"tab": old})).unwrap();
        assert_eq!(result["tab"], "graph");
        assert_eq!(h.state().ui.r_studio_tab, "console");
        assert!(crate::panel_docking::showing(&h.state().ui, "graph"));
    }
    configure_workspace(h.state_mut(), &json!({"rTab": "plots"})).unwrap();
    assert_eq!(h.state().ui.r_studio_tab, "console");
    assert!(crate::panel_docking::showing(&h.state().ui, "graph"));
}

#[test]
fn graph_linked_panel_unlink_and_delete_keep_confirmation_and_artwork_policy() {
    let mut h = linked_workspace_harness(900.0, 720.0, 1.0);
    h.state_mut().run("window.panel.activate", json!({"panel": "graph"})).unwrap();
    h.run_steps(3);
    let panel = h.state().scientific.linked_panels[0].panel_id;
    h.get_by_label("Unlink").click();
    h.run_steps(2);
    assert!(h.state().session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(panel)).is_some());
    h.get_by_label("Unlink & Edit").click();
    h.run_steps(2);
    assert!(h.state().scientific.linked_panels.is_empty());
    assert!(h.state().session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(panel)).is_some());

    let mut h = linked_workspace_harness(900.0, 720.0, 1.0);
    h.state_mut().run("window.panel.activate", json!({"panel": "graph"})).unwrap();
    h.run_steps(3);
    let panel = h.state().scientific.linked_panels[0].panel_id;
    h.get_by_label("Delete Artwork").click();
    h.run_steps(2);
    assert!(h.state().session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(panel)).is_some());
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.state().session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(panel)).is_some());
    h.get_by_label("Delete Artwork").click();
    h.run_steps(2);
    h.get_by_label("Confirm Delete").click();
    h.run_steps(2);
    assert!(h.state().session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(panel)).is_none());
}

#[test]
fn r_workspace_stage4_inspector_tracks_link_metadata_without_erasing_a_draft() {
    let mut h = linked_workspace_harness(1280.0, 900.0, 1.0);
    let panel = h.state().scientific.linked_panels[0].clone();
    h.state_mut().run("select.set", json!({"ids":[panel.panel_id]})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().scientific.properties_width_mm, 100.0);
    let mut changed = panel.clone();
    changed.width_mm = 110.0;
    changed.height_mm = 85.0;
    h.state_mut().run("object.setName", json!({"id":panel.panel_id,"name":persistent_panel_name(&changed,110.0,85.0,ResizeMode::Reflow)})).unwrap();
    h.run_steps(3);
    assert_eq!((h.state().scientific.properties_width_mm, h.state().scientific.properties_height_mm), (110.0, 85.0));
    h.state_mut().scientific.properties_width_mm = 120.0;
    changed.width_mm = 130.0;
    h.state_mut().run("object.setName", json!({"id":panel.panel_id,"name":persistent_panel_name(&changed,130.0,85.0,ResizeMode::Reflow)})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().scientific.properties_width_mm, 120.0, "editing draft survives external linked metadata change");
    h.state_mut().run("select.none", json!({})).unwrap();
    h.run_steps(2);
    h.state_mut().run("select.set", json!({"ids":[panel.panel_id]})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().scientific.properties_width_mm, 130.0, "reselection starts from stored dimensions");
}

#[test]
fn r_workspace_stage5_narrow_selection_actions_stay_on_canvas() {
    for width in [640.0, 900.0, 1280.0] {
        let mut h = linked_workspace_harness(width, 720.0, 1.0);
        let panel = h.state().scientific.linked_panels[0].panel_id;
        h.state_mut().run("select.set", json!({"ids":[panel]})).unwrap();
        h.run_steps(5);
        let canvas = h.state().canvas_rect.unwrap();
        let bar = crate::canvas::task_bar_rect(&h.ctx).unwrap();
        assert!(
            bar.left() >= canvas.left() - 1.0 && bar.right() <= canvas.right() + 1.0,
            "context actions must stay in canvas at {width}: {bar:?} / {canvas:?}"
        );
        if width <= 900.0 {
            h.get_by_label("Selection actions").click();
            h.run_steps(2);
            let labels: Vec<_> =
                h.root().children_recursive().filter_map(|node| node.accesskit_node().label()).map(|label| label.trim().to_owned()).collect();
            for action in ["Ungroup", "Isolate", "Duplicate"] {
                assert!(labels.iter().any(|label| label == action), "compact selection menu lacks {action}: {labels:?}");
            }
            h.key_press(egui::Key::Escape);
            h.run_steps(2);
        }
        assert!(h.state().session.doc().unwrap().doc.node(vectorcraft_doc::NodeId(panel)).is_some());
        let measurements = workspace_measurements(&h, width, 720.0);
        capture(&mut h, BottomTab::Console, width, 1.0, "stage5-selection-actions-contained", &measurements);
    }
}

#[test]
fn r_workspace_stage5_narrow_properties_remain_reachable() {
    let mut h = workspace_harness(640.0, 720.0, 1.0);
    let initial_left = h.query_all_by_label("Document Setup").next().unwrap().rect().left();
    h.get_by_label("→").click();
    h.run_steps(3);
    let after_right = h.query_all_by_label("Document Setup").next().unwrap().rect().left();
    assert!(after_right < initial_left - 20.0, "visible Inspector arrow should expose right-side controls");
    let edit = h.get_by_label("Edit Artboards");
    assert!(
        edit.rect().center().x >= h.state().canvas_rect.unwrap().right() && edit.rect().center().x <= 640.0,
        "right-side Inspector action should be reachable after scrolling: {:?}",
        edit.rect()
    );
    h.get_by_label("←").click();
    h.run_steps(3);
    let setup = h.query_all_by_label("Document Setup").next().unwrap();
    assert!(
        setup.rect().center().x >= h.state().canvas_rect.unwrap().right() && setup.rect().center().x <= 640.0,
        "left-side Inspector action should be reachable after scrolling back: {:?}",
        setup.rect()
    );
    let measurements = workspace_measurements(&h, 640.0, 720.0);
    capture(&mut h, BottomTab::Console, 640.0, 1.0, "stage5-narrow-inspector-reachable", &measurements);
}

fn source_harness(width: f32, scale: f32) -> Harness<'static, crate::VectorcraftApp> {
    let mut app = fixture(5);
    app.scientific.scripts[0] = ScriptDocument::new(
        1,
        "# 한글 source\n\nplot_A <- ggplot(df, aes(x, y)) +\n  geom_point(color = \"#377EB8\")\n".into(),
        Some("C:/test/분석_very_long_source_filename_for_narrow_tabs.R".into()),
        None,
    );
    app.scientific.scripts.push(ScriptDocument::new(2, "value <- 42\n".into(), Some("C:/test/second.R".into()), None));
    app.scientific.scripts[0].text.push_str("# unsaved\n");
    app.scientific.next_script_id = 3;
    let mut harness = Harness::builder()
        .with_size(vec2(width, HEIGHT))
        .with_pixels_per_point(scale)
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_context_setup(|ctx| {
            crate::theme::install_fonts(ctx);
            crate::theme::apply(ctx, crate::theme::Brightness::MediumDark);
        })
        .build_ui_state(
            |ui, app: &mut crate::VectorcraftApp| {
                handle_shortcuts(app, ui.ctx());
                ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);
                show_script_tabs(app, ui);
                show_source_editor(&mut app.scientific, ui, 340.0);
                show_bottom_tabs(app, ui);
                app.ui_fonts.frame(ui.ctx());
                app.ui_fonts.finish(ui.ctx());
            },
            app,
        );
    harness.run_steps(5);
    harness
}

#[test]
fn r_workspace_source_navigation_keeps_first_column_visible_and_focuses_target_tab() {
    for scale in [1.0, 1.5, 2.0] {
        let mut h = source_harness(560.0, scale);
        let long_line = format!("{}\n\nplot_B <- 2\n", "x".repeat(180));
        h.state_mut().scientific.scripts[1].text = long_line.clone();
        h.run_steps(3);
        assert!(navigate_source(&mut h.state_mut().scientific, 2, 150));
        h.run_steps(4);
        let id = h.state().scientific.editor_widget_ids[&2];
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(id));
        assert_eq!(
            egui::TextEdit::load_state(&h.ctx, id).and_then(|state| state.cursor.char_range()).map(|range| usize::from(range.primary.index)),
            Some(150)
        );
        assert!(navigate_source(&mut h.state_mut().scientific, 2, long_line.find("plot_B").unwrap()));
        h.run_steps(4);
        assert_eq!(
            egui::TextEdit::load_state(&h.ctx, id).and_then(|state| state.cursor.char_range()).map(|range| usize::from(range.primary.index)),
            Some(182)
        );
        h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::COMMAND, egui::Key::Home);
        h.run_steps(4);
        assert_eq!(
            egui::TextEdit::load_state(&h.ctx, id).and_then(|state| state.cursor.char_range()).map(|range| usize::from(range.primary.index)),
            Some(0)
        );
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(id));
        let measurements = measured_geometry(&h, 560.0, true);
        capture(&mut h, BottomTab::Console, 560.0, scale, "source-column-one-navigation", &measurements);
    }
}

#[test]
fn r_workspace_go_to_source_reveals_distant_lines_in_both_directions_and_other_tabs() {
    let mut h = source_harness(560.0, 1.0);
    let mut lines: Vec<String> = (1..=600).map(|line| format!("# line {line}")).collect();
    lines[11] = "plot_top <- 1".into();
    lines[509] = "plot_bottom <- 2".into();
    let first = lines.join("\n");
    let second =
        (1..=600).map(|line| if line == 530 { "plot_other <- 3".to_owned() } else { format!("# other {line}") }).collect::<Vec<_>>().join("\n");
    h.state_mut().scientific.scripts[0].text = first.clone();
    h.state_mut().scientific.scripts[1].text = second.clone();
    for (name, script_id, line, source_revision) in
        [("plot_top", 1, 11, first.clone()), ("plot_bottom", 1, 509, first), ("plot_other", 2, 529, second)]
    {
        h.state_mut().scientific.source_origins.insert(name.into(), SourceOrigin { script_id, line, source_revision });
    }
    h.run_steps(3);
    let visible = |h: &Harness<'_, crate::VectorcraftApp>, number: &str| {
        h.output().shapes.iter().any(|shape| {
            matches!(&shape.shape, Shape::Text(text)
                if text.galley.text() == number && shape.clip_rect.intersects(text.visual_bounding_rect()))
        })
    };
    go_to_plot_source(&mut h.state_mut().scientific, "plot_bottom");
    h.run_steps(7);
    assert!(visible(&h, "510"), "downward source jump must reveal line 510");
    assert!(!visible(&h, "12"));
    assert!(h.state().scientific.scripts[0].pending_reveal_index.is_none());
    go_to_plot_source(&mut h.state_mut().scientific, "plot_top");
    h.run_steps(7);
    assert!(visible(&h, "12"), "upward source jump must reveal line 12");
    assert!(!visible(&h, "510"));
    h.ctx.memory_mut(|memory| memory.surrender_focus(h.state().scientific.editor_widget_ids[&1]));
    go_to_plot_source(&mut h.state_mut().scientific, "plot_other");
    h.run_steps(7);
    assert_eq!(h.state().scientific.active_script_id, 2);
    assert!(visible(&h, "530"), "cross-tab source jump must reveal line 530");
    assert!(h.state().scientific.scripts[1].pending_reveal_index.is_none());
}

#[test]
fn r_workspace_stage2_source_and_console_matrix() {
    for width in WIDTHS {
        for scale in [1.0, 1.5] {
            let mut h = source_harness(width, scale);
            assert_eq!(h.state().scientific.active_script_id, 1);
            assert!(h.query_by_label_contains("* 분석_").is_some());
            let accent = crate::theme::Tokens::get(&h.ctx).accent;
            assert!(
                h.output()
                    .shapes
                    .iter()
                    .any(|s| matches!(&s.shape, Shape::LineSegment { stroke, .. } if stroke.color == accent && stroke.width == 2.0))
            );
            let measurements = measured_geometry(&h, width, true);
            capture(&mut h, BottomTab::Console, width, scale, "source-tabs-gutter", &measurements);
            h.get_by_label("second.R").scroll_to_me();
            h.run_steps(3);
            h.get_by_label("second.R").click();
            h.run_steps(3);
            assert_eq!(h.state().scientific.active_script_id, 2);
            let id = h.state().scientific.editor_widget_ids[&2];
            assert_eq!(h.ctx.memory(|m| m.focused()), Some(id));
            let measurements = measured_geometry(&h, width, true);
            capture(&mut h, BottomTab::Console, width, scale, "source-second-tab", &measurements);
            h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Tab);
            h.run_steps(3);
            assert_eq!(h.state().scientific.active_script_id, 1);
        }
    }
    let mut h = source_harness(800.0, 1.0);
    h.state_mut().scientific.activate_script(2);
    h.run_steps(8);
    h.set_size(vec2(320.0, HEIGHT));
    h.run_steps(12);
    let close = h.query_all_by_label("x").last().unwrap().rect();
    assert!(close.left() >= 0.0 && close.right() <= 320.0, "narrowing must reveal the active tab's close action: {close:?}");
    let measurements = measured_geometry(&h, 320.0, true);
    capture(&mut h, BottomTab::Console, 320.0, 1.0, "source-narrowed-active-tab", &measurements);
}

#[test]
fn r_workspace_stage2_source_scroll_indentation_and_dirty_close() {
    let mut h = source_harness(320.0, 1.0);
    let original = (1..=150).map(|i| format!("# line {i}: 한글\n")).collect::<String>();
    h.state_mut().scientific.scripts[0].text = original.clone();
    h.run_steps(3);
    let id = h.state().scientific.editor_widget_ids[&1];
    h.ctx.memory_mut(|m| m.request_focus(id));
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::COMMAND, egui::Key::End);
    h.run_steps(5);
    let visible_number = |h: &Harness<'_, crate::VectorcraftApp>, number: &str| {
        h.output().shapes.iter().any(|s| {
            matches!(&s.shape, Shape::Text(t)
            if t.galley.text() == number && s.clip_rect.intersects(t.visual_bounding_rect()))
        })
    };
    assert!(visible_number(&h, "150"), "last line number must scroll into view with its code");
    assert!(!visible_number(&h, "1"), "top line must scroll away");
    let measurements = measured_geometry(&h, 320.0, true);
    capture(&mut h, BottomTab::Console, 320.0, 1.0, "source-150-lines-scrolled", &measurements);
    assert_eq!(h.state().scientific.scripts[0].text, original);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Tab);
    h.run_steps(3);
    assert_eq!(h.state().scientific.active_script_id, 2);
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Tab);
    h.run_steps(3);
    assert!(visible_number(&h, "150"), "tab switch must retain the document scroll/caret");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::W);
    h.run_steps(2);
    assert_eq!(h.state().scientific.pending_close_script_id, Some(1));
    assert_eq!(h.state().scientific.scripts.len(), 2);
    // Indentation remains the existing editor behavior, never a source-string transform.
    h.state_mut().scientific.pending_close_script_id = None;
    h.state_mut().scientific.scripts[0].text = "    value <- 42".into();
    h.ctx.memory_mut(|m| m.request_focus(id));
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::COMMAND, egui::Key::Home);
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Tab);
    h.run_steps(3);
    assert_eq!(h.state().scientific.scripts[0].text, "  value <- 42", "preserve the existing two-space outdent");
}

#[test]
fn r_workspace_stage2_console_scrollback_keeps_input_and_focus() {
    let mut h = harness(BottomTab::Console, 320.0, 1.0, 5, true);
    h.state_mut().scientific.console_output = format!("{}\nshort\n", "long_output_".repeat(200));
    h.run_steps(5);
    let id = h.state().scientific.console_input_id.unwrap();
    h.ctx.memory_mut(|m| m.request_focus(id));
    h.event(egui::Event::Text("draft".into()));
    h.run_steps(12);
    let input = h.ctx.read_response(id).unwrap().interact_rect;
    assert!(input.left() >= 0.0 && input.width() >= 200.0, "revealing input must not scroll to a long output line's tail: {input:?}");
    assert!(
        h.get_by_label("short").rect().left() >= h.get_by_label("Clear console").rect().left() - 1.0,
        "revealing input must retain the first column of transcript text"
    );
    h.state_mut().scientific.console_output = (0..500).map(|i| format!("output {i}: 한글\n")).collect();
    h.run_steps(5);
    h.hover_at(egui::pos2(120.0, 180.0));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 20_000.0),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
    });
    h.run_steps(5);
    assert!(!h.state().scientific.console_scroll_at_bottom);
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(id));
    assert_eq!(h.state().scientific.console_input_id, Some(id));
    assert!(h.ctx.read_response(id).is_some(), "live widget must still exist during scrollback");
    h.state_mut().scientific.console_output.push_str("new output while browsing\n");
    h.run_steps(3);
    assert!(!h.state().scientific.console_scroll_at_bottom, "output must not steal the scroll position");
    let measurements = geometry(&h, 320.0);
    capture(&mut h, BottomTab::Console, 320.0, 1.0, "scrollback-stable-input-focus", &measurements);
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(2);
    assert_eq!(h.state().scientific.console_input, "mean(c(10, 20, 30))");
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(2);
    assert_eq!(h.state().scientific.console_input, "draft");
    h.run_steps(12);
    assert!(h.ctx.read_response(id).unwrap().interact_rect.is_positive(), "history must reveal the live prompt");
    let multiline = (0..40).map(|i| format!("# paste {i}\n")).collect::<String>();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text(multiline.clone()));
    h.run_steps(12);
    assert_eq!(h.state().scientific.console_input, multiline);
    let measurements = geometry(&h, 320.0);
    capture(&mut h, BottomTab::Console, 320.0, 1.0, "multiline-input-scroll", &measurements);
    let input = h.ctx.read_response(id).unwrap().interact_rect;
    assert!(input.is_positive() && input.height() < HEIGHT * 0.35, "multiline input must be visible and bounded: {input:?}");
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(id), "multiline paste must retain input focus");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().scientific.console_input.is_empty());
    h.get_by_label("Environment").click_accesskit();
    h.run_steps(3);
    h.get_by_label("Console").click_accesskit();
    h.run_steps(3);
    assert_eq!(h.ctx.memory(|m| m.focused()), h.state().scientific.console_input_id);
}

#[test]
fn r_workspace_control_configuration_validates_before_mutating() {
    let mut app = fixture(5);
    let width = app.ui.r_studio_workspace_width;
    assert!(configure_workspace(&mut app, &json!({"rWorkspaceWidth": 320, "rTab": "invalid"})).is_err());
    assert_eq!(app.ui.r_studio_workspace_width, width);
    configure_workspace(&mut app, &json!({"rWorkspaceWidth": 320, "rTab": "environment", "rEditorFraction": 0.4})).unwrap();
    assert_eq!(app.ui.r_studio_workspace_width, 320.0);
    assert_eq!(app.ui.r_studio_tab, "environment");
}

#[test]
fn r_workspace_source_navigation_on_long_line_clears_reveal_and_console_history_moves_caret_to_end() {
    let mut h = source_harness(560.0, 1.0);
    let long_line = format!("{}\nplot_B <- 2\n", "x".repeat(220));
    h.state_mut().scientific.scripts[0].text = long_line;
    h.run_steps(3);
    assert!(navigate_source(&mut h.state_mut().scientific, 1, 180));
    h.run_steps(4);
    assert!(
        h.state().scientific.scripts[0].pending_reveal_index.is_none(),
        "revealing a caret past horizontal viewport width must still clear pending_reveal_index once vertically visible"
    );

    let console_id = h.state().scientific.console_input_id.unwrap();
    h.ctx.memory_mut(|m| m.request_focus(console_id));
    h.run_steps(2);
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(3);
    let expected_len = h.state().scientific.console_input.chars().count();
    assert!(expected_len > 0);
    assert_eq!(
        egui::TextEdit::load_state(&h.ctx, console_id).and_then(|s| s.cursor.char_range()).map(|r| usize::from(r.primary.index)),
        Some(expected_len),
        "recalling console history must place the caret at the end of the recalled expression"
    );

    // Pressing Enter while R is busy must not insert a stray newline into the draft input.
    h.state_mut().scientific.running = true;
    let draft_before = h.state().scientific.console_input.clone();
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().scientific.console_input, draft_before);
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(console_id));
}

#[test]
fn r_environment_table_headers_categories_and_vertical_alignment() {
    let mut h = harness(BottomTab::Environment, 560.0, 1.0, 5, false);
    h.state_mut().scientific.objects.push(crate::r_session::RObject {
        variable: "summarize_fn".into(),
        classes: vec!["function".into()],
        data_type: "closure".into(),
        dimensions: String::new(),
        length: 1,
        preview: "function (x, na.rm = TRUE)".into(),
        category: "Functions".into(),
        description: "function(x, na.rm)".into(),
        plot_kind: String::new(),
    });
    h.run_steps(3);

    // 1. Visible column headers in left-to-right order on the same header row.
    let name_hdr = h.get_by_label("Name").rect();
    let type_hdr = h.get_by_label("Type / Structure").rect();
    let actions_hdr = h.get_by_label("Actions").rect();
    let splitter = h.get_by_label("Resize Name and Type columns").rect();
    assert!((name_hdr.center().y - type_hdr.center().y).abs() < 1.0);
    assert!((type_hdr.center().y - actions_hdr.center().y).abs() < 1.0);
    assert!(name_hdr.right() <= splitter.left() + 1.0);
    assert!(splitter.right() <= type_hdr.left() + 1.0);
    assert!(type_hdr.right() <= actions_hdr.left() + 1.0);

    // 2. Category sections with counts within the same table.
    assert!(h.query_by_label("Data (1)").is_some());
    assert!(h.query_by_label("Values (4)").is_some());
    assert!(h.query_by_label("Functions (1)").is_some());

    // 3. Name and Type / Structure cells align vertically across Data, Values, and Functions.
    let df_name = h.get_by_label("df").rect();
    let val_name = h.get_by_label("한글_변수").rect();
    let plot_name = h.get_by_label("plot_A").rect();
    let fn_name = h.get_by_label("summarize_fn").rect();
    for rect in [val_name, plot_name, fn_name] {
        assert!((rect.left() - df_name.left()).abs() < 0.5, "Name column left must align across categories");
        assert!((rect.right() - df_name.right()).abs() < 0.5, "Name column right must align across categories");
    }

    let df_type = h.get_by_label("4 obs. of 5 variables").rect();
    let val_type = h.get_by_label("한글 값 / UTF-8 / café / 日本語").rect();
    let plot_type = h.get_by_label("ggplot2::ggplot").rect();
    let fn_type = h.get_by_label("function(x, na.rm)").rect();
    for rect in [val_type, plot_type, fn_type] {
        assert!((rect.left() - df_type.left()).abs() < 0.5, "Type / Structure column left must align across categories");
        assert!((rect.right() - df_type.right()).abs() < 0.5, "Type / Structure column right must align across categories");
    }

    // 4. Collapsible category sections toggle their rows without breaking table layout.
    h.get_by_label("Data (1)").click();
    h.run_steps(2);
    assert!(h.query_by_label("df").is_none(), "collapsing Data (1) hides its rows");
    assert!(h.query_by_label("plot_A").is_some(), "other categories remain open");
    h.get_by_label("Data (1)").click();
    h.run_steps(2);
    assert!(h.query_by_label("df").is_some(), "expanding Data (1) restores its rows");
    geometry(&h, 560.0);
}

#[test]
fn r_environment_resizable_columns_preserve_width_and_clamp_safely() {
    let mut h = harness(BottomTab::Environment, 560.0, 1.0, 5, true);
    h.get_by_label("Environment").click_accesskit();
    h.run_steps(3);

    let before_df_name = h.get_by_label("df").rect();
    let before_df_type = h.get_by_label("4 obs. of 5 variables").rect();
    let before_actions = h.get_by_label("Actions").rect();

    // Drag the Name / Type separator to the right by 60px.
    drag_splitter(&mut h, "Resize Name and Type columns", vec2(60.0, 0.0));
    let after_df_name = h.get_by_label("df").rect();
    let after_df_type = h.get_by_label("4 obs. of 5 variables").rect();
    let after_val_name = h.get_by_label("plot_A").rect();
    let after_actions = h.get_by_label("Actions").rect();

    assert!((after_df_name.width() - (before_df_name.width() + 60.0)).abs() < 3.0);
    assert!((after_df_type.left() - (before_df_type.left() + 60.0)).abs() < 3.0);
    assert!((after_val_name.right() - after_df_name.right()).abs() < 0.5, "all categories resize together");
    assert!((after_actions.left() - before_actions.left()).abs() < 0.5, "Actions column stays fixed-width and right-aligned");

    // Keyboard arrow resize while the column splitter is focused.
    h.get_by_label("Resize Name and Type columns").click();
    h.run_steps(2);
    h.key_press(egui::Key::ArrowLeft);
    h.run_steps(2);
    let after_key_name = h.get_by_label("df").rect();
    assert!((after_key_name.width() - (after_df_name.width() - 12.0)).abs() < 1.5);

    // Extreme drag clamps to sensible minimum widths without overlapping or overflowing.
    drag_splitter(&mut h, "Resize Name and Type columns", vec2(-500.0, 0.0));
    let min_name = h.get_by_label("df").rect();
    assert!(min_name.width() >= 65.0, "Name column enforces a sensible minimum width: {min_name:?}");
    geometry(&h, 560.0);

    drag_splitter(&mut h, "Resize Name and Type columns", vec2(90.0, 0.0));
    let chosen_width = h.get_by_label("df").rect().width();

    // Preserved across tab switches while the workspace is open.
    h.get_by_label("Console").click_accesskit();
    h.run_steps(2);
    h.get_by_label("Environment").click_accesskit();
    h.run_steps(2);
    assert!((h.get_by_label("df").rect().width() - chosen_width).abs() < 1.0);
}

#[test]
fn r_environment_icon_actions_by_type_routing_and_mandatory_remove_confirmation() {
    let mut h = harness(BottomTab::Environment, 420.0, 1.0, 5, false);

    // No text action buttons on variable rows.
    for old_text in ["View data", "View Data"] {
        assert!(
            !h.output().shapes.iter().any(|shape| matches!(&shape.shape, Shape::Text(text) if text.galley.text() == old_text)),
            "row action {old_text:?} must be an icon button, not a text button"
        );
    }

    //Fixture(5) has:
    // - 1 Data frame (`df`) -> View Data + Remove
    // - 2 Plots (`plot_A`, `한글_plot_B_...`) -> Plot Registry + Remove
    // - 2 Values (`한글_변수`, `numeric_04`) -> Remove only
    let view_buttons: Vec<_> = h.query_all_by_label("View Data").collect();
    let plot_buttons: Vec<_> = h.query_all_by_label("Plot Registry").collect();
    let remove_buttons: Vec<_> = h.query_all_by_label("Remove").collect();
    assert_eq!(view_buttons.len(), 1, "only viewable data frames show View Data icon");
    assert_eq!(plot_buttons.len(), 2, "only recognized plot objects show Plot Registry icon");
    assert_eq!(remove_buttons.len(), 5, "every removable object shows Remove icon");

    // Verify right-alignment and single-row alignment with the row's variable name.
    let df_rect = h.get_by_label("df").rect();
    let df_view_rect = view_buttons[0].rect();
    let df_remove_rect = remove_buttons[0].rect();
    assert!((df_view_rect.center().y - df_rect.center().y).abs() < 1.5, "View Data icon stays on the same row as df");
    assert!((df_remove_rect.center().y - df_rect.center().y).abs() < 1.5, "Remove icon stays on the same row as df");
    assert!(df_view_rect.right() < df_remove_rect.left(), "View Data sits immediately left of Remove");
    assert!(df_remove_rect.right() >= 420.0 - 24.0, "Remove icon is right-aligned near the panel edge: {df_remove_rect:?}");

    // Clicking View Data routes to request_data_page and selects `df`.
    view_buttons[0].click();
    h.run_steps(2);
    assert_eq!(h.state().scientific.selected_object.as_deref(), Some("df"));
    assert_eq!(h.state().scientific.viewer_start, 0);
    h.state_mut().scientific.running = false;

    // Clicking Plot Registry icon selects the plot and opens the Plot Registry tab.
    h.query_all_by_label("Plot Registry").next().unwrap().click();
    h.run_steps(2);
    assert_eq!(h.state().scientific.selected_plot.as_deref(), Some("plot_A"));
    assert!(crate::panel_docking::showing(&h.state().ui, "graph"));

    // Switch back to Environment and verify Remove requires confirmation and never deletes immediately.
    h.state_mut().ui.r_studio_tab = BottomTab::Environment.key().into();
    h.run_steps(2);
    assert_eq!(h.state().scientific.objects.len(), 5);
    h.query_all_by_label("Remove").next().unwrap().click();
    h.run_steps(2);
    assert_eq!(h.state().scientific.confirm_object_remove.as_deref(), Some("df"));
    assert_eq!(h.state().scientific.objects.len(), 5, "clicking Remove icon must not delete immediately");
    assert!(h.query_by_label("Confirm Remove").is_some());
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.state().scientific.confirm_object_remove.is_none());
    assert_eq!(h.state().scientific.objects.len(), 5);
}

#[test]
fn r_environment_narrow_layout_truncation_hover_metadata_and_filtering() {
    let mut h = harness(BottomTab::Environment, 320.0, 1.0, 5, false);
    geometry(&h, 320.0);

    let long_name = "한글_plot_B_with_a_long_variable_name_that_must_remain_readable";
    let long_node = h.get_by_label(long_name);
    let long_rect = long_node.rect();

    // Ensure action icons on the long-variable row do not wrap to a second line at 320px width.
    let plot_icons: Vec<_> = h.query_all_by_label("Plot Registry").collect();
    let second_plot_icon = plot_icons[1].rect();
    assert!((second_plot_icon.center().y - long_rect.center().y).abs() < 1.5, "action buttons must not wrap in narrow panes");

    // Ensure the visual text in the Name column is truncated with an ellipsis.
    assert!(
        h.output().shapes.iter().any(|shape| matches!(&shape.shape, Shape::Text(text)
            if text.galley.text().starts_with("한글_plot_B") && text.galley.text().ends_with('…'))),
        "long variable name must be visually truncated with an ellipsis in narrow panes"
    );

    // Hovering over the truncated Name cell reveals the full variable name and metadata in a tooltip.
    h.ctx.all_styles_mut(|style| style.interaction.tooltip_delay = 0.0);
    h.hover_at(long_rect.center());
    h.run_steps(3);
    fn contains_text(shape: &Shape, needle: &str) -> bool {
        match shape {
            Shape::Text(text) => text.galley.text().contains(needle),
            Shape::Vec(shapes) => shapes.iter().any(|s| contains_text(s, needle)),
            _ => false,
        }
    }
    assert!(
        h.output().shapes.iter().any(|s| contains_text(&s.shape, long_name))
            && h.output().shapes.iter().any(|s| contains_text(&s.shape, "Dimensions:")),
        "hovering a truncated variable row must show its full name and metadata tooltip"
    );

    // Filtering updates category counts and preserves row selection.
    let filter_input = h.get_by_role(Role::TextInput);
    filter_input.focus();
    filter_input.type_text("plot_A");
    h.run_steps(3);
    assert!(h.query_by_label("Data (0)").is_some());
    assert!(h.query_by_label("Values (1)").is_some());
    assert!(h.query_by_label("df").is_none());
    h.get_by_label("plot_A").click();
    h.run_steps(2);
    assert_eq!(h.state().scientific.selected_object.as_deref(), Some("plot_A"));
    assert_eq!(h.state().scientific.selected_plot.as_deref(), Some("plot_A"));
}

fn focus_script_editor(h: &mut Harness<'static, crate::VectorcraftApp>, script_id: u64, cursor_char: usize) -> egui::Id {
    let editor_id = h.state().scientific.editor_widget_ids[&script_id];
    h.ctx.memory_mut(|m| m.request_focus(editor_id));
    let mut state = egui::TextEdit::load_state(&h.ctx, editor_id).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_char))));
    state.store(&h.ctx, editor_id);
    h.run_steps(2);
    editor_id
}

#[test]
fn r_source_editor_tab_completion_trigger_navigation_and_escape() {
    let mut h = source_harness(560.0, 1.0);
    h.state_mut().scientific.scripts[0].text = "se".into();
    h.run_steps(2);
    let editor_id = focus_script_editor(&mut h, 1, 2);

    // 1. Typing 'se' + Tab opens completion popup without altering source text or inserting Tab.
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    let active = h.state().scientific.completion.clone().expect("Tab must open completion popup");
    assert_eq!(active.script_id, 1);
    assert_eq!(active.context.prefix, "se");
    assert_eq!(h.state().scientific.scripts[0].text, "se", "Source text must remain unmodified on trigger");
    assert!(active.candidates.len() >= 2, "Expected multiple 'se' candidates for navigation");
    assert_eq!(active.selected, 0);

    // 2. ArrowDown and ArrowUp navigate candidate selection.
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(2);
    assert_eq!(h.state().scientific.completion.as_ref().unwrap().selected, 1);
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(2);
    assert_eq!(h.state().scientific.completion.as_ref().unwrap().selected, 0);

    // Escape closes completion popup without altering source text.
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(h.state().scientific.completion.is_none(), "Escape must dismiss completion popup");
    assert_eq!(h.state().scientific.scripts[0].text, "se");
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(editor_id));
}

#[test]
fn r_source_editor_tab_completion_function_acceptance_and_no_duplicate_parens() {
    let mut h = source_harness(560.0, 1.0);
    h.state_mut().scientific.scripts[0].text = "summ".into();
    h.run_steps(2);
    let editor_id = focus_script_editor(&mut h, 1, 4);

    // Trigger completion and accept with second Tab.
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert!(h.state().scientific.completion.is_some());

    // 3. Tab accepts selected candidate ('summary').
    // 4. Function completion inserts '()' with caret between parentheses.
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert!(h.state().scientific.completion.is_none(), "Acceptance must close completion popup");
    assert_eq!(h.state().scientific.scripts[0].text, "summary()");
    let cursor = egui::TextEdit::load_state(&h.ctx, editor_id).and_then(|s| s.cursor.char_range()).map(|r| usize::from(r.primary.index));
    assert_eq!(cursor, Some(8), "Caret must be placed inside parentheses");

    // 16. Avoid inserting duplicate parentheses when '(' already follows.
    h.state_mut().scientific.scripts[0].text = "summ(10)".into();
    h.run_steps(2);
    focus_script_editor(&mut h, 1, 4);
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert_eq!(h.state().scientific.scripts[0].text, "summary(10)", "Must not create duplicate parentheses or insert tab");
}

#[test]
fn r_source_editor_tab_completion_variable_and_column_members() {
    let mut h = source_harness(560.0, 1.0);
    // User-defined script variable and .GlobalEnv object
    h.state_mut().scientific.scripts[0].text = "한글_변수_로컬 <- 123\n한글_".into();
    h.run_steps(2);
    let text_len = h.state().scientific.scripts[0].text.chars().count();
    let _editor_id = focus_script_editor(&mut h, 1, text_len);

    // Trigger completion on local variable
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert!(h.state().scientific.completion.is_some());
    // 5. Variable completion inserts identifier without parentheses.
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert_eq!(h.state().scientific.scripts[0].text, "한글_변수_로컬 <- 123\n한글_변수_로컬");

    // 6. Data-frame column completion after 'df$' preserves syntax and inserts column name without parentheses.
    h.state_mut().scientific.scripts[0].text = "df$mp".into();
    h.run_steps(2);
    focus_script_editor(&mut h, 1, 5);
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    let active = h.state().scientific.completion.clone().expect("df$ completion must open");
    assert!(active.candidates.iter().any(|c| c.name == "mpg"));
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert_eq!(h.state().scientific.scripts[0].text, "df$mpg");

    // Member completion after bracket 'df[["'
    h.state_mut().scientific.scripts[0].text = "df[[\"mp".into();
    h.run_steps(2);
    focus_script_editor(&mut h, 1, 7);
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert_eq!(h.state().scientific.scripts[0].text, "df[[\"mpg\"]]");
}

#[test]
fn r_source_editor_tab_completion_namespace_qualification() {
    let mut h = source_harness(560.0, 1.0);
    // 7. Namespace completion after 'stats::' and 'stats:::'
    h.state_mut().scientific.scripts[0].text = "stats::med".into();
    h.run_steps(2);
    focus_script_editor(&mut h, 1, 10);
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    let active = h.state().scientific.completion.clone().expect("stats:: completion must open");
    assert!(active.candidates.iter().any(|c| c.name == "median"));
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert_eq!(h.state().scientific.scripts[0].text, "stats::median()");

    // Internal namespace 'stats:::'
    h.state_mut().scientific.scripts[0].text = "stats:::run".into();
    h.run_steps(2);
    focus_script_editor(&mut h, 1, 11);
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert!(h.state().scientific.completion.is_some());
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert!(h.state().scientific.scripts[0].text.starts_with("stats:::"));
}

#[test]
fn r_source_editor_tab_completion_indentation_fallbacks() {
    let mut h = source_harness(560.0, 1.0);
    let editor_id = h.state().scientific.editor_widget_ids[&1];

    // 8. No matches fallback to 2-space indentation.
    h.state_mut().scientific.scripts[0].text = "zzznomatch".into();
    h.run_steps(2);
    focus_script_editor(&mut h, 1, 10);
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert!(h.state().scientific.completion.is_none(), "No popup when candidates are empty");
    assert_eq!(h.state().scientific.scripts[0].text, "zzznomatch  ");

    // 9. Empty line Tab fallback to 2-space indentation.
    h.state_mut().scientific.scripts[0].text = "".into();
    h.run_steps(2);
    focus_script_editor(&mut h, 1, 0);
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert!(h.state().scientific.completion.is_none());
    assert_eq!(h.state().scientific.scripts[0].text, "  ");

    // 10. Multiline selection Tab indents selection by 2 spaces.
    h.state_mut().scientific.scripts[0].text = "line_one <- 1\nline_two <- 2".into();
    h.run_steps(2);
    h.ctx.memory_mut(|m| m.request_focus(editor_id));
    let mut state = egui::TextEdit::load_state(&h.ctx, editor_id).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(h.state().scientific.scripts[0].text.chars().count()),
    )));
    state.store(&h.ctx, editor_id);
    h.run_steps(2);
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert_eq!(h.state().scientific.scripts[0].text, "  line_one <- 1\n  line_two <- 2");

    // 11. Shift+Tab outdents multiline selection.
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Tab);
    h.run_steps(2);
    assert_eq!(h.state().scientific.scripts[0].text, "line_one <- 1\nline_two <- 2");
}

#[test]
fn r_source_editor_tab_completion_execution_switching_dismissal_and_undo() {
    let mut h = source_harness(560.0, 1.0);
    let editor_id = h.state().scientific.editor_widget_ids[&1];

    // 12. Ctrl+Enter executes expression and closes completion popup.
    h.state_mut().scientific.scripts[0].text = "plot_A <- 1\nsumm".into();
    h.run_steps(2);
    focus_script_editor(&mut h, 1, 16);
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert!(h.state().scientific.completion.is_some());
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::COMMAND, egui::Key::Enter);
    h.run_steps(3);
    assert!(h.state().scientific.completion.is_none(), "Execution must close completion popup");

    // 13. Script switching dismisses open completion.
    h.state_mut().scientific.scripts[0].text = "summ".into();
    h.run_steps(2);
    focus_script_editor(&mut h, 1, 4);
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert!(h.state().scientific.completion.is_some());
    // Switch to script 2
    h.get_by_label("second.R").click();
    h.run_steps(2);
    assert!(h.state().scientific.completion.is_none(), "Script tab switch must dismiss completion popup");

    // 14. Async completion metadata never inserts into wrong script.
    h.state_mut().scientific.scripts[1].text = "value <- 42\n".into();
    let msg = RMessage::CompletionMetadata {
        script_id: 1,
        source_revision: "summ".into(),
        object_columns: Some(("df".into(), vec![("new_col".into(), "numeric".into())])),
        namespace_items: None,
    };
    let _ = h.state_mut().scientific._tx.send(msg);
    poll_messages(h.state_mut());
    h.run_steps(2);
    assert_eq!(h.state().scientific.scripts[1].text, "value <- 42\n", "Second script must remain untouched");
    assert!(h.state().scientific.completion.is_none(), "Completion must not open on wrong active script");

    // 15. Ctrl+Z undos completion acceptance.
    h.state_mut().scientific.activate_script(1);
    h.run_steps(2);
    h.state_mut().scientific.scripts[0].text = "summ".into();
    h.run_steps(2);
    focus_script_editor(&mut h, 1, 4);
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    assert_eq!(h.state().scientific.scripts[0].text, "summary()");
    // Perform undo
    let mut state = egui::TextEdit::load_state(&h.ctx, editor_id).unwrap_or_default();
    let current_cursor = state.cursor.char_range().unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(0)));
    let mut undoer = state.undoer();
    if let Some((_cursor, text)) = undoer.undo(&(current_cursor, h.state().scientific.scripts[0].text.clone())) {
        h.state_mut().scientific.scripts[0].text = text.clone();
    }
    state.set_undoer(undoer);
    state.store(&h.ctx, editor_id);
    h.run_steps(2);
    assert_eq!(h.state().scientific.scripts[0].text, "summ", "Undo must restore pre-completion state");
}

#[test]
fn r_environment_view_data_opens_data_viewer_tab_in_central_area() {
    let mut app = fixture(5);
    app.ui.r_studio_tab = BottomTab::Environment.key().into();

    let mut h = Harness::builder()
        .with_size(vec2(560.0, HEIGHT))
        .with_pixels_per_point(1.0)
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_context_setup(|ctx| {
            crate::theme::install_fonts(ctx);
            crate::theme::apply(ctx, crate::theme::Brightness::MediumDark);
        })
        .build_ui_state(
            |ui, app: &mut crate::VectorcraftApp| {
                handle_shortcuts(app, ui.ctx());
                ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);
                show_script_tabs(app, ui);
                if let Some(active_viewer) = app.scientific.active_viewer_tab.clone() {
                    show_data_viewer_tab(&mut app.scientific, ui, 340.0, &active_viewer);
                } else {
                    show_source_editor(&mut app.scientific, ui, 340.0);
                }
                show_bottom_tabs(app, ui);
                app.ui_fonts.frame(ui.ctx());
                app.ui_fonts.finish(ui.ctx());
            },
            app,
        );
    h.run_steps(5);

    // Initial state: 1 script open, no viewer tabs open
    assert_eq!(h.state().scientific.scripts.len(), 1);
    assert!(h.state().scientific.viewer_tabs.is_empty());
    assert!(h.state().scientific.active_viewer_tab.is_none());

    // Click "View Data" next to `df`
    let view_btn = h.query_all_by_label("View Data").next().expect("View Data icon for df");
    view_btn.click();
    h.run_steps(2);

    // Verify Data Viewer tab opened in central area
    assert_eq!(h.state().scientific.active_viewer_tab.as_deref(), Some("df"));
    assert_eq!(h.state().scientific.viewer_tabs.len(), 1);
    assert_eq!(h.state().scientific.viewer_tabs[0].name, "df");

    // Populate data page for df
    h.state_mut().scientific.viewer_tabs[0].loading = false;
    h.state_mut().scientific.viewer_tabs[0].page = Some(crate::r_session::DataPage {
        total_rows: 32,
        columns: vec![("mpg".into(), "numeric".into()), ("cyl".into(), "integer".into())],
        rows: vec![vec!["21.0".into(), "6".into()], vec!["21.0".into(), "6".into()], vec!["22.8".into(), "4".into()]],
    });
    h.run_steps(2);

    // Verify Data Viewer table rendered with columns and cells
    assert!(h.query_by_label_contains("mpg").is_some());
    assert!(h.query_by_label_contains("cyl").is_some());
    assert!(
        h.output().shapes.iter().any(|shape| matches!(&shape.shape, Shape::Text(text) if text.galley.text() == "22.8")),
        "cell value 22.8 rendered in table"
    );

    // Verify tab strip displays both the script tab and the data viewer tab
    assert!(h.get_all_by_role_and_label(Role::Button, "df").next().is_some());

    // Clicking script tab switches back to source editor
    let script_id = h.state().scientific.scripts[0].id;
    let script_tab_label = h.state().scientific.scripts[0].filename().to_owned();
    let script_btn = h.get_by_label(&script_tab_label);
    script_btn.click();
    h.run_steps(2);
    assert!(h.state().scientific.active_viewer_tab.is_none());
    assert_eq!(h.state().scientific.active_script_id, script_id);
    // Data Viewer tab is still cached
    assert_eq!(h.state().scientific.viewer_tabs.len(), 1);

    // Clicking Data Viewer tab switches back to Data Viewer
    let viewer_tab_btn = h.get_all_by_role_and_label(Role::Button, "df").next().unwrap();
    viewer_tab_btn.click();
    h.run_steps(2);
    assert_eq!(h.state().scientific.active_viewer_tab.as_deref(), Some("df"));

    // Closing the Data Viewer tab
    h.state_mut().scientific.close_data_viewer("df");
    h.run_steps(2);
    assert!(h.state().scientific.viewer_tabs.is_empty());
    assert!(h.state().scientific.active_viewer_tab.is_none());
    assert_eq!(h.state().scientific.active_script_id, script_id);
    // R object in environment was not deleted!
    assert_eq!(h.state().scientific.objects.len(), 5);
}
