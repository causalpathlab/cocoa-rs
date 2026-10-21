use super::*;
use clap::Parser;

#[test]
fn auroc_is_the_chance_a_causal_gene_ranks_above_a_null_one() {
    let perfect = [(3.0, true), (2.0, true), (1.0, false), (0.5, false)];
    assert_eq!(auroc(&perfect), Some(1.0));
    let ties = [(1.0, true), (1.0, false)];
    assert_eq!(auroc(&ties), Some(0.5));
    assert_eq!(auroc(&[(1.0, true)]), None);
}

#[test]
fn benjamini_hochberg_calls_and_scores_the_calls() {
    // 10 tests: p = 0.001, 0.002, 0.003 called at q = 0.1; one of them null.
    let mut p = vec![(0.001, true), (0.002, true), (0.003, false)];
    p.extend((0..7).map(|k| (0.5 + k as f64 * 0.05, k == 0)));
    let (k, power, fdr) = called_at(&p, 0.1, 3);
    assert_eq!(k, 3);
    assert!((power - 2.0 / 3.0).abs() < 1e-9);
    assert!((fdr - 1.0 / 3.0).abs() < 1e-9);
    assert_eq!(called_at(&[(0.9, true)], 0.1, 1), (0, 0.0, 0.0));
}

/// A small collider simulation and its diff with permutations, in `dir`:
/// the simulation's prefix and the diff's.
pub fn simulated_diff(dir: &Path) -> (PathBuf, PathBuf) {
    let sim = dir.join("collider");
    let diff = dir.join("collider-diff");
    let s = |p: &Path, ext: &str| format!("{}{ext}", p.display());
    let args = crate::run_sim_collider::SimColliderArgs::try_parse_from([
        "x",
        "-r",
        "120",
        "-c",
        "800",
        "-a",
        "20",
        "-t",
        "3",
        "--n-samples-per-exposure",
        "6",
        "-o",
        &s(&sim, ""),
    ])
    .unwrap();
    crate::run_sim_collider::run_sim_collider_data(args).unwrap();
    let args = crate::run_diff::DiffArgs::try_parse_from([
        "x",
        &s(&sim, ".zarr.zip"),
        "-i",
        &s(&sim, ".samples.gz"),
        "-e",
        &s(&sim, ".exposures.gz"),
        "-t",
        &s(&sim, ".celltypes.gz"),
        "--n-permutations",
        "20",
        "-o",
        &s(&diff, ""),
    ])
    .unwrap();
    crate::run_diff::run_cocoa_diff(args).unwrap();
    (sim, diff)
}

#[test]
fn a_finished_diff_reads_back_scored_against_its_simulation() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, diff) = simulated_diff(tmp.path());
    assert!(is_diff(&diff) && !is_diff(&sim));
    let truth = PathBuf::from(format!("{}.causal.gz", sim.display()));
    let mut r = Results::read(&diff, Some(&truth)).unwrap();
    assert_eq!(r.levels, [""]);
    assert_eq!(r.genes.len(), 120);
    assert!(r.has_perm());
    // 20 draws of a causal gene, some of them the same gene.
    let causal = r.truth.as_ref().unwrap().genes.len();
    assert!((10..=20).contains(&causal), "{causal}");

    // Sorted by |z|, untested genes last.
    let z: Vec<f32> = r.order.iter().map(|&g| r.genes[g].z[0]).collect();
    let tested: Vec<f32> = z.iter().copied().filter(|z| z.is_finite()).collect();
    assert!(tested.windows(2).all(|w| w[0].abs() >= w[1].abs()));
    assert!(z.iter().skip(tested.len()).all(|z| !z.is_finite()));
    r.next_sort();
    assert_eq!(r.sort, Sort::Contrast);

    let (adjusted, unadjusted) = r.score().unwrap();
    assert!(adjusted.auroc.is_some_and(|a| (0.0..=1.0).contains(&a)));
    assert!(adjusted.called.is_some());
    // Without covariates the run writes no unadjusted contrast.
    assert!(unadjusted.is_none() && !r.has_unadjusted());
    let (hist, sd) = r.null_z(16).unwrap();
    assert_eq!(hist.len(), 16);
    assert!(sd > 0.0);

    // The heatmap: genes leading each of the 3 topics.
    let h = r.heat.clone().expect("a heatmap");
    assert_eq!(h.topics.len(), 3);
    assert!(!h.genes.is_empty() && h.genes.len() <= 3 * PER_TOPIC);
    assert_eq!(h.v.len(), h.genes.len() * 3);
    r.per_topic_by(-4);
    assert!(r.heat.as_ref().unwrap().genes.len() <= 3);

    let pdf = tmp.path().join("heat.pdf");
    let page = legume_tui::export::PageSize {
        width_in: 5.0,
        dpi: 150,
    };
    r.save_heat(&pdf, page).unwrap();
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
}
