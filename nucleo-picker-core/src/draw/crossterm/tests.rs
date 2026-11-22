use super::*;

#[test]
fn colors_preserve_names_indices_and_rgb_channels() {
    let named = [
        (Color::Reset, CrosstermColor::Reset),
        (Color::Black, CrosstermColor::Black),
        (Color::DarkGrey, CrosstermColor::DarkGrey),
        (Color::Red, CrosstermColor::Red),
        (Color::DarkRed, CrosstermColor::DarkRed),
        (Color::Green, CrosstermColor::Green),
        (Color::DarkGreen, CrosstermColor::DarkGreen),
        (Color::Yellow, CrosstermColor::Yellow),
        (Color::DarkYellow, CrosstermColor::DarkYellow),
        (Color::Blue, CrosstermColor::Blue),
        (Color::DarkBlue, CrosstermColor::DarkBlue),
        (Color::Magenta, CrosstermColor::Magenta),
        (Color::DarkMagenta, CrosstermColor::DarkMagenta),
        (Color::Cyan, CrosstermColor::Cyan),
        (Color::DarkCyan, CrosstermColor::DarkCyan),
        (Color::White, CrosstermColor::White),
        (Color::Grey, CrosstermColor::Grey),
    ];
    let indexed =
        (0..=u8::MAX).map(|value| (Color::AnsiValue(value), CrosstermColor::AnsiValue(value)));
    let rgb = [0, 1, 127, 255].into_iter().flat_map(|r| {
        [0, 1, 127, 255].into_iter().flat_map(move |g| {
            [0, 1, 127, 255]
                .into_iter()
                .map(move |b| (Color::Rgb { r, g, b }, CrosstermColor::Rgb { r, g, b }))
        })
    });
    for (native, crossterm) in named.into_iter().chain(indexed).chain(rgb) {
        assert_eq!(CrosstermColor::from(native), crossterm);
        assert_eq!(Color::from(crossterm), native);
    }
}

#[test]
fn positive_attributes_and_combinations_round_trip() {
    let pairs = [
        (Attributes::BOLD, Attribute::Bold),
        (Attributes::DIM, Attribute::Dim),
        (Attributes::ITALIC, Attribute::Italic),
        (Attributes::UNDERLINED, Attribute::Underlined),
        (Attributes::DOUBLE_UNDERLINED, Attribute::DoubleUnderlined),
        (Attributes::UNDERCURLED, Attribute::Undercurled),
        (Attributes::UNDERDOTTED, Attribute::Underdotted),
        (Attributes::UNDERDASHED, Attribute::Underdashed),
        (Attributes::SLOW_BLINK, Attribute::SlowBlink),
        (Attributes::RAPID_BLINK, Attribute::RapidBlink),
        (Attributes::REVERSE, Attribute::Reverse),
        (Attributes::HIDDEN, Attribute::Hidden),
        (Attributes::CROSSED_OUT, Attribute::CrossedOut),
        (Attributes::FRAKTUR, Attribute::Fraktur),
        (Attributes::FRAMED, Attribute::Framed),
        (Attributes::ENCIRCLED, Attribute::Encircled),
        (Attributes::OVERLINED, Attribute::OverLined),
    ];
    let mut all_native = Attributes::empty();
    let mut all_crossterm = CrosstermAttributes::none();
    for (native, crossterm) in pairs {
        all_native |= native;
        all_crossterm.set(crossterm);
        for (other_native, other_crossterm) in pairs {
            let combined_native = native | other_native;
            let combined_crossterm = CrosstermAttributes::from(crossterm).with(other_crossterm);
            assert_eq!(
                CrosstermAttributes::from(combined_native),
                combined_crossterm
            );
            assert_eq!(Attributes::from(combined_crossterm), combined_native);
        }
    }
    assert_eq!(CrosstermAttributes::from(all_native), all_crossterm);
    assert_eq!(Attributes::from(all_crossterm), all_native);
    for (native, crossterm) in pairs {
        assert_eq!(
            CrosstermAttributes::from(all_native - native),
            all_crossterm.without(crossterm)
        );
        assert_eq!(
            Attributes::from(all_crossterm.without(crossterm)),
            all_native - native
        );
    }
    assert_eq!(
        CrosstermAttributes::from(Attributes::empty()),
        CrosstermAttributes::none()
    );
    assert_eq!(
        Attributes::from(CrosstermAttributes::none()),
        Attributes::empty()
    );
}

#[test]
fn imported_off_commands_disable_attributes_without_affecting_other_flags() {
    for (off, attributes, expected) in [
        (
            Attribute::NoBold,
            Attributes::BOLD | Attributes::DIM,
            Attributes::DIM,
        ),
        (
            Attribute::NormalIntensity,
            Attributes::BOLD | Attributes::DIM | Attributes::ITALIC,
            Attributes::ITALIC,
        ),
        (
            Attribute::NoItalic,
            Attributes::ITALIC | Attributes::FRAKTUR | Attributes::BOLD,
            Attributes::BOLD,
        ),
        (
            Attribute::NoUnderline,
            Attributes::UNDERLINED
                | Attributes::DOUBLE_UNDERLINED
                | Attributes::UNDERCURLED
                | Attributes::UNDERDOTTED
                | Attributes::UNDERDASHED
                | Attributes::ITALIC,
            Attributes::ITALIC,
        ),
        (
            Attribute::NoBlink,
            Attributes::SLOW_BLINK | Attributes::RAPID_BLINK | Attributes::HIDDEN,
            Attributes::HIDDEN,
        ),
        (
            Attribute::NoReverse,
            Attributes::REVERSE | Attributes::BOLD,
            Attributes::BOLD,
        ),
        (
            Attribute::NoHidden,
            Attributes::HIDDEN | Attributes::DIM,
            Attributes::DIM,
        ),
        (
            Attribute::NotCrossedOut,
            Attributes::CROSSED_OUT | Attributes::OVERLINED,
            Attributes::OVERLINED,
        ),
        (
            Attribute::NotFramedOrEncircled,
            Attributes::FRAMED | Attributes::ENCIRCLED | Attributes::OVERLINED,
            Attributes::OVERLINED,
        ),
        (
            Attribute::NotOverLined,
            Attributes::OVERLINED | Attributes::FRAMED,
            Attributes::FRAMED,
        ),
    ] {
        let commands = CrosstermAttributes::from(attributes).with(off);
        assert_eq!(Attributes::from(commands), expected, "{off:?}");
        assert_eq!(
            Attributes::from(commands.with(Attribute::Reset)),
            expected,
            "{off:?}",
        );
        assert_eq!(
            Attributes::from(CrosstermAttributes::from(off)),
            Attributes::empty(),
            "{off:?}",
        );
    }
    let mut commands = CrosstermAttributes::none();
    for attribute in Attribute::iterator() {
        commands.set(attribute);
    }
    assert_eq!(Attributes::from(commands), Attributes::empty());
}

#[test]
fn imported_reset_precedes_positive_attributes_and_clears_colors() {
    let commands = CrosstermAttributes::from(Attribute::Reset);
    assert_eq!(Attributes::from(commands), Attributes::empty());
    assert_eq!(
        Attributes::from(commands.with(Attribute::Bold)),
        Attributes::BOLD,
    );
    let mut style = ContentStyle {
        foreground_color: Some(CrosstermColor::Red),
        background_color: Some(CrosstermColor::Blue),
        underline_color: Some(CrosstermColor::Green),
        attributes: commands
            .with(Attribute::Bold)
            .with(Attribute::Italic)
            .with(Attribute::NoItalic),
    };
    assert_eq!(Style::from(style), Style::new().bold());
    style.attributes.unset(Attribute::Reset);
    assert_eq!(
        Style::from(style),
        Style::new()
            .bold()
            .fg(Color::Red)
            .bg(Color::Blue)
            .underline_color(Color::Green),
    );
}

#[test]
fn styles_preserve_each_color_slot_and_positive_attributes() {
    let colors = [
        None,
        Some(Color::Reset),
        Some(Color::Red),
        Some(Color::DarkRed),
        Some(Color::AnsiValue(9)),
        Some(Color::Rgb { r: 1, g: 2, b: 3 }),
    ];
    for foreground_color in colors {
        for background_color in colors {
            for underline_color in colors {
                for attributes in [
                    Attributes::empty(),
                    Attributes::BOLD | Attributes::DIM | Attributes::ITALIC,
                    Attributes::UNDERCURLED | Attributes::FRAMED,
                ] {
                    let style = Style {
                        foreground_color,
                        background_color,
                        underline_color,
                        attributes,
                    };
                    let converted = ContentStyle::from(style);
                    assert_eq!(converted.foreground_color, foreground_color.map(Into::into));
                    assert_eq!(converted.background_color, background_color.map(Into::into));
                    assert_eq!(converted.underline_color, underline_color.map(Into::into));
                    assert_eq!(converted.attributes, attributes.into());
                    assert_eq!(Style::from(converted), style);
                    assert_eq!(ContentStyle::from(Style::from(converted)), converted);
                }
            }
        }
    }
}

#[test]
fn styled_strings_keep_the_original_borrow_in_both_directions() {
    let source = String::from("prefix é界👩🏽‍💻 suffix");
    let style = Style::new()
        .fg(Color::Cyan)
        .bg(Color::Reset)
        .underline_color(Color::AnsiValue(42))
        .bold();
    for text in [&source[..0], &source[7..source.len() - 7]] {
        let native = style.apply(text);
        let crossterm = StyledContent::from(native);
        assert_eq!(*crossterm.content(), text);
        assert_eq!(crossterm.content().as_ptr(), text.as_ptr());
        assert_eq!(*crossterm.style(), ContentStyle::from(style));
        let restored = StyledStr::from(crossterm);
        assert_eq!(restored, native);
        assert_eq!(restored.text.as_ptr(), text.as_ptr());
        assert_eq!(StyledContent::from(restored), crossterm);
    }
}
