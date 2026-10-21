use super::*;
use crate::tui::results::tests::simulated_diff;
use crate::tui::results::Results;

#[test]
fn p_values_print_as_the_papers_print_them() {
    assert_eq!(p_text(7.4e-7), "7.4e-07");
    assert_eq!(p_text(3.2e-12), "3.2e-12");
    assert_eq!(p_text(0.0123), "0.012");
    assert_eq!(p_text(f32::NAN), "");
}

#[test]
fn ticks_are_round_numbers_across_the_range() {
    assert_eq!(ticks(-0.5, 0.5, 4), [-0.5, -0.25, 0.0, 0.25, 0.5]);
    assert_eq!(ticks(-1.1, 0.3, 3), [-1.0, -0.5, 0.0]);
    assert_eq!(tick_text(0.25), "0.25");
    assert_eq!(tick_text(-1.0), "-1");
}

#[test]
fn insets_hold_each_individual_shifted_by_its_level_s_effect() {
    let tmp = tempfile::tempdir().unwrap();
    let (_, diff) = simulated_diff(tmp.path());
    let r = Results::read(&diff, None).unwrap();
    let g = r.genes[r.called()[0]].name.clone();
    let t = r.strongest_topic(&g).unwrap();
    let psi = r.effect_at(&g, &t, 0);
    assert!(psi.is_finite());
    let wanted = vec![(g.clone(), t.clone(), 0.01)];
    let (insets, levels) = read_insets(
        &diff,
        &wanted,
        |_, _, l| if l == 1 { psi } else { 0.0 },
        &Default::default(),
    )
    .unwrap();
    assert_eq!(levels, ["0", "1"]);
    let inset = &insets[0];
    assert_eq!(
        (inset.gene.as_str(), inset.topic.as_str()),
        (g.as_str(), t.as_str())
    );
    assert!(inset.points.len() >= 6, "{}", inset.points.len());
    assert!(inset.points.iter().any(|p| p.level == 0) && inset.points.iter().any(|p| p.level == 1));
    // The counts are there: each point has its spread.
    assert!(inset
        .points
        .iter()
        .all(|p| p.count >= 0.0 && p.sd > 0.0 && p.sd <= 1.0));
    // The exposed sit higher by the effect, on average, for a gene called up.
    let mean = |l: usize| {
        let v: Vec<f32> = inset
            .points
            .iter()
            .filter(|p| p.level == l)
            .map(|p| p.y)
            .collect();
        v.iter().sum::<f32>() / v.len() as f32
    };
    assert!(
        (mean(1) - mean(0)).signum() == psi.signum(),
        "{} {} {psi}",
        mean(0),
        mean(1)
    );

    // The figure draws on both panels.
    let mut frame = Frame::blank(1200, 800, THEME.background);
    let font = Font::for_cell_height(16.0, false);
    let bold = Font::for_cell_height(16.0, true);
    let fig = Figure {
        results: &r,
        genome: None,
        insets: &insets,
        levels: &levels,
        from: 0.0,
        span: 1.0,
    };
    fig.draw(&mut frame, font, bold);
    let img = frame.to_image();
    let bg = THEME.background;
    let inked = |y0: u32, y1: u32| {
        (y0..y1)
            .flat_map(|y| (0..img.width()).map(move |x| (x, y)))
            .filter(|&(x, y)| img.get_pixel(x, y).0[..3] != bg)
            .count()
    };
    assert!(inked(40, 480) > 2000, "the genome panel");
    assert!(inked(520, 800) > 1000, "the insets");
}

/// A diff's effect and delta tables, by hand: `levels` in the run's order
/// (the reference first) and the individuals' exposures.
fn tables(dir: &std::path::Path, levels: &[&str], indv: &[(&str, &str)]) -> std::path::PathBuf {
    use legume_numeric::matrix::parquet::{write_named_table, Column};
    let prefix = dir.join("hand");
    let b = |s: &str| -> Box<str> { s.into() };
    let (topic, tau) = ([b("T1")], [0.0f32]);
    let columns: Vec<(Box<str>, Column)> = std::iter::once((b("topic"), Column::Str(&topic)))
        .chain(
            levels
                .iter()
                .map(|l| (b(&format!("tau_{l}")), Column::F32(&tau))),
        )
        .collect();
    let effect = super::super::results::table(&prefix, "effect");
    write_named_table(&effect.to_string_lossy(), "gene", &[b("GENE1")], &columns).unwrap();
    let n = indv.len();
    let ids: Vec<Box<str>> = indv.iter().map(|(i, _)| b(i)).collect();
    let exp: Vec<Box<str>> = indv.iter().map(|(i, e)| b(&format!("{i}_{e}"))).collect();
    let topics = vec![b("T1"); n];
    let delta = vec![1.0f32; n];
    let delta_t = super::super::results::table(&prefix, "delta");
    write_named_table(
        &delta_t.to_string_lossy(),
        "gene",
        &vec![b("GENE1"); n],
        &[
            (b("individual"), Column::Str(&ids)),
            (b("individual_exposure"), Column::Str(&exp)),
            (b("topic"), Column::Str(&topics)),
            (b("delta"), Column::F32(&delta)),
        ],
    )
    .unwrap();
    prefix
}

#[test]
fn insets_number_the_levels_as_the_run_does() {
    let tmp = tempfile::tempdir().unwrap();
    // The run's reference L0 has no individuals here; L2 sorts after L1 but
    // comes first after the reference.
    let prefix = tables(
        tmp.path(),
        &["L0", "L2", "L1"],
        &[("I1", "L1"), ("I2", "L2")],
    );
    let wanted = vec![("GENE1".to_string(), "T1".to_string(), 0.01)];
    let psi = |_: &str, _: &str, l: usize| l as f32 * 10.0;
    let (insets, levels) = read_insets(&prefix, &wanted, psi, &Default::default()).unwrap();
    assert_eq!(levels, ["L0", "L2", "L1"]);
    let at: Vec<(usize, f32)> = insets[0].points.iter().map(|p| (p.level, p.y)).collect();
    // I1 at L1 (level 2), I2 at L2 (level 1), each shifted by its own effect.
    assert_eq!(at, [(2, 20.0), (1, 10.0)]);
}

#[test]
fn a_read_stopped_ends_without_reading_the_table() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tables(tmp.path(), &["L0", "L1"], &[("I1", "L1"), ("I2", "L0")]);
    let wanted = vec![("GENE1".to_string(), "T1".to_string(), 0.01)];
    let stop = std::sync::atomic::AtomicBool::new(true);
    let read = read_insets(&prefix, &wanted, |_, _, _| 0.0, &stop);
    assert!(read.is_err());
    stop.store(false, std::sync::atomic::Ordering::Relaxed);
    assert!(read_insets(&prefix, &wanted, |_, _, _| 0.0, &stop).is_ok());
}

#[test]
fn a_gene_is_read_from_the_row_groups_that_hold_it() {
    use crate::common::Mat;
    use crate::stat::propensity_effect::ExposureEffect;
    use legume_numeric::matrix::parquet::{write_named_table, Column};
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join("grouped");
    let b = |s: &str| -> Box<str> { s.into() };
    let (topic, tau) = ([b("T1")], [0.0f32]);
    write_named_table(
        &super::super::results::table(&prefix, "effect").to_string_lossy(),
        "gene",
        &[b("GENE1")],
        &[
            (b("topic"), Column::Str(&topic)),
            (b("tau_L0"), Column::F32(&tau)),
            (b("tau_L1"), Column::F32(&tau)),
        ],
    )
    .unwrap();
    let genes: Vec<Box<str>> = ["GENE1", "GENE2", "GENE3"].map(b).to_vec();
    let effect = ExposureEffect {
        psi: Mat::zeros(3, 2),
        tau: Mat::zeros(3, 2),
        delta: Mat::from_fn(3, 2, |d, i| (d + 1) as f32 * if i == 0 { 1.0 } else { 2.0 }),
        used: vec![true, true],
        iterations: 0,
    };
    let y = Mat::from_element(3, 2, 4.0);
    let delta = super::super::results::table(&prefix, "delta");
    crate::run_diff::write_deltas(
        &[effect],
        &[(&y, &y)],
        &genes,
        &[b("T1")],
        (&[b("I1"), b("I2")], &[b("I1_L0"), b("I2_L1")]),
        &delta.to_string_lossy(),
        2,
    )
    .unwrap();
    let wanted: std::collections::HashSet<&str> = ["GENE2"].into();
    assert_eq!(groups_in(&delta, &wanted), [1]);
    // Two genes a group: a gene at either end of one, or between, is in it.
    let pairs = tmp.path().join("pairs.delta.parquet");
    crate::run_diff::write_deltas(
        &[ExposureEffect {
            psi: Mat::zeros(3, 2),
            tau: Mat::zeros(3, 2),
            delta: Mat::from_element(3, 2, 1.0),
            used: vec![true, true],
            iterations: 0,
        }],
        &[(&y, &y)],
        &genes,
        &[b("T1")],
        (&[b("I1"), b("I2")], &[b("I1_L0"), b("I2_L1")]),
        &pairs.to_string_lossy(),
        4,
    )
    .unwrap();
    for (gene, groups) in [
        ("GENE1", vec![0]),
        ("GENE2", vec![0]),
        ("GENE3", vec![1]),
        ("GENE9", vec![]),
    ] {
        let one: std::collections::HashSet<&str> = [gene].into();
        assert_eq!(groups_in(&pairs, &one), groups, "{gene}");
    }
    let want = vec![("GENE2".to_string(), "T1".to_string(), 0.01)];
    let (insets, _) = read_insets(&prefix, &want, |_, _, _| 0.0, &Default::default()).unwrap();
    let at: Vec<(usize, f32)> = insets[0].points.iter().map(|p| (p.level, p.y)).collect();
    assert_eq!(at, [(0, 2.0f32.ln()), (1, 4.0f32.ln())]);
}

/// The row groups of the delta table `file` that may hold one of `genes`.
fn groups_in(file: &std::path::Path, genes: &std::collections::HashSet<&str>) -> Vec<usize> {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    let reader = SerializedFileReader::new(std::fs::File::open(file).unwrap()).unwrap();
    groups_holding(reader.metadata(), genes)
}
