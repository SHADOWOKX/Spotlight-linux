//! Temporary, bounded, in-memory desktop activation diagnostics.
//! No key events, queries, clipboard data, or token contents are retained.
use std::{cell::RefCell, collections::VecDeque, fmt::Write, rc::Rc, time::Instant};

const CAPACITY: usize = 256;

#[derive(Clone, Copy)]
pub enum ActivationEvent {
    Activated {
        timestamp: u128,
        has_token: bool,
    },
    Deactivated {
        timestamp: u128,
    },
    Window {
        event: &'static str,
        visible: bool,
        mapped: bool,
        active: bool,
        search_focused: bool,
    },
}

struct Entry {
    elapsed_us: u128,
    event: ActivationEvent,
}

struct State {
    started: Instant,
    entries: RefCell<VecDeque<Entry>>,
}

#[derive(Clone)]
pub struct ActivationTrace(Rc<State>);

impl Default for ActivationTrace {
    fn default() -> Self {
        Self(Rc::new(State {
            started: Instant::now(),
            entries: RefCell::new(VecDeque::with_capacity(CAPACITY)),
        }))
    }
}

impl ActivationTrace {
    pub fn record(&self, event: ActivationEvent) {
        let mut entries = self.0.entries.borrow_mut();
        if entries.len() == CAPACITY {
            entries.pop_front();
        }
        entries.push_back(Entry {
            elapsed_us: self.0.started.elapsed().as_micros(),
            event,
        });
    }

    pub fn snapshot(&self) -> String {
        let mut output = String::new();
        for entry in self.0.entries.borrow().iter() {
            let _ = write!(output, "activation_trace=+{}us ", entry.elapsed_us);
            match entry.event {
                ActivationEvent::Activated {
                    timestamp,
                    has_token,
                } => {
                    let _ = writeln!(
                        output,
                        "Activated portal_timestamp={timestamp} token_received={has_token}"
                    );
                }
                ActivationEvent::Deactivated { timestamp } => {
                    let _ = writeln!(
                        output,
                        "Deactivated portal_timestamp={timestamp} action=none"
                    );
                }
                ActivationEvent::Window {
                    event,
                    visible,
                    mapped,
                    active,
                    search_focused,
                } => {
                    let _ = writeln!(
                        output,
                        "{event} visible={visible} mapped={mapped} active={active} search_focused={search_focused}"
                    );
                }
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_trace_is_bounded_and_shared() {
        let trace = ActivationTrace::default();
        let other = trace.clone();
        for timestamp in 0..300 {
            other.record(ActivationEvent::Activated {
                timestamp,
                has_token: true,
            });
        }
        let snapshot = trace.snapshot();
        assert_eq!(snapshot.lines().count(), CAPACITY);
        assert!(!snapshot.contains("portal_timestamp=0 "));
        assert!(snapshot.contains("portal_timestamp=299 token_received=true"));
    }
}
