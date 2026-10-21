use super::*;

#[test]
fn a_files_name_suggests_its_role() {
    for (name, role) in [
        ("sim.zarr.zip", Role::Data),
        ("cells.h5", Role::Data),
        ("sim.samples.gz", Role::Individuals),
        ("sim.exposures.gz", Role::Exposure),
        ("sim.celltypes.gz", Role::Topics),
        ("theta_prop.tsv.gz", Role::Proportions),
        ("sim.causal.gz", Role::Truth),
        ("sim.conf_V.tsv.gz", Role::Covariates),
        ("neg_controls.txt", Role::Controls),
    ] {
        assert_eq!(Role::guess(name), role, "{name}");
    }
}

#[test]
fn space_gives_the_guess_then_each_role_after_it_then_none() {
    let tmp = tempfile::tempdir().unwrap();
    let mut i = Inputs::new(tmp.path().to_path_buf());
    let f = tmp.path().join("a.causal.gz");
    i.cycle(f.clone());
    assert_eq!(i.role_of(&f), Some(Role::Truth));
    i.cycle(f.clone());
    assert_eq!(i.role_of(&f), None);
    // One exposure file: a second takes the role from the first.
    let (e1, e2) = (tmp.path().join("e1"), tmp.path().join("e2"));
    i.set(e1.clone(), Some(Role::Exposure));
    i.set(e2.clone(), Some(Role::Exposure));
    assert_eq!(i.files(Role::Exposure), [&e2]);
    // Several data files stay.
    i.set(tmp.path().join("d1.zarr"), Some(Role::Data));
    i.set(tmp.path().join("d2.zarr"), Some(Role::Data));
    assert_eq!(i.files(Role::Data).len(), 2);
    assert_eq!(i.missing(), ["individuals"]);
}

#[test]
fn the_inputs_are_named_from_where_the_run_goes() {
    let dir = Path::new("/w/sim");
    let picked = vec![
        (dir.join("s.zarr.zip"), Role::Data),
        (dir.join("s.samples.gz"), Role::Individuals),
        (PathBuf::from("/elsewhere/e.gz"), Role::Exposure),
        (dir.join("s.causal.gz"), Role::Truth),
    ];
    assert_eq!(
        argv(&picked, Some(dir)),
        [
            "s.zarr.zip",
            "--indv-files",
            "s.samples.gz",
            "--exposure-assignment-file",
            "/elsewhere/e.gz"
        ]
    );
    assert_eq!(argv(&picked, None)[0], "/w/sim/s.zarr.zip");
}

#[test]
fn checks_say_how_the_individuals_and_exposures_fit() {
    let tmp = tempfile::tempdir().unwrap();
    let indv = tmp.path().join("indv.txt");
    let exp = tmp.path().join("exp.txt");
    std::fs::write(&indv, "a\na\nb\nc\n").unwrap();
    std::fs::write(&exp, "a\t0\nb\t1\n").unwrap();
    let c = checks(&[indv], Some(&exp));
    assert_eq!(c[0], "✓ 4 cells of 3 individuals");
    assert_eq!(c[1], "✓ exposure levels 0: 1, 1: 1");
    assert_eq!(c[2], "✗ 1 individuals have no exposure: c");
}

#[test]
fn checks_split_lines_as_cocoa_reads_them() {
    let tmp = tempfile::tempdir().unwrap();
    let indv = tmp.path().join("indv.csv");
    let exp = tmp.path().join("exp.csv");
    // Cell and individual pairs, with a column after; exposures by comma.
    std::fs::write(&indv, "c1,I1,x\nc2,I1,x\nc3,I2,y\n").unwrap();
    std::fs::write(&exp, "I1,L0\nI2,L1\n").unwrap();
    let c = checks(&[indv], Some(&exp));
    assert_eq!(
        c,
        [
            "✓ 3 cells of 2 individuals",
            "✓ exposure levels L0: 1, L1: 1",
            "✓ every individual has an exposure",
        ]
    );
}
