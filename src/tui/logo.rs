//! cocoa's mark: two cocoa beans leaning apart, each with the groove down
//! its length, on olive, as pixel art in half blocks (two pixels a cell,
//! each its own colour), heads the pop-ups. Two tiny beans in quadrant
//! blocks sit in the top-right corner of the views, as faba's beans do.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// The beans' grooves.
pub const HUSK: Color = Color::Rgb(92, 58, 30);
/// The light bean.
pub const BEAN: Color = Color::Rgb(201, 155, 100);
/// The dark bean.
pub const LEAF: Color = Color::Rgb(150, 105, 60);
/// The olive the beans sit on.
pub const OLIVE: Color = Color::Rgb(78, 88, 40);

/// The beans in five rows: ten pixels high, `T` the light bean, `L` the
/// dark one, `D` their grooves, on `O` olive. Each is almond-shaped, the
/// dark one the light one mirrored.
pub const LARGE: [&str; 10] = [
    "OOOOOOOOOOOOOOO",
    "OOOOTTOOOLLOOOO",
    "OOOTTTTOLLLLOOO",
    "OOTTTDTOLDLLLOO",
    "OOTTDTTOLLDLLOO",
    "OTTTDTOOOLDLLLO",
    "OTTDTTOOOLLDLLO",
    "OTTTTOOOOOLLLLO",
    "OOTTOOOOOOOLLOO",
    "OOOOOOOOOOOOOOO",
];

fn colour(px: u8) -> Option<Color> {
    match px {
        b'D' => Some(HUSK),
        b'T' => Some(BEAN),
        b'L' => Some(LEAF),
        b'O' => Some(OLIVE),
        _ => None,
    }
}

/// `art` (an even number of pixel rows) as text: each cell the pixel above
/// and the one below it, the background left as it is where there is none.
#[must_use]
pub fn lines(art: &[&str]) -> Vec<Line<'static>> {
    art.chunks(2)
        .map(|pair| {
            let (top, bottom) = (
                pair[0].as_bytes(),
                pair.get(1).map_or(&[][..], |b| b.as_bytes()),
            );
            let spans = (0..top.len().max(bottom.len()))
                .map(|x| {
                    let t = top.get(x).copied().and_then(colour);
                    let b = bottom.get(x).copied().and_then(colour);
                    match (t, b) {
                        (None, None) => Span::raw(" "),
                        (Some(t), None) => Span::styled("▀", Style::new().fg(t)),
                        (None, Some(b)) => Span::styled("▄", Style::new().fg(b)),
                        (Some(t), Some(b)) if t == b => Span::styled("█", Style::new().fg(t)),
                        (Some(t), Some(b)) => Span::styled("▀", Style::new().fg(t).bg(b)),
                    }
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect()
}

/// One bean in quadrant blocks, five columns by two rows, pointed at both
/// ends, as faba draws a bean.
pub const POD: [&str; 2] = ["▗▟█▙▖", "▝▜█▛▘"];

/// The two beans' tones, as in the large mark.
pub const TONES: [Color; 2] = [BEAN, LEAF];

/// Columns the corner mark takes: two beans a column apart, a space, the
/// name.
const WIDTH: u16 = 2 * 5 + 1 + 1 + 5;

/// Draw the beans and the name at the right end of the first two rows of
/// `area`, when it leaves `reserve` columns free on its left for the rows'
/// own text.
pub fn draw_corner(buf: &mut Buffer, area: Rect, reserve: u16) {
    if area.height < 2 || area.width < (WIDTH + 1).saturating_add(reserve) {
        return;
    }
    let x = area.right() - WIDTH - 1;
    for (k, &tone) in TONES.iter().enumerate() {
        let style = Style::new().fg(tone);
        for (dy, row) in POD.iter().enumerate() {
            buf.set_string(x + 6 * k as u16, area.y + dy as u16, row, style);
        }
    }
    let name = Style::new().fg(BEAN).add_modifier(Modifier::BOLD);
    buf.set_string(x + WIDTH - 5, area.y + 1, "cocoa", name);
}

/// The large beans beside `title` and `under`, for the head of a pop-up.
#[must_use]
pub fn heading(
    title: &str,
    under: &str,
    title_style: Style,
    under_style: Style,
) -> Vec<Line<'static>> {
    let mut out = lines(&LARGE);
    for l in &mut out {
        l.spans.insert(0, Span::raw(" "));
    }
    out[2]
        .spans
        .push(Span::styled(format!("   {title}"), title_style));
    out[3]
        .spans
        .push(Span::styled(format!("   {under}"), under_style));
    out
}

#[cfg(test)]
#[path = "tests/logo.rs"]
mod tests;
