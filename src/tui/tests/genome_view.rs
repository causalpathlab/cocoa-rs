use super::*;
use ratatui::crossterm::event::KeyModifiers;

fn key(v: &mut GenomeView, code: KeyCode) -> bool {
    v.nav_key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn the_genome_zooms_about_its_middle_and_scrolls_within_its_ends() {
    let mut v = GenomeView::default();
    assert_eq!(v.window(), "whole genome");
    assert!(key(&mut v, KeyCode::Char('+')));
    assert_eq!((v.zoom, v.from), (2.0, 0.25));
    key(&mut v, KeyCode::Right);
    assert_eq!(v.from, 0.375);
    key(&mut v, KeyCode::End);
    assert_eq!(v.from, 0.5, "the window ends at the genome's end");
    key(&mut v, KeyCode::Home);
    key(&mut v, KeyCode::Left);
    assert_eq!(v.from, 0.0);
    assert_eq!(v.window(), "0–50% of the genes");
    for _ in 0..10 {
        key(&mut v, KeyCode::Char('+'));
    }
    assert_eq!(v.zoom, ZOOM_MAX);
    for _ in 0..10 {
        key(&mut v, KeyCode::Char('-'));
    }
    assert_eq!((v.zoom, v.from), (1.0, 0.0));
    assert!(!key(&mut v, KeyCode::Char('x')));
}

#[test]
fn brackets_go_a_chromosome_at_a_time_and_say_where_they_are() {
    let names: Vec<String> = vec![];
    let genes = [("1", 1000u64), ("2", 1000), ("3", 2000)]
        .iter()
        .map(|(c, len)| annotation::Gene {
            id: format!("G{c}"),
            name: format!("G{c}"),
            chrom: format!("chr{c}"),
            start: 1,
            end: *len,
        })
        .collect::<Vec<_>>();
    let mut v = GenomeView {
        genome: Some(Genome::place(Path::new("x.gtf"), &genes, &names)),
        ..GenomeView::default()
    };
    key(&mut v, KeyCode::Char(']'));
    assert_eq!((v.zoom, v.from), (4.0, 0.25));
    assert_eq!(v.window(), "chr 2 · 4× wide");
    key(&mut v, KeyCode::Char(']'));
    assert_eq!(v.from, 0.5);
    assert_eq!(v.window(), "chr 3 · 4× wide");
    key(&mut v, KeyCode::Char('['));
    key(&mut v, KeyCode::Char('['));
    assert_eq!(v.from, 0.0);
    key(&mut v, KeyCode::Char('-'));
    assert_eq!(
        v.window(),
        "chr 1–2 · 2× wide",
        "half the genome, from its start"
    );
}

#[test]
fn a_run_opened_starts_clean_of_the_last_one() {
    use crate::tui::results::tests::simulated_diff;
    let tmp = tempfile::tempdir().unwrap();
    let (_, diff) = simulated_diff(tmp.path());
    let mut r = Results::read(&diff, None).unwrap();
    // The last run's genes pinned, and its insets and genome on their way.
    let mut v = GenomeView {
        pinned: vec!["GENE1".into()],
        ..GenomeView::default()
    };
    v.reading = Some(figure::Reading::start(
        diff.clone(),
        vec![("GENE1".into(), "T1".into(), 0.01)],
        |_, _, _| 0.0,
    ));
    let (_placed, rx) = std::sync::mpsc::channel();
    v.placing = Some(rx);
    // The next run calls no genes: no insets of its own to read.
    r.genes.clear();
    v.opened(&r, None);
    v.tick(Some(&r));
    assert!(v.pinned.is_empty() && v.levels.is_empty());
    assert!(v.reading.is_none() && v.placing.is_none());
}

#[test]
fn insets_with_nothing_to_read_drop_the_read_on_its_way() {
    use crate::tui::results::tests::simulated_diff;
    let tmp = tempfile::tempdir().unwrap();
    let (_, diff) = simulated_diff(tmp.path());
    let mut r = Results::read(&diff, None).unwrap();
    let mut v = GenomeView {
        reading: Some(figure::Reading::start(
            diff.clone(),
            vec![("GENE1".into(), "T1".into(), 0.01)],
            |_, _, _| 0.0,
        )),
        ..GenomeView::default()
    };
    // Another level, whose genes have no strongest topic.
    r.genes.clear();
    v.read_insets(&r);
    assert!(
        v.reading.is_none(),
        "the old read would land on the new level"
    );
}
