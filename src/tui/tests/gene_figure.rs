use super::*;
use crate::tui::annotation::Gene;
use crate::tui::results::tests::simulated_diff;
use crate::tui::results::Results;

#[test]
fn the_page_names_the_gene_and_draws_every_panel() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, diff) = simulated_diff(tmp.path());
    let truth = std::path::PathBuf::from(format!("{}.causal.gz", sim.display()));
    let r = Results::read(&diff, Some(&truth)).unwrap();
    let g = r.called()[0];
    let name = r.genes[g].name.clone();
    // Every gene on one chromosome, 10 kb apart: the locus has neighbours.
    let genes: Vec<Gene> = r
        .genes
        .iter()
        .enumerate()
        .map(|(k, x)| Gene {
            id: x.name.clone(),
            name: x.name.clone(),
            chrom: "chr1".into(),
            start: 10_000 * (k as u64 + 1),
            end: 10_000 * (k as u64 + 1) + 500,
        })
        .collect();
    let names: Vec<String> = r.genes.iter().map(|x| x.name.clone()).collect();
    let genome = Genome::place(std::path::Path::new("t.gtf"), &genes, &names);
    let wanted: Vec<(String, String, f32)> = r
        .topics()
        .iter()
        .map(|t| (name.clone(), t.clone(), f32::NAN))
        .collect();
    let (facets, levels) =
        super::super::figure::read_insets(&diff, &wanted, |_, _, _| 0.0, &Default::default())
            .unwrap();
    let fig = GeneFigure {
        results: &r,
        genome: Some(&genome),
        gene: g,
        facets: &facets,
        levels: &levels,
        flank: 200_000,
    };
    let head = fig.headline();
    assert!(head.starts_with(&format!("{name} · chr1:")), "{head}");
    assert!(head.contains(" · z ") && head.contains(" · p "), "{head}");

    let mut frame = Frame::blank(1400, 900, THEME.background);
    let font = Font::for_cell_height(16.0, false);
    let bold = Font::for_cell_height(16.0, true);
    fig.draw(&mut frame, font, bold);
    let img = frame.to_image();
    let bg = THEME.background;
    let inked = |x0: u32, x1: u32, y0: u32, y1: u32| {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| img.get_pixel(x, y).0[..3] != bg)
            .count()
    };
    assert!(inked(0, 500, 40, 360) > 1500, "effect by cell type");
    assert!(inked(560, 1400, 40, 360) > 1500, "the locus");
    assert!(inked(0, 1400, 380, 900) > 4000, "individuals by cell type");
}
