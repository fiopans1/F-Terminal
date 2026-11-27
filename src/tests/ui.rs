use super::*;

#[test]
fn type_scale_keeps_legibility_floors_at_every_dpi() {
    for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
        let tokens = Tokens::new(12.5 * scale, scale);
        assert!(tokens.caption >= px(11.0, scale));
        assert!(tokens.small >= px(12.0, scale));
        assert!(tokens.body >= px(12.0, scale));
        assert!(tokens.section > tokens.body);
        assert!(tokens.title > tokens.section);
    }
}

#[test]
fn mix_clamps_between_the_two_colors() {
    let a = Rgb {
        r: 10,
        g: 20,
        b: 30,
    };
    let b = Rgb {
        r: 210,
        g: 220,
        b: 230,
    };
    assert_eq!(mix(a, b, -1.0), a);
    assert_eq!(mix(a, b, 2.0), b);
    assert_eq!(mix(a, b, 0.5).r, 110);
}
