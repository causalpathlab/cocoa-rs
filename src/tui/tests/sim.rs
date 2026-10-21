use super::*;
use crate::runs::{cocoa_cli, plan};

#[test]
fn an_edge_steps_by_its_pve_within_zero_and_one() {
    assert_eq!(stepped("0.30", 1.0).as_deref(), Some("0.35"));
    assert_eq!(stepped("0.02", -1.0).as_deref(), Some("0.00"));
    assert_eq!(stepped("0.98", 1.0).as_deref(), Some("1.00"));
    assert_eq!(stepped("x", 1.0), None);
}

#[test]
fn a_size_steps_to_the_next_round_number() {
    assert_eq!(resized("2000", 1).as_deref(), Some("4000"));
    assert_eq!(resized("2000", -1).as_deref(), Some("1000"));
    assert_eq!(resized("5", 1).as_deref(), Some("6"));
    assert_eq!(resized("1", -1).as_deref(), Some("1"));
    assert_eq!(resized("10.0", 1).as_deref(), Some("11"));
}

#[test]
fn each_model_starts_runnable_and_its_rows_are_its_flags() {
    let app = cocoa_cli();
    let sim = Sim::new(&app, Model::Collider).unwrap();
    for model in [Model::One, Model::Collider] {
        let form = match model {
            Model::One => &sim.one,
            Model::Collider => &sim.collider,
        };
        for r in model.rows() {
            assert!(
                form.get(r.long).is_some(),
                "{} --{}",
                model.command(),
                r.long
            );
        }
        assert!(form.get("output").is_none());
        let mut argv = vec![model.command().to_string()];
        argv.extend(form.argv());
        let outputs = plan(&app, &argv, "/x/s").unwrap();
        assert!(
            outputs.iter().any(|o| o.ends_with("s.causal.gz")),
            "{outputs:?}"
        );
    }
}

#[test]
fn switching_models_keeps_the_row_both_have() {
    let app = cocoa_cli();
    let mut sim = Sim::new(&app, Model::Collider).unwrap();
    sim.at = Model::Collider
        .rows()
        .iter()
        .position(|r| r.long == "pve-exposure-gene")
        .unwrap();
    sim.nudge(1);
    assert_eq!(sim.form().get("pve-exposure-gene").unwrap().value, "0.35");
    sim.switch();
    assert_eq!(sim.model, Model::One);
    assert_eq!(sim.row().unwrap().0.long, "pve-exposure-gene");
    // Each model keeps its own values.
    assert_eq!(sim.form().get("pve-exposure-gene").unwrap().value, "0.3");
    assert_eq!(sim.individuals(), Some(10.0));
}
