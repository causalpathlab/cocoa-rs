use super::*;

fn view(words: &str, terminal: bool) -> bool {
    let argv: Vec<String> = ["cocoa"]
        .into_iter()
        .chain(words.split(' '))
        .map(String::from)
        .collect();
    view_matches(&argv, terminal).unwrap().is_some()
}

const FULL: &str = "diff d.zarr -i s.gz -e x.gz -o out";

#[test]
fn a_complete_diff_runs_straight_through_as_scripts_expect() {
    assert!(!view(FULL, true));
    assert!(!view(&format!("{FULL} --batch-process"), true));
}

#[test]
fn a_diff_set_up_in_part_or_asking_for_the_view_opens_it() {
    assert!(view("diff", true));
    assert!(view("diff d.zarr -o out", true));
    assert!(view(&format!("{FULL} --results out"), true));
    assert!(view(&format!("{FULL} --watch"), true));
}

#[test]
fn without_a_terminal_nothing_opens_a_view() {
    assert!(!view("diff d.zarr -o out", false));
    assert!(!view(FULL, false));
}

#[test]
fn asked_for_the_view_without_a_terminal_says_so_and_runs_nothing() {
    for asked in ["--results out", "--watch", "--job"] {
        let argv: Vec<String> = format!("cocoa {FULL} {asked}")
            .split(' ')
            .map(String::from)
            .collect();
        let Err(e) = view_matches(&argv, false) else {
            panic!("{asked}: no terminal, yet no complaint");
        };
        assert!(e.contains("terminal"), "{asked}: {e}");
    }
    let partial: Vec<String> = ["cocoa", "diff", "--results", "out"]
        .map(String::from)
        .to_vec();
    assert!(view_matches(&partial, false).is_err());
}

#[test]
fn a_session_job_runs_in_the_view() {
    assert!(view(&format!("{FULL} --job"), true));
}
