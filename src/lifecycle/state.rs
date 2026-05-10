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

pub(crate) struct IllegalTransition {
    pub from: State,
    pub to: State,
}

pub(crate) fn transition(from: State, to: State) -> Result<State, IllegalTransition> {
    unimplemented!()
}
