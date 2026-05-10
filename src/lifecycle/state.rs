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
