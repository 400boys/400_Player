//! The systems without an implementation yet: every call does nothing, so the rest of the program
//! does not care which system it runs on.

use super::{Command, HandOver, State, Waker};
use crate::cli::Cli;

/// Never created here: [`Service::start`] finds nothing to join.
#[allow(dead_code)]
pub struct Service;

#[allow(dead_code)]
impl Service {
    pub fn start() -> Option<Self> {
        None
    }

    pub fn set_waker(&mut self, _waker: Waker) {}

    pub fn poll(&mut self) -> Vec<Command> {
        Vec::new()
    }

    pub fn publish(&mut self, _state: State) {}
}

pub fn hand_over(_cli: &Cli) -> HandOver {
    HandOver::NoRunningWindow
}
