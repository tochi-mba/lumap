//! Step 3, Content: the library. Tiles for the built-in looks (with small moving previews), your
//! media, live inputs and test patterns. Click a tile to put it on the selected surface, or on
//! every surface of the projector when none is selected; or drag it onto a surface on the
//! canvas.

use super::super::super::canvas::CanvasState;
use super::super::super::handles::{contains, handles, Mapping};
use super::super::super::thumbs::downscale;
use super::super::super::{Editor, Selection};
use super::super::{Actions, PanelState};
use eframe::egui::{self, RichText, Sense, Stroke};
use lumap_core::looks::LOOKS;
use lumap_core::project::{GeneratorCategory, SourceKind, TestPattern};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Side of a tile's preview, in points.
pub const TILE: f32 = 96.0;
/// Pixels a look's preview is rendered at.
const PREVIEW_PX: u32 = 72;
/// How often a visible look preview moves on.
const PREVIEW_EVERY: f64 = 0.12;
/// At most this many previews are redrawn per frame; the rest keep their last picture.
const PREVIEWS_PER_FRAME: usize = 3;

/// What a tile puts on a surface.
#[derive(Debug, Clone, PartialEq)]
pub enum Tile {
    /// A built-in look (index into `LOOKS`).
    Look(usize),
    /// Content already in the show.
    Source(String),
    /// A test pattern.
    Pattern(TestPattern),
    /// Plain white.
    White,
    /// A room effect, drawn from the projector's scan.
    Room(lumap_core::project::RoomEffect),
    /// Paint drawn on the wall with a light or a hand.
    Paint(lumap_core::project::PaintInput),
    /// The live camera.
    Camera,
}

/// Which tiles the library shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    /// Everything, in sections.
    #[default]
    All,
    /// Looks of one category.
    Looks(GeneratorCategory),
    /// Media and live inputs already in the show.
    Media,
    /// Test patterns and white.
    Patterns,
}

/// The library's own state: the filter and the moving previews.
#[derive(Default)]
pub struct LibraryState {
    /// Which tiles are shown.
    pub filter: Filter,
    /// Look previews: when each was last drawn, and its texture.
    previews: HashMap<usize, (f64, egui::TextureHandle)>,
    /// Pattern previews, drawn once.
    patterns: HashMap<&'static str, egui::TextureHandle>,
    /// Look previews still allowed to move on this frame.
    budget: usize,
}

impl std::fmt::Debug for LibraryState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibraryState")
            .field("filter", &self.filter)
            .finish_non_exhaustive()
    }
}

/// The test patterns offered, with their names.
pub const PATTERNS: [(&str, TestPattern); 4] = [
    ("Grid", TestPattern::Grid),
    ("Colour bars", TestPattern::Bars),
    ("Checker", TestPattern::Checker),
    ("Grey ramp", TestPattern::Ramp),
];

/// Puts a tile on a surface as one undo step.
pub fn apply_tile(editor: &mut Editor, tile: &Tile, surface: &str) {
    let source = match tile {
        Tile::Look(i) => {
            if let Some(look) = LOOKS.get(*i) {
                super::super::looks::apply_look(editor, look, surface);
            }
            return;
        }
        Tile::Source(id) => id.clone(),
        Tile::Pattern(p) => find_or_add(editor, &SourceKind::TestPattern { pattern: *p }),
        Tile::White => find_or_add(
            editor,
            &SourceKind::Solid {
                color: [1.0, 1.0, 1.0, 1.0],
            },
        ),
        Tile::Room(effect) => {
            let Some(projector) = editor.project.surface(surface).map(|s| s.output.clone()) else {
                return;
            };
            find_or_add(
                editor,
                &SourceKind::Room {
                    projector,
                    effect: *effect,
                    color: [1.0, 1.0, 1.0, 1.0],
                    speed: 1.0,
                },
            )
        }
        Tile::Paint(input) => {
            let Some(projector) = editor.project.surface(surface).map(|s| s.output.clone()) else {
                return;
            };
            find_or_add(
                editor,
                &SourceKind::Paint {
                    projector,
                    input: *input,
                    brush: lumap_core::project::Brush::Neon,
                    color: [0.84, 1.0, 0.25, 1.0],
                    size: 12.0,
                    fade_seconds: 0.0,
                },
            )
        }
        Tile::Camera => find_or_add(
            editor,
            &SourceKind::Camera {
                device: String::new(),
            },
        ),
    };
    let sid = surface.to_string();
    let chosen = source.clone();
    if editor.edit(move |p| {
        if let Some(s) = p.surface_mut(&sid) {
            s.source = chosen;
        }
    }) {
        editor.status = format!("{source} on {surface}");
    }
}

/// The source of this kind already in the show, or a new one.
fn find_or_add(editor: &mut Editor, kind: &SourceKind) -> String {
    match editor.project.sources.iter().find(|s| &s.kind == kind) {
        Some(s) => s.id.clone(),
        None => editor.add_source(kind.clone()),
    }
}

/// Where a click on a tile goes: the selected surface, or every surface of the projector.
fn targets(editor: &Editor, canvas: &CanvasState) -> Vec<String> {
    if let Selection::Surface { id, .. } = &editor.selection {
        let mut all = vec![id.clone()];
        all.extend(editor.extra_selected.iter().cloned());
        return all;
    }
    super::map::current_projector(editor, canvas).map_or_else(Vec::new, |out| {
        editor
            .project
            .surfaces_for_output(&out)
            .iter()
            .map(|s| s.id.clone())
            .collect()
    })
}

/// The library, as the left panel on Content.
#[allow(clippy::too_many_lines)]
pub fn library(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    state: &mut PanelState,
    canvas: &CanvasState,
    actions: &mut Actions,
) {
    egui::Panel::left("lumap-library")
        .default_size(372.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Library");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button("+ generator", |ui| {
                        super::super::looks::generator_menu(ui, editor);
                    });
                    if ui.small_button("+ media").on_hover_text("Video, images, GIFs").clicked() {
                        actions.add_media = true;
                    }
                });
            });
            let target_text = match &editor.selection {
                Selection::Surface { id, .. } => format!("Click a tile to put it on {id}, or drag it onto any surface."),
                _ => "Click a tile to put it on every surface of this projector, or drag it onto one.".into(),
            };
            ui.label(RichText::new(target_text).weak().small());
            ui.add_space(4.0);
            filters(ui, &mut state.library.filter);
            ui.add_space(6.0);
            let now = ui.input(|i| i.time);
            state.library.budget = PREVIEWS_PER_FRAME;
            let mut clicked: Option<Tile> = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                let filter = state.library.filter;
                let show_looks = |c: GeneratorCategory| matches!(filter, Filter::All) || filter == Filter::Looks(c);
                let looks: Vec<usize> = (0..LOOKS.len()).filter(|&i| show_looks(LOOKS[i].category())).collect();
                if !looks.is_empty() {
                    crate::theme::section_header(ui, "Looks");
                    grid(ui, |ui| {
                        for i in looks {
                            let lib = &mut state.library;
                            let r = tile_response(ui, &Tile::Look(i), LOOKS[i].name, LOOKS[i].blurb, |ctx| {
                                Some(look_preview(ctx, lib, i, now).id())
                            });
                            if r.clicked() {
                                clicked = Some(Tile::Look(i));
                            }
                        }
                    });
                }
                if matches!(filter, Filter::All | Filter::Media) {
                    crate::theme::section_header(ui, "In this show");
                    let media: Vec<(String, bool)> = editor
                        .project
                        .sources
                        .iter()
                        .map(|s| (s.id.clone(), matches!(s.kind, SourceKind::Spout { .. })))
                        .collect();
                    if media.is_empty() {
                        ui.label(
                            RichText::new("Nothing yet. Add media, or drop files anywhere on the window.")
                                .weak()
                                .small(),
                        );
                    }
                    grid(ui, |ui| {
                        for (id, live) in &media {
                            let frame = editor.sources.frame(id).cloned();
                            let thumbs = &mut state.thumbs;
                            let hint = if *live {
                                "Live input. Right-click for its settings."
                            } else {
                                "Right-click for its settings, or to remove it."
                            };
                            let r = tile_response(ui, &Tile::Source(id.clone()), id, hint, |ctx| {
                                frame.map(|f| thumbs.get(ctx, id, &f).0)
                            });
                            if r.clicked() {
                                clicked = Some(Tile::Source(id.clone()));
                            }
                            r.context_menu(|ui| source_menu(ui, editor, id));
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Add media…").clicked() {
                            actions.add_media = true;
                        }
                        if ui.button("Add frame folder…").clicked() {
                            actions.add_folder = true;
                        }
                    });
                    crate::theme::section_header(ui, "Live input");
                    ui.horizontal(|ui| {
                        let label = ui.label("Spout sender");
                        ui.add(egui::TextEdit::singleline(&mut state.spout_sender).desired_width(150.0))
                            .labelled_by(label.id)
                            .on_hover_text("The name another app (Resolume, TouchDesigner…) shares its video as");
                        if ui.button("Add Spout receiver").clicked() {
                            editor.add_source(SourceKind::Spout {
                                sender: state.spout_sender.trim().to_string(),
                            });
                        }
                    });
                }
                if matches!(filter, Filter::All) {
                    let scans = editor.scans.ready().len();
                    crate::theme::section_header(
                        ui,
                        if scans == 0 {
                            "Room-aware effects · scan needed"
                        } else {
                            "Room-aware effects"
                        },
                    );
                    if scans > 0 {
                        ui.label(
                            RichText::new(format!(
                                "{scans} projector scan{} ready · effects follow the scanned wall",
                                if scans == 1 { "" } else { "s" }
                            ))
                            .weak()
                            .small(),
                        );
                    } else {
                        ui.label(
                            RichText::new(
                                "Scan in Setup once, then these effects follow the real wall.",
                            )
                            .weak()
                            .small(),
                        );
                    }
                    grid(ui, |ui| {
                        for e in lumap_core::project::RoomEffect::ALL {
                            let r = tile_response(ui, &Tile::Room(e), e.label(), e.blurb(), |_| None);
                            if r.clicked() {
                                clicked = Some(Tile::Room(e));
                            }
                        }
                        for (input, name) in [
                            (lumap_core::project::PaintInput::Light, "Paint with light"),
                            (lumap_core::project::PaintInput::Hand, "Paint with your hand"),
                        ] {
                            let r = tile_response(ui, &Tile::Paint(input), name, input.label(), |_| None);
                            if r.clicked() {
                                clicked = Some(Tile::Paint(input));
                            }
                        }
                        let r = tile_response(ui, &Tile::Camera, "Camera", "The camera, live", |_| None);
                        if r.clicked() {
                            clicked = Some(Tile::Camera);
                        }
                    });
                }
                if matches!(filter, Filter::All | Filter::Patterns) {
                    crate::theme::section_header(ui, "Patterns");
                    grid(ui, |ui| {
                        for (name, p) in PATTERNS {
                            let lib = &mut state.library;
                            let r = tile_response(ui, &Tile::Pattern(p), name, "A test pattern", |ctx| {
                                Some(pattern_preview(ctx, lib, name, p).id())
                            });
                            if r.clicked() {
                                clicked = Some(Tile::Pattern(p));
                            }
                        }
                        if tile_response(ui, &Tile::White, "White", "Plain white light", |_| None).clicked() {
                            clicked = Some(Tile::White);
                        }
                    });
                }
            });
            if let Some(t) = clicked {
                let surfaces = targets(editor, canvas);
                if surfaces.is_empty() {
                    editor.status = "Add a surface in step 2, Map, first.".into();
                }
                for s in &surfaces {
                    apply_tile(editor, &t, s);
                }
            }
            // Moving previews only move while the library is on screen.
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(PREVIEW_EVERY));
        });
}

fn filters(ui: &mut egui::Ui, filter: &mut Filter) {
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(filter, Filter::All, "All");
        for c in GeneratorCategory::ALL {
            ui.selectable_value(filter, Filter::Looks(c), c.label());
        }
        ui.selectable_value(filter, Filter::Media, "Your media");
        ui.selectable_value(filter, Filter::Patterns, "Patterns");
    });
}

fn grid(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        add(ui);
    });
    ui.add_space(6.0);
}

/// A look's preview texture, moved on at most every `PREVIEW_EVERY` seconds.
fn look_preview(
    ctx: &egui::Context,
    lib: &mut LibraryState,
    i: usize,
    now: f64,
) -> egui::TextureHandle {
    if let Some((at, tex)) = lib.previews.get(&i) {
        if now - at < PREVIEW_EVERY || lib.budget == 0 {
            return tex.clone();
        }
    }
    lib.budget = lib.budget.saturating_sub(1);
    let look = LOOKS[i];
    let frame = lumap_sources::generator::render(
        look.kind,
        now * f64::from(look.speed),
        look.color,
        PREVIEW_PX,
    );
    let image = downscale(&frame, PREVIEW_PX);
    if let Some((at, tex)) = lib.previews.get_mut(&i) {
        tex.set(image, egui::TextureOptions::LINEAR);
        *at = now;
        return tex.clone();
    }
    let tex = ctx.load_texture(
        format!("look-preview-{i}"),
        image,
        egui::TextureOptions::LINEAR,
    );
    lib.previews.insert(i, (now, tex.clone()));
    tex
}

fn pattern_preview(
    ctx: &egui::Context,
    lib: &mut LibraryState,
    name: &'static str,
    p: TestPattern,
) -> egui::TextureHandle {
    lib.patterns
        .entry(name)
        .or_insert_with(|| {
            let frame = lumap_sources::pattern::generate(p, 96, 54);
            ctx.load_texture(
                format!("pattern-preview-{name}"),
                downscale(&frame, 96),
                egui::TextureOptions::LINEAR,
            )
        })
        .clone()
}

/// Settings or removal for content in the show (the tile's right-click menu).
fn source_menu(ui: &mut egui::Ui, editor: &mut Editor, id: &str) {
    if ui.button("Settings").clicked() {
        editor.selection = Selection::Source(id.to_string());
        editor.extra_selected.clear();
        ui.close();
    }
    let used = editor.project.surfaces.iter().any(|s| s.source == id);
    if ui
        .add_enabled(!used, egui::Button::new("Remove"))
        .on_disabled_hover_text("A surface still shows it")
        .clicked()
    {
        let id = id.to_string();
        editor.edit(|p| p.sources.retain(|s| s.id != id));
        editor.reload_sources();
        ui.close();
    }
}

/// A tile: preview, name, hover text; click applies it, dragging carries it to the canvas.
/// The preview is asked for only when the tile is on screen.
fn tile_response(
    ui: &mut egui::Ui,
    t: &Tile,
    name: &str,
    hint: &str,
    preview: impl FnOnce(&egui::Context) -> Option<egui::TextureId>,
) -> egui::Response {
    let size = egui::vec2(TILE, TILE * 9.0 / 16.0 + 22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    if !ui.is_rect_visible(rect) {
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
        response.dnd_set_drag_payload(t.clone());
        return response;
    }
    let tex = preview(ui.ctx());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    let painter = ui.painter_at(rect);
    let hovered = response.hovered() || response.dragged();
    painter.rect_filled(rect, 8.0, crate::theme::RAISED);
    let img = egui::Rect::from_min_size(
        rect.min + egui::vec2(4.0, 4.0),
        egui::vec2(TILE - 8.0, TILE * 9.0 / 16.0 - 8.0),
    );
    match tex {
        Some(id) => {
            painter.image(
                id,
                img,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            painter.rect_filled(img, 4.0, crate::theme::TEXT);
        }
    }
    painter.text(
        egui::pos2(rect.min.x + 6.0, rect.max.y - 6.0),
        egui::Align2::LEFT_BOTTOM,
        name,
        egui::FontId::proportional(11.5),
        crate::theme::TEXT,
    );
    painter.rect_stroke(
        rect,
        8.0,
        Stroke::new(
            1.0,
            if hovered {
                crate::theme::SIGNAL
            } else {
                crate::theme::LINE
            },
        ),
        egui::StrokeKind::Inside,
    );
    response.dnd_set_drag_payload(t.clone());
    response.on_hover_text(hint)
}

/// While a tile is dragged over the canvas, lights up the surface under the pointer; on release,
/// puts the tile on it.
pub fn canvas_drop(ctx: &egui::Context, editor: &mut Editor, canvas: &CanvasState) {
    if canvas.stage || !egui::DragAndDrop::has_payload_of_type::<Tile>(ctx) {
        return;
    }
    let (Some(rect), Some(out)) = (
        canvas.last_rect,
        super::map::current_projector(editor, canvas),
    ) else {
        return;
    };
    let Some(pos) = ctx.pointer_latest_pos() else {
        return;
    };
    let map = Mapping { rect };
    let p = map.to_norm(pos);
    let target = editor
        .project
        .surfaces_for_output(&out)
        .iter()
        .rev()
        .find(|s| contains(&s.warp, p))
        .map(|s| (s.id.clone(), s.warp.clone()));
    let Some((sid, warp)) = target else {
        return;
    };
    let outline: Vec<egui::Pos2> = handles(&warp, None)
        .into_iter()
        .map(|(_, q)| map.to_screen(q))
        .collect();
    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("lumap-drop-target"),
    ))
    .add(egui::Shape::closed_line(
        outline,
        Stroke::new(3.0, crate::theme::SIGNAL),
    ));
    if ctx.input(|i| i.pointer.any_released()) {
        if let Some(tile) = egui::DragAndDrop::take_payload::<Tile>(ctx) {
            apply_tile(editor, &Arc::unwrap_or_clone(tile), &sid);
            editor.selection = Selection::Surface {
                id: sid,
                mask: None,
            };
            editor.extra_selected.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumap_core::project::demo_project;

    #[test]
    fn tiles_put_looks_media_patterns_and_white_on_a_surface() {
        let mut e = Editor::new(demo_project(), None);
        let anniversary = LOOKS
            .iter()
            .position(|l| l.name == "Anniversary")
            .unwrap_or_else(|| unreachable!());
        apply_tile(&mut e, &Tile::Look(anniversary), "wall-left");
        assert_eq!(
            e.project
                .surface("wall-left")
                .map(|s| s.source.clone())
                .as_deref(),
            Some("look-anniversary")
        );
        apply_tile(&mut e, &Tile::Source("bars".into()), "wall-left");
        assert_eq!(
            e.project
                .surface("wall-left")
                .map(|s| s.source.clone())
                .as_deref(),
            Some("bars")
        );
        let before = e.project.sources.len();
        apply_tile(&mut e, &Tile::Pattern(TestPattern::Grid), "wall-left");
        assert_eq!(e.project.sources.len(), before, "the demo's grid is reused");
        apply_tile(&mut e, &Tile::Pattern(TestPattern::Checker), "wall-left");
        assert_eq!(e.project.sources.len(), before + 1);
        apply_tile(&mut e, &Tile::White, "wall-right");
        apply_tile(&mut e, &Tile::White, "logo");
        assert_eq!(e.project.sources.len(), before + 2, "one white, shared");
        apply_tile(&mut e, &Tile::Look(usize::MAX), "wall-left");
        assert_eq!(Filter::default(), Filter::All);
        assert!(format!("{:?}", LibraryState::default()).contains("All"));
    }

    #[test]
    fn a_click_goes_to_the_selection_or_to_the_whole_projector() {
        let mut e = Editor::new(demo_project(), None);
        let canvas = CanvasState::default();
        assert_eq!(targets(&e, &canvas), ["wall-left"]);
        e.selection = Selection::Surface {
            id: "logo".into(),
            mask: None,
        };
        e.extra_selected = vec!["wall-right".into()];
        assert_eq!(targets(&e, &canvas), ["logo", "wall-right"]);
        let empty = Editor::new(lumap_core::project::Project::default(), None);
        assert!(targets(&empty, &canvas).is_empty());
    }
}
