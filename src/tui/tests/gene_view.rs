use super::*;
use crate::tui::results::tests::simulated_diff;
use ratatui::crossterm::event::KeyModifiers;

fn key(c: KeyCode) -> KeyEvent {
    KeyEvent::new(c, KeyModifiers::NONE)
}

/// Wait for the page's individuals.
fn settle(v: &mut GeneView) {
    for _ in 0..500 {
        if v.tick() || !v.busy() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the individuals never came");
}

#[test]
fn a_gene_s_page_reads_it_in_every_topic_and_steps_through_the_table() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, diff) = simulated_diff(tmp.path());
    let r = Results::read(&diff, None).unwrap();
    let first = r.order[0];
    let mut v = GeneView::open(&r, first);
    settle(&mut v);
    assert_eq!(v.facets.len(), r.topics().len(), "a facet per topic");
    assert!(v.facets.iter().all(|f| f.gene == r.genes[first].name));
    assert_eq!(v.levels, ["0", "1"]);

    // n and N follow the table's order, and stop at its ends.
    assert_eq!(v.key(key(KeyCode::Char('n')), &r), Took::Consumed);
    assert_eq!(v.gene, r.order[1]);
    settle(&mut v);
    v.key(key(KeyCode::Char('N')), &r);
    v.key(key(KeyCode::Up), &r);
    assert_eq!(v.gene, first);
    // + and - narrow and widen the locus within their bounds.
    for _ in 0..20 {
        v.key(key(KeyCode::Char('+')), &r);
    }
    assert_eq!(v.flank, FLANK_MIN);
    for _ in 0..20 {
        v.key(key(KeyCode::Char('-')), &r);
    }
    assert_eq!(v.flank, FLANK_MAX);
    assert_eq!(v.key(key(KeyCode::Esc), &r), Took::Close);
    assert_eq!(v.key(key(KeyCode::Char('x')), &r), Took::Ignored);

    // A gene by its whole name, or its symbol, any case.
    let name = r.genes[5].name.clone();
    assert_eq!(GeneView::find(&r, &name), Some(5));
    assert_eq!(GeneView::find(&r, "no-such-gene"), None);
}
