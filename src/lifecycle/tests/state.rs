use crate::lifecycle::{state::transition, State};

#[test]
fn legal_path_through_states() {
    let s = transition(State::Configured, State::Initializing).unwrap();
    let s = transition(s, State::Booting).unwrap();
    let s = transition(s, State::Running).unwrap();
    let s = transition(s, State::Exiting).unwrap();
    let _ = transition(s, State::Terminated).unwrap();
}

#[test]
fn failed_reachable_from_anywhere() {
    transition(State::Configured, State::Failed).unwrap();
    transition(State::Booting, State::Failed).unwrap();
    transition(State::Running, State::Failed).unwrap();
}

#[test]
fn illegal_transitions_rejected() {
    assert!(transition(State::Configured, State::Running).is_err());
    assert!(transition(State::Terminated, State::Initializing).is_err());
    assert!(transition(State::Running, State::Booting).is_err());
}
