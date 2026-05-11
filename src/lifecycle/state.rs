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

pub(crate) fn transition(from: State, to: State) -> Result<State, IllegalTransition> {
    use State::*;
    let ok = matches!(
        (from, to),
        (Configured, Initializing)
            | (Initializing, Booting)
            | (Booting, Running)
            | (Running, Exiting)
            | (Exiting, Terminated)
            | (_, Failed)
    );
    if ok {
        Ok(to)
    } else {
        Err(IllegalTransition { from, to })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
