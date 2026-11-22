use super::*;

#[test]
fn const_styles_can_be_specialized_without_mutating_the_base() {
    const BASE: Style = Style::new()
        .fg(Color::Cyan)
        .bg(Color::DarkGrey)
        .underline_color(Color::Red)
        .bold()
        .italic();
    const SPECIALIZED: StyledStr<'static> = BASE
        .fg(Color::Yellow)
        .remove_attributes(Attributes::ITALIC)
        .add_attributes(Attributes::UNDERCURLED)
        .apply("label");
    assert_eq!(Style::new(), Style::default());
    assert_eq!(SPECIALIZED.text, "label");
    assert_eq!(SPECIALIZED.style.foreground_color, Some(Color::Yellow));
    assert_eq!(SPECIALIZED.style.background_color, BASE.background_color);
    assert_eq!(SPECIALIZED.style.underline_color, BASE.underline_color);
    assert_eq!(
        SPECIALIZED.style.attributes,
        Attributes::BOLD | Attributes::UNDERCURLED
    );
    assert_eq!(BASE.foreground_color, Some(Color::Cyan));
    assert_eq!(BASE.attributes, Attributes::BOLD | Attributes::ITALIC);
    assert_eq!(BASE.attributes(Attributes::DIM).attributes, Attributes::DIM);
}

#[test]
fn attribute_set_operations_preserve_only_enabled_flags() {
    let mut attributes = Attributes::BOLD | Attributes::ITALIC | Attributes::UNDERLINED;
    assert!(attributes.contains(Attributes::BOLD | Attributes::UNDERLINED));
    assert!(attributes.contains(Attributes::empty()));
    assert!(!attributes.contains(Attributes::BOLD | Attributes::DIM));
    attributes.remove(Attributes::BOLD | Attributes::ITALIC);
    assert_eq!(attributes, Attributes::UNDERLINED);
    attributes |= Attributes::DIM | Attributes::ITALIC;
    attributes &= Attributes::DIM | Attributes::UNDERLINED;
    assert_eq!(attributes, Attributes::DIM | Attributes::UNDERLINED);
    attributes -= Attributes::DIM | Attributes::UNDERLINED;
    assert!(attributes.is_empty());
}
