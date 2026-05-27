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

pub struct UiStylePlugin;

impl Plugin for UiStylePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_standard_button_visuals);
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
