//! Reusable, large, high-contrast widgets. All text sizes live here so they are
//! easy to tune for readability.

use iced::widget::{Button, Text, button, text};
use iced::{Border, Length, Theme};

pub const TITLE_SIZE: u32 = 44;
pub const BODY_SIZE: u32 = 28;
pub const BUTTON_SIZE: u32 = 30;
pub const SPACING: u32 = 24;

pub fn title<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(TITLE_SIZE)
}

pub fn body<'a>(content: impl text::IntoFragment<'a>) -> Text<'a> {
    text(content).size(BODY_SIZE)
}

/// The one obvious "do the thing" button on a screen.
/// `on_press: None` shows it greyed out.
pub fn main_button<'a, M: 'a>(
    label: impl text::IntoFragment<'a>,
    on_press: Option<M>,
) -> Button<'a, M> {
    big_button(label, on_press).style(button::success)
}

pub fn secondary_button<'a, M: 'a>(
    label: impl text::IntoFragment<'a>,
    on_press: Option<M>,
) -> Button<'a, M> {
    big_button(label, on_press).style(button::secondary)
}

fn big_button<'a, M: 'a>(label: impl text::IntoFragment<'a>, on_press: Option<M>) -> Button<'a, M> {
    button(text(label).size(BUTTON_SIZE).center().width(Length::Fill))
        .padding([18, 36])
        .on_press_maybe(on_press)
}

/// Style for a photo tile: a thick border when chosen.
pub fn tile_style(theme: &Theme, status: button::Status, selected: bool) -> button::Style {
    let palette = theme.extended_palette();
    let base = button::secondary(theme, status);
    let border = if selected {
        Border {
            color: palette.success.strong.color,
            width: 8.0,
            radius: 12.0.into(),
        }
    } else {
        Border {
            color: palette.background.strong.color,
            width: 2.0,
            radius: 12.0.into(),
        }
    };
    button::Style {
        background: Some(palette.background.weak.color.into()),
        text_color: palette.background.base.text,
        border,
        ..base
    }
}

/// "1 photo", "3 photos".
pub fn photos(count: usize) -> String {
    if count == 1 {
        "1 photo".to_string()
    } else {
        format!("{count} photos")
    }
}

#[cfg(test)]
mod tests {
    use super::photos;

    #[test]
    fn pluralises() {
        assert_eq!(photos(0), "0 photos");
        assert_eq!(photos(1), "1 photo");
        assert_eq!(photos(2), "2 photos");
    }
}
