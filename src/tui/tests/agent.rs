use super::*;

#[test]
fn a_table_counts_as_written_once_its_footer_is() {
    use legume_numeric::matrix::parquet::{write_named_table, Column};
    let tmp = tempfile::tempdir().unwrap();
    let table = tmp.path().join("run.perm.parquet");
    let x = [1.0f32];
    write_named_table(
        &table.to_string_lossy(),
        "gene",
        &["GENE1".into()],
        &[("x".into(), Column::F32(&x))],
    )
    .unwrap();
    assert!(written(&table));
    // Still being written: the footer is not there yet.
    let whole = std::fs::read(&table).unwrap();
    std::fs::write(&table, &whole[..whole.len() - 8]).unwrap();
    assert!(!written(&table));
    assert!(!written(&tmp.path().join("none.parquet")));
    let other = tmp.path().join("run.cmd.sh");
    std::fs::write(&other, "cocoa diff\n").unwrap();
    assert!(written(&other));
}
