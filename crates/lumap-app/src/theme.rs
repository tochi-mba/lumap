//! Visual style: the REX Technologies palette on a dark theme tuned for a dim control booth,
//! the embedded Inter typeface, a few branded widgets, and the window icon.
//!
//! The palette is the one every REX product site uses (ink, panel, raised, line, text, muted,
//! signal, live), so the app and its website read as one thing. Signal is the only accent:
//! selection, handles, the active cue, primary buttons. Live is reserved for show mode,
//! blackout and anything destructive. Everything here is pure data so it is testable without
//! a window: [`visuals`] returns the egui visuals, [`fonts`] the font definitions and [`icon`]
//! draws the icon into an RGBA buffer.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Response, RichText,
    Stroke, TextStyle, Ui,
};
use std::sync::Arc;

/// Page background, the darkest tone.
pub const INK: Color32 = Color32::from_rgb(8, 10, 9);
/// Panels and cards.
pub const PANEL: Color32 = Color32::from_rgb(17, 21, 18);
/// Windows, menus and widgets that sit on a panel.
pub const RAISED: Color32 = Color32::from_rgb(24, 30, 25);
/// Hairlines and borders.
pub const LINE: Color32 = Color32::from_rgb(41, 48, 42);
/// Body text.
pub const TEXT: Color32 = Color32::from_rgb(242, 245, 238);
/// Secondary text.
pub const MUTED: Color32 = Color32::from_rgb(133, 141, 131);
/// The accent: selection, handles, the active cue, primary actions.
pub const SIGNAL: Color32 = Color32::from_rgb(215, 255, 63);
/// Show mode, blackout, destructive actions, errors.
pub const LIVE: Color32 = Color32::from_rgb(255, 119, 77);

/// Accent used for selection, handles and the active cue (the signal colour).
pub const ACCENT: Color32 = SIGNAL;
/// Background of the canvas and output previews.
pub const CANVAS_BG: Color32 = INK;

/// Name under which Inter is registered with egui.
pub const FONT_BODY: &str = "Inter";
/// Name under which `Inter SemiBold` is registered with egui.
pub const FONT_STRONG: &str = "Inter SemiBold";
/// The proportional family that resolves to `Inter SemiBold`.
pub fn strong_family() -> FontFamily {
    FontFamily::Name(Arc::from(FONT_STRONG))
}

/// The lumap dark theme.
pub fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.weak_text_color = Some(MUTED);
    v.panel_fill = PANEL;
    v.window_fill = RAISED;
    v.window_stroke = Stroke::new(1.0, LINE);
    v.extreme_bg_color = INK;
    v.faint_bg_color = RAISED;
    v.code_bg_color = RAISED;
    v.selection.bg_fill = SIGNAL.linear_multiply(0.25);
    v.selection.stroke = Stroke::new(1.0, SIGNAL);
    v.hyperlink_color = SIGNAL;
    v.warn_fg_color = SIGNAL;
    v.error_fg_color = LIVE;
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.bg_fill = RAISED;
    v.widgets.inactive.weak_bg_fill = RAISED;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.bg_fill = Color32::from_rgb(34, 42, 36);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(34, 42, 36);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, MUTED);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.active.bg_fill = SIGNAL.linear_multiply(0.45);
    v.widgets.active.weak_bg_fill = SIGNAL.linear_multiply(0.45);
    v.widgets.active.bg_stroke = Stroke::new(1.0, SIGNAL);
    v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.open.bg_fill = RAISED;
    v.widgets.open.weak_bg_fill = RAISED;
    v.widgets.open.bg_stroke = Stroke::new(1.0, MUTED);
    v.widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(6);
    }
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow.spread = 0;
    v.popup_shadow.spread = 0;
    v.striped = false;
    v
}

/// Inter (OFL 1.1, `assets/fonts/`) ahead of egui's own fonts, which stay as fallbacks for
/// the glyphs Inter lacks.
pub fn fonts() -> FontDefinitions {
    let mut f = FontDefinitions::default();
    f.font_data.insert(
        FONT_BODY.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    f.font_data.insert(
        FONT_STRONG.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    f.families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, FONT_BODY.to_owned());
    let mut strong = vec![FONT_STRONG.to_owned()];
    strong.extend(
        f.families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    f.families.insert(strong_family(), strong);
    f
}

/// Applies the theme, fonts, spacing and text sizes to a context. Idempotent.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_visuals(visuals());
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.spacing.slider_width = 160.0;
        style.spacing.interact_size.y = 22.0;
        style.spacing.window_margin = egui::Margin::same(14);
        style.spacing.menu_margin = egui::Margin::same(8);
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(11.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.0, strong_family())),
            (TextStyle::Heading, FontId::new(17.0, strong_family())),
            (
                TextStyle::Monospace,
                FontId::new(12.5, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}

/// A small section label with a hairline under it: the site's eyebrow. The text is the
/// widget's accessible name, unchanged, so tests and screen readers see the plain title.
pub fn section_header(ui: &mut Ui, title: &str) -> Response {
    ui.add_space(6.0);
    let r = ui.add(egui::Label::new(
        RichText::new(title)
            .size(11.0)
            .family(strong_family())
            .color(MUTED),
    ));
    let rect = ui.available_rect_before_wrap();
    let y = ui.cursor().top() + 2.0;
    ui.painter()
        .hline(rect.x_range(), y, Stroke::new(1.0, LINE));
    ui.add_space(6.0);
    r
}

/// The one button in a group that does the main thing: signal fill, ink text.
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(INK).family(strong_family()))
            .fill(SIGNAL)
            .stroke(Stroke::NONE),
    )
}

/// A button for something hard to undo: live outline, live text.
pub fn danger_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(LIVE).family(strong_family()))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, LIVE)),
    )
}

/// A small status chip. `live` paints it in the live colour (show mode, blackout, learning).
pub fn pill(ui: &mut Ui, text: &str, live: bool) -> Response {
    let (fg, bg, line) = if live {
        (LIVE, LIVE.linear_multiply(0.18), LIVE)
    } else {
        (MUTED, RAISED, LINE)
    };
    egui::Frame::new()
        .fill(bg)
        .stroke(Stroke::new(1.0, line))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| ui.label(RichText::new(text).small().color(fg)))
        .inner
}

/// What an empty list says: a title in text and a hint in muted.
pub fn empty_state(ui: &mut Ui, title: &str, hint: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).family(strong_family()));
    ui.label(RichText::new(hint).weak().small());
    ui.add_space(8.0);
}

/// WCAG relative luminance of a colour, in sRGB.
#[cfg(test)]
fn luminance(c: Color32) -> f64 {
    let channel = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.039_28 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

/// WCAG contrast ratio between two opaque colours.
#[cfg(test)]
pub fn contrast(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Draws the app icon: a projector beam (a light wedge in signal) on an ink rounded square,
/// with a bright lens dot. Returns straight-alpha RGBA, row-major.
pub fn icon(size: u32) -> Vec<u8> {
    let n = size.max(8);
    let s = f64::from(n);
    let mut out = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (u, v) = ((f64::from(x) + 0.5) / s, (f64::from(y) + 0.5) / s);
            // Rounded square mask.
            let r = 0.18;
            let (cx, cy) = ((u - 0.5).abs() - (0.5 - r), (v - 0.5).abs() - (0.5 - r));
            let d = (cx.max(0.0).powi(2) + cy.max(0.0).powi(2)).sqrt() + cx.max(cy).min(0.0) - r;
            let alpha = (0.5 - d * s).clamp(0.0, 1.0);
            // Beam: a wedge from the lens at (0.25, 0.5) widening to the right.
            let (lx, ly) = (0.25, 0.5);
            let dx = u - lx;
            let inside_beam = dx > 0.0 && (v - ly).abs() < dx * 0.4 && u < 0.9;
            let beam = if inside_beam {
                (1.0 - dx / 0.9).clamp(0.8, 1.0)
            } else {
                0.0
            };
            let lens = f64::from(u8::from(
                ((u - lx).powi(2) + (v - ly).powi(2)).sqrt() < 0.09,
            ));
            let base = [f64::from(INK.r()), f64::from(INK.g()), f64::from(INK.b())];
            let tint = [
                f64::from(SIGNAL.r()),
                f64::from(SIGNAL.g()),
                f64::from(SIGNAL.b()),
            ];
            let white = [
                f64::from(TEXT.r()),
                f64::from(TEXT.g()),
                f64::from(TEXT.b()),
            ];
            let mut px = [0u8; 4];
            for c in 0..3 {
                let col = base[c] + (tint[c] - base[c]) * beam + (white[c] - base[c]) * lens;
                px[c] = col.clamp(0.0, 255.0).round() as u8;
            }
            px[3] = (alpha * 255.0).round() as u8;
            out.extend_from_slice(&px);
        }
    }
    out
}

/// The icon as eframe wants it.
pub fn icon_data(size: u32) -> egui::IconData {
    let n = size.max(8);
    egui::IconData {
        rgba: icon(n),
        width: n,
        height: n,
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    #[test]
    fn icon_has_a_transparent_corner_an_opaque_centre_and_a_bright_lens() {
        let n = 64;
        let px = icon(n);
        assert_eq!(px.len(), (n * n * 4) as usize);
        let at = |x: u32, y: u32| {
            let i = ((y * n + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        assert_eq!(at(0, 0)[3], 0, "corner is outside the rounded square");
        assert_eq!(at(32, 32)[3], 255);
        assert_eq!(
            at(16, 32),
            [TEXT.r(), TEXT.g(), TEXT.b(), 255],
            "lens is text white"
        );
        let beam = at(40, 32);
        assert!(
            beam[1] > beam[0] && beam[0] > beam[2] && beam[1] > 200,
            "beam is signal: {beam:?}"
        );
        let dark = at(40, 4);
        assert!(dark[1] < 40, "outside the beam stays ink: {dark:?}");
        let d = icon_data(4);
        assert_eq!((d.width, d.height), (8, 8), "tiny sizes are clamped");
        assert_eq!(d.rgba.len(), 8 * 8 * 4);
    }

    #[test]
    fn theme_applies_to_a_context_with_inter_and_the_rex_palette() {
        let ctx = egui::Context::default();
        apply(&ctx);
        apply(&ctx);
        let style = ctx.global_style();
        assert_eq!(style.visuals.selection.stroke.color, SIGNAL);
        assert_eq!(style.visuals.panel_fill, PANEL);
        assert_eq!(style.visuals.override_text_color, Some(TEXT));
        assert_eq!(style.text_styles[&TextStyle::Body].size, 13.0);
        assert_eq!(
            style.text_styles[&TextStyle::Heading].family,
            strong_family()
        );
        assert!(visuals().dark_mode);
        let f = fonts();
        assert!(f.font_data.contains_key(FONT_BODY) && f.font_data.contains_key(FONT_STRONG));
        assert_eq!(f.families[&FontFamily::Proportional][0], FONT_BODY);
        assert_eq!(f.families[&strong_family()][0], FONT_STRONG);
        assert!(
            f.families[&strong_family()].len() > 1,
            "fallbacks follow the strong face"
        );
        assert_eq!(ACCENT, SIGNAL);
        assert_eq!(CANVAS_BG, INK);
    }

    #[test]
    fn the_palette_reads_in_a_dim_booth() {
        assert!(contrast(TEXT, PANEL) >= 7.0, "{}", contrast(TEXT, PANEL));
        assert!(contrast(TEXT, RAISED) >= 7.0);
        assert!(contrast(INK, SIGNAL) >= 7.0, "{}", contrast(INK, SIGNAL));
        assert!(contrast(MUTED, PANEL) >= 4.5, "{}", contrast(MUTED, PANEL));
        assert!(contrast(LIVE, INK) >= 4.5, "{}", contrast(LIVE, INK));
        assert!(contrast(SIGNAL, INK) > contrast(MUTED, INK));
        assert_eq!(contrast(TEXT, TEXT), 1.0);
    }

    #[test]
    fn branded_widgets_keep_their_text_as_the_accessible_name() {
        let mut clicks = Vec::new();
        let mut h = Harness::new_ui(|ui| {
            apply(ui.ctx());
            section_header(ui, "Outputs");
            if primary_button(ui, "GO").clicked() {
                clicks.push("GO");
            }
            if danger_button(ui, "Discard").clicked() {
                clicks.push("Discard");
            }
            pill(ui, "OSC 9000", false);
            pill(ui, "BLACKOUT", true);
            empty_state(ui, "No cues yet", "Store a cue to keep this look.");
        });
        h.run();
        h.get_by_label("Outputs");
        h.get_by_label("OSC 9000");
        h.get_by_label("BLACKOUT");
        h.get_by_label("No cues yet");
        h.get_by_label("Store a cue to keep this look.");
        h.get_by_label("GO").click();
        h.run();
        h.get_by_label("Discard").click();
        h.run();
        drop(h);
        assert_eq!(clicks, ["GO", "Discard"]);
    }
}
