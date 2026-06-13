use bevy::prelude::*;

pub mod palette {
    use bevy::color::Color;

    pub const BG_OVERLAY: Color = Color::srgba(0.03, 0.04, 0.07, 0.82);
    pub const BG_PANEL: Color = Color::srgba(0.08, 0.10, 0.16, 0.94);
    pub const BG_PANEL_SUNK: Color = Color::srgba(0.05, 0.07, 0.11, 0.92);
    pub const BG_PANEL_RAISED: Color = Color::srgba(0.11, 0.14, 0.20, 0.95);

    pub const BG_BUTTON: Color = Color::srgba(0.14, 0.19, 0.28, 0.95);
    pub const BG_BUTTON_HOVER: Color = Color::srgba(0.20, 0.27, 0.40, 1.0);
    pub const BG_BUTTON_PRESSED: Color = Color::srgba(0.26, 0.36, 0.54, 1.0);

    pub const BORDER: Color = Color::srgba(0.26, 0.34, 0.48, 1.0);
    pub const BORDER_HOVER: Color = Color::srgba(0.42, 0.56, 0.78, 1.0);
    pub const BORDER_PRESSED: Color = Color::srgba(0.58, 0.78, 1.0, 1.0);
    pub const BORDER_SUBTLE: Color = Color::srgba(0.18, 0.24, 0.34, 0.95);
    pub const BORDER_ACCENT: Color = Color::srgb(0.55, 0.75, 0.95);

    pub const TEXT_HEADING: Color = Color::srgb(0.97, 0.98, 1.0);
    pub const TEXT_PRIMARY: Color = Color::srgb(0.92, 0.94, 0.98);
    pub const TEXT_SECONDARY: Color = Color::srgb(0.70, 0.76, 0.86);
    pub const TEXT_DIM: Color = Color::srgb(0.50, 0.55, 0.62);

    pub const ACCENT_PRIMARY: Color = Color::srgb(0.55, 0.75, 0.95);
    pub const ACCENT_SUCCESS: Color = Color::srgb(0.45, 0.85, 0.55);
    pub const ACCENT_DANGER: Color = Color::srgb(0.92, 0.40, 0.45);
    pub const ACCENT_WARNING: Color = Color::srgb(0.95, 0.78, 0.35);

    // ---- In-battle palette ----
    /// Side tints for floating combatant frames and the turn strip.
    pub const ALLY: Color = Color::srgb(0.50, 0.74, 0.96);
    pub const ENEMY: Color = Color::srgb(0.93, 0.46, 0.46);
    /// Health bar fill, shading to amber/red as it drains (see `health_fill`).
    pub const HEALTH_FULL: Color = Color::srgb(0.42, 0.82, 0.48);
    pub const HEALTH_MID: Color = Color::srgb(0.92, 0.74, 0.32);
    pub const HEALTH_LOW: Color = Color::srgb(0.90, 0.34, 0.34);
    /// Morale / resolve bar fill (a calmer violet, distinct from health).
    pub const MORALE: Color = Color::srgb(0.62, 0.55, 0.92);
    /// Empty track behind any stat bar.
    pub const BAR_TRACK: Color = Color::srgba(0.04, 0.05, 0.08, 0.85);
    /// Status-badge tints by tier (1 mild → 3 severe).
    pub const STATUS_TIER_1: Color = Color::srgb(0.70, 0.74, 0.42);
    pub const STATUS_TIER_2: Color = Color::srgb(0.90, 0.62, 0.30);
    pub const STATUS_TIER_3: Color = Color::srgb(0.90, 0.34, 0.34);

    // ---- Title-scene palette ----
    /// Warm gold used for the game's wordmark — evokes shrine lantern light.
    pub const BRAND: Color = Color::srgb(0.95, 0.83, 0.58);
    pub const BRAND_BRIGHT: Color = Color::srgb(1.0, 0.94, 0.78);
    /// Soft bloom tint that sits behind the title (kept gentle so the 3D cast
    /// rendered behind the UI is not washed out).
    pub const SCENE_GLOW: Color = Color::srgba(0.58, 0.70, 0.98, 0.10);
    /// Near-opaque dark used by the legibility scrims behind text.
    pub const SCRIM: Color = Color::srgba(0.01, 0.02, 0.05, 0.92);
}

pub mod spacing {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

pub mod radius {
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 10.0;
    pub const LG: f32 = 14.0;
}

pub mod font_size {
    pub const TITLE: f32 = 48.0;
    pub const HEADING: f32 = 32.0;
    pub const SUBHEADING: f32 = 22.0;
    pub const BODY_LG: f32 = 20.0;
    pub const BODY: f32 = 18.0;
    pub const LABEL: f32 = 16.0;
    pub const SMALL: f32 = 14.0;
}

pub fn overlay_root() -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            position_type: PositionType::Absolute,
            ..default()
        },
        BackgroundColor(palette::BG_OVERLAY),
    )
}

pub fn panel(width_px: f32) -> impl Bundle {
    (
        Node {
            width: Val::Px(width_px),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: Val::Px(spacing::MD),
            padding: UiRect::all(Val::Px(spacing::XL)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(radius::LG)),
            ..default()
        },
        BackgroundColor(palette::BG_PANEL),
        BorderColor::all(palette::BORDER_SUBTLE),
    )
}

pub fn floating_panel(width_px: f32) -> impl Bundle {
    (
        Node {
            width: Val::Px(width_px),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: Val::Px(spacing::SM),
            padding: UiRect::all(Val::Px(spacing::LG)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(radius::MD)),
            ..default()
        },
        BackgroundColor(palette::BG_PANEL),
        BorderColor::all(palette::BORDER_ACCENT),
    )
}

/// Transparent, full-screen root for the main-menu scene.
///
/// The backdrop is a live 3D stage (see `menu::spawn_menu_scene`) rendered by
/// the dedicated menu camera, so the UI root itself paints nothing — it just
/// centres its page content and lets the cast show through. Legibility comes
/// from [`scene_vignette`] / [`bottom_scrim`] / [`top_scrim`] and the page's own
/// panels.
pub fn menu_scene_overlay() -> impl Bundle {
    Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        display: Display::Flex,
        flex_direction: FlexDirection::Column,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        position_type: PositionType::Absolute,
        ..default()
    }
}

/// A dark gradient hugging the bottom of the screen so foreground buttons stay
/// legible over the bright 3D cast behind them.
pub fn bottom_scrim() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(42.0),
            ..default()
        },
        BackgroundGradient(vec![Gradient::Linear(LinearGradient::to_top(vec![
            ColorStop::percent(palette::SCRIM, 0.0),
            ColorStop::percent(Color::srgba(0.01, 0.02, 0.05, 0.0), 100.0),
        ]))]),
    )
}

/// Mirror of [`bottom_scrim`] for the top of the screen, framing the wordmark.
pub fn top_scrim() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(30.0),
            ..default()
        },
        BackgroundGradient(vec![Gradient::Linear(LinearGradient::to_bottom(vec![
            ColorStop::percent(palette::SCRIM, 0.0),
            ColorStop::percent(Color::srgba(0.01, 0.02, 0.05, 0.0), 100.0),
        ]))]),
    )
}

/// A soft warm bloom centred on the screen, drawn behind the title. Spawn it
/// *before* the foreground content so it renders underneath.
pub fn scene_glow() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundGradient(vec![Gradient::Radial(RadialGradient::new(
            UiPosition::CENTER,
            RadialGradientShape::FarthestSide,
            vec![
                ColorStop::percent(palette::SCENE_GLOW, 0.0),
                ColorStop::percent(Color::srgba(0.0, 0.0, 0.0, 0.0), 45.0),
            ],
        ))]),
    )
}

/// Darkened corners to frame the scene and draw the eye to the centre.
pub fn scene_vignette() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundGradient(vec![Gradient::Radial(RadialGradient::new(
            UiPosition::CENTER,
            RadialGradientShape::FarthestCorner,
            vec![
                ColorStop::percent(Color::srgba(0.0, 0.0, 0.0, 0.0), 38.0),
                ColorStop::percent(Color::srgba(0.0, 0.0, 0.0, 0.62), 100.0),
            ],
        ))]),
    )
}

pub fn button_node(height_px: f32) -> Node {
    Node {
        height: Val::Px(height_px),
        display: Display::Flex,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        padding: UiRect::all(Val::Px(spacing::SM)),
        border: UiRect::all(Val::Px(1.5)),
        border_radius: BorderRadius::all(Val::Px(radius::MD)),
        ..default()
    }
}

pub fn button_visual() -> impl Bundle {
    (
        BackgroundColor(palette::BG_BUTTON),
        BorderColor::all(palette::BORDER),
    )
}

// ---------------------------------------------------------------------------
// Image-skinned buttons
//
// Interactivity lives on the `Button` + `Node` (its layout rectangle), never on
// the pixels — so an image-skinned button clicks and hovers exactly like a
// coloured one. `button_skin()` is a drop-in replacement for `button_visual()`:
// keep the same `Button::default()` + `*_node()` and your existing
// `Interaction` queries keep working unchanged.
// ---------------------------------------------------------------------------

/// Handles to the shared button artwork, loaded once at startup. Drop the PNGs
/// at these paths under `assets/`; missing files just log and render nothing
/// (the layout box stays clickable), so this is safe to wire up before the art
/// exists.
///
/// NOTE: the UI uses the flat-colour [`button_visual`] everywhere, so this
/// skinned path is dormant. We leave the handles empty (default) instead of
/// eagerly loading PNGs that don't exist yet — that spammed "Path not found:
/// ui/button_*.png" every boot. When the art lands at `assets/ui/button_*.png`,
/// restore the `assets.load(...)` calls below and start spawning `button_skin`.
#[derive(Resource, Clone, Default)]
pub struct UiAssets {
    pub button_normal: Handle<Image>,
    pub button_hover: Handle<Image>,
    pub button_pressed: Handle<Image>,
}

/// The three textures a skinned button swaps between. Carried per-button so
/// different buttons can use different art while sharing one swap system.
#[derive(Component, Clone)]
pub struct ButtonSkin {
    pub normal: Handle<Image>,
    pub hovered: Handle<Image>,
    pub pressed: Handle<Image>,
}

/// Border thickness (in source-texture pixels) preserved by 9-slicing, so the
/// button can be any size without stretching its corners. Tune to your art.
const SKIN_SLICE_BORDER: f32 = 12.0;

/// Drop-in replacement for [`button_visual`] that renders the standard button
/// artwork instead of a flat colour. Pair with `button_node(..)`/`toggle_node()`
/// for the layout, exactly as before:
///
/// ```ignore
/// parent.spawn((Button::default(), button_node(48.0), button_skin(&ui), MyTag));
/// ```
pub fn button_skin(ui: &UiAssets) -> impl Bundle {
    (
        ImageNode {
            image: ui.button_normal.clone(),
            // 9-slice so one small PNG skins any button size cleanly.
            image_mode: NodeImageMode::Sliced(TextureSlicer {
                border: BorderRect::all(SKIN_SLICE_BORDER),
                ..default()
            }),
            ..default()
        },
        ButtonSkin {
            normal: ui.button_normal.clone(),
            hovered: ui.button_hover.clone(),
            pressed: ui.button_pressed.clone(),
        },
    )
}

pub fn title_text(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont {
            font_size: font_size::TITLE,
            ..default()
        },
        TextColor(palette::TEXT_HEADING),
    )
}

pub fn heading_text(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont {
            font_size: font_size::HEADING,
            ..default()
        },
        TextColor(palette::TEXT_HEADING),
    )
}

pub fn subheading_text(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont {
            font_size: font_size::SUBHEADING,
            ..default()
        },
        TextColor(palette::TEXT_PRIMARY),
    )
}

pub fn body_text(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont {
            font_size: font_size::BODY,
            ..default()
        },
        TextColor(palette::TEXT_PRIMARY),
    )
}

pub fn label_text(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont {
            font_size: font_size::LABEL,
            ..default()
        },
        TextColor(palette::TEXT_SECONDARY),
    )
}

pub fn button_text(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont {
            font_size: font_size::BODY,
            ..default()
        },
        TextColor(palette::TEXT_PRIMARY),
    )
}

pub fn button_text_lg(text: impl Into<String>) -> impl Bundle {
    (
        Text::new(text.into()),
        TextFont {
            font_size: font_size::SUBHEADING,
            ..default()
        },
        TextColor(palette::TEXT_PRIMARY),
    )
}

// ---------------------------------------------------------------------------
// In-battle primitives
// ---------------------------------------------------------------------------

/// Health-bar fill colour for a fill fraction in `0.0..=1.0`: green when full,
/// fading through amber and into red as it drains.
pub fn health_fill(frac: f32) -> Color {
    let f = frac.clamp(0.0, 1.0);
    if f >= 0.5 {
        // green → amber over the top half.
        palette::HEALTH_MID.mix(&palette::HEALTH_FULL, (f - 0.5) * 2.0)
    } else {
        // amber → red over the bottom half.
        palette::HEALTH_LOW.mix(&palette::HEALTH_MID, f * 2.0)
    }
}

/// Tint for a status badge of the given tier (1..=3); clamps out-of-range tiers.
pub fn status_tier_color(tier: u8) -> Color {
    match tier {
        0 | 1 => palette::STATUS_TIER_1,
        2 => palette::STATUS_TIER_2,
        _ => palette::STATUS_TIER_3,
    }
}

/// An empty stat-bar track (the dark groove a fill sits inside). Spawn a
/// [`bar_fill`] as its child. `width`/`height` are in pixels.
pub fn bar_track(width: f32, height: f32) -> impl Bundle {
    (
        Node {
            width: Val::Px(width),
            height: Val::Px(height),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(radius::SM)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(palette::BAR_TRACK),
        BorderColor::all(palette::BORDER_SUBTLE),
    )
}

/// The coloured fill inside a [`bar_track`], sized to `frac` of the track width.
pub fn bar_fill(frac: f32, color: Color) -> impl Bundle {
    (
        Node {
            width: Val::Percent(frac.clamp(0.0, 1.0) * 100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(color),
    )
}

/// A compact pop-up panel for the command-bar flyouts: dark, accent-bordered,
/// column layout. Anchored by the caller (absolute positioning).
pub fn flyout_panel() -> impl Bundle {
    (
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: Val::Px(spacing::XS),
            padding: UiRect::all(Val::Px(spacing::SM)),
            border: UiRect::all(Val::Px(1.5)),
            border_radius: BorderRadius::all(Val::Px(radius::MD)),
            ..default()
        },
        BackgroundColor(palette::BG_PANEL),
        BorderColor::all(palette::BORDER_ACCENT),
    )
}

pub struct UiStylePlugin;

impl Plugin for UiStylePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiAssets>().add_systems(
            Update,
            (update_standard_button_visuals, update_skinned_button_visuals),
        );
    }
}

/// Image counterpart to [`update_standard_button_visuals`]: swaps the button's
/// texture on hover/press. Runs for every `ButtonSkin` across all screens.
fn update_skinned_button_visuals(
    mut buttons: Query<
        (&Interaction, &ButtonSkin, &mut ImageNode),
        (Changed<Interaction>, With<Button>),
    >,
) {
    for (interaction, skin, mut image) in &mut buttons {
        image.image = match *interaction {
            Interaction::Pressed => skin.pressed.clone(),
            Interaction::Hovered => skin.hovered.clone(),
            Interaction::None => skin.normal.clone(),
        };
    }
}

fn update_standard_button_visuals(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (Changed<Interaction>, With<Button>),
    >,
) {
    for (interaction, mut bg, mut border) in &mut buttons {
        match *interaction {
            Interaction::Pressed => {
                bg.0 = palette::BG_BUTTON_PRESSED;
                set_border(&mut border, palette::BORDER_PRESSED);
            }
            Interaction::Hovered => {
                bg.0 = palette::BG_BUTTON_HOVER;
                set_border(&mut border, palette::BORDER_HOVER);
            }
            Interaction::None => {
                bg.0 = palette::BG_BUTTON;
                set_border(&mut border, palette::BORDER);
            }
        }
    }
}

fn set_border(border: &mut BorderColor, color: Color) {
    border.top = color;
    border.right = color;
    border.bottom = color;
    border.left = color;
}
