use super::*;

fn text(l: &Line) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[test]
fn every_row_of_the_beans_is_as_wide_and_holds_only_their_tones() {
    assert_eq!(LARGE.len() % 2, 0);
    assert!(LARGE.iter().all(|r| r.len() == LARGE[0].len()));
    assert!(LARGE
        .iter()
        .all(|r| r.bytes().all(|b| b"DTLO.".contains(&b))));
    assert!(POD.iter().all(|r| r.chars().count() == 5));
}

#[test]
fn two_pixels_make_a_cell_in_their_own_colours() {
    let l = lines(&["DT.D", "DD.."]);
    assert_eq!(text(&l[0]), "█▀ ▀");
    assert_eq!(l[0].spans[1].style, Style::new().fg(BEAN).bg(HUSK));
    assert_eq!(l[0].spans[3].style, Style::new().fg(HUSK));
    assert_eq!(lines(&LARGE).len(), 5);
}

fn rows(width: u16) -> Vec<String> {
    let area = Rect::new(0, 0, width, 3);
    let mut buf = Buffer::empty(area);
    draw_corner(&mut buf, area, 40);
    (0..3)
        .map(|y| {
            (0..width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect()
        })
        .collect()
}

#[test]
fn two_tiny_beans_sit_top_right_and_stay_out_of_a_narrow_screen() {
    let r = rows(80);
    assert!(r[0].trim_end().ends_with("▗▟█▙▖ ▗▟█▙▖"), "{r:?}");
    assert!(r[1].trim_end().ends_with("▝▜█▛▘ ▝▜█▛▘ cocoa"), "{r:?}");
    assert!(r[2].trim().is_empty());
    assert!(rows(50).iter().all(|r| r.trim().is_empty()));
}
