//! VM lifecycle states and the transition function that enforces
//! their canonical order.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Configured,
    Initializing,
    Booting,
    Running,
    Exiting,
    Terminated,
    Failed,
}

#[derive(Debug)]
pub(crate) struct IllegalTransition {
    pub from: State,
    pub to: State,
}

impl State {
    pub(crate) fn transition(self, to: State) -> Result<State, IllegalTransition> {
        use State::*;
        let ok = matches!(
            (self, to),
            (Configured, Initializing)
                | (Initializing, Booting)
                | (Booting, Running)
                | (Running, Exiting)
                | (Exiting, Terminated)
                | (_, Failed)
        );
        if ok { Ok(to) } else { Err(IllegalTransition { from: self, to }) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_path_through_states() {
        let s = State::Configured.transition(State::Initializing).unwrap();
        let s = s.transition(State::Booting).unwrap();
        let s = s.transition(State::Running).unwrap();
        let s = s.transition(State::Exiting).unwrap();
        let _ = s.transition(State::Terminated).unwrap();
    }

    #[test]
    fn failed_reachable_from_anywhere() {
        State::Configured.transition(State::Failed).unwrap();
        State::Booting.transition(State::Failed).unwrap();
        State::Running.transition(State::Failed).unwrap();
    }

    #[test]
    fn illegal_transitions_rejected() {
        assert!(State::Configured.transition(State::Running).is_err());
        assert!(State::Terminated.transition(State::Initializing).is_err());
        assert!(State::Running.transition(State::Booting).is_err());
    }
}
