//! The `diff` command line carries no ablation switches for its own method:
//! topic residualization, the likelihood pseudobulk refine, and raw counts
//! into the model are the method, not options.

use super::*;

fn parse(extra: &[&str]) -> Result<DiffArgs, clap::Error> {
    let required = [
        "cocoa-diff",
        "data.zarr",
        "-i",
        "indv.gz",
        "-e",
        "exposure.gz",
        "-o",
        "out",
    ];
    DiffArgs::try_parse_from(required.iter().chain(extra).copied())
}

#[test]
fn required_arguments_parse() {
    parse(&[]).expect("minimal diff invocation parses");
}

#[test]
fn removed_switches_are_rejected() {
    for flag in [
        "--no-refine",
        "--pb-refine-sweeps",
        "--no-residualize-topics",
        "--no-adjust-housekeeping",
    ] {
        assert!(parse(&[flag]).is_err(), "{flag} should not parse");
    }
}

#[test]
fn deltas_go_a_few_whole_genes_a_row_group_in_name_order() {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    use parquet::file::statistics::Statistics;
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("d.delta.parquet");
    let genes: Vec<Box<str>> = ["GENE3", "GENE1", "GENE2"].map(Into::into).to_vec();
    let (indv, indv_exp): (Vec<Box<str>>, Vec<Box<str>>) = (
        vec!["I1".into(), "I2".into()],
        vec!["I1_L0".into(), "I2_L1".into()],
    );
    let topics: Vec<Box<str>> = vec!["T1".into(), "T2".into()];
    // Topic T2 has only I2.
    let effect = |used: Vec<bool>| ExposureEffect {
        psi: Mat::zeros(3, 2),
        tau: Mat::zeros(3, 2),
        delta: Mat::from_fn(3, 2, |d, i| (10 * d + i) as f32),
        used,
        iterations: 0,
    };
    let effects = [effect(vec![true, true]), effect(vec![false, true])];
    let y = Mat::from_element(3, 2, 1.0);
    let counts = [(&y, &y), (&y, &y)];
    // Room for one gene's three rows a group.
    write_deltas(
        &effects,
        &counts,
        &genes,
        &topics,
        (&indv, &indv_exp),
        &file.to_string_lossy(),
        3,
    )
    .unwrap();
    let r = SerializedFileReader::new(std::fs::File::open(&file).unwrap()).unwrap();
    let groups: Vec<(String, String, i64)> = r
        .metadata()
        .row_groups()
        .iter()
        .map(|g| match g.column(0).statistics() {
            Some(Statistics::ByteArray(s)) => (
                s.min_opt().unwrap().as_utf8().unwrap().to_string(),
                s.max_opt().unwrap().as_utf8().unwrap().to_string(),
                g.num_rows(),
            ),
            other => panic!("{other:?}"),
        })
        .collect();
    let one = |g: &str| (g.to_string(), g.to_string(), 3);
    assert_eq!(groups, [one("GENE1"), one("GENE2"), one("GENE3")]);
    // Every row there, with the gene's own delta.
    let t = legume_numeric::matrix::table::TableReader::open(&file.to_string_lossy()).unwrap();
    let rows: Vec<Vec<Box<str>>> = t.rows(&[0, 1, 3, 4]).unwrap().map(Result::unwrap).collect();
    let row = |g: &str, i: &str, t: &str| {
        rows.iter()
            .find(|r| (&*r[0], &*r[1], &*r[2]) == (g, i, t))
            .map(|r| r[3].to_string())
    };
    assert_eq!(rows.len(), 9);
    assert_eq!(row("GENE3", "I2", "T1").as_deref(), Some("1"));
    assert_eq!(row("GENE1", "I2", "T2").as_deref(), Some("11"));
    assert_eq!(row("GENE1", "I1", "T2"), None);
}
