use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Monotonically increasing identifier for a submitted query.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct QueryGeneration(pub u64);

#[derive(Clone, Debug)]
pub struct GenerationClock {
    current: Arc<AtomicU64>,
}

impl Default for GenerationClock {
    fn default() -> Self {
        Self::new()
    }
}

impl GenerationClock {
    pub fn new() -> Self {
        Self {
            current: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Begins a new query and invalidates every previously issued token.
    pub fn next(&self) -> CancellationToken {
        let generation = self.current.fetch_add(1, Ordering::AcqRel) + 1;
        CancellationToken {
            current: Arc::clone(&self.current),
            generation: QueryGeneration(generation),
        }
    }

    pub fn cancel_current(&self) {
        self.current.fetch_add(1, Ordering::AcqRel);
    }

    pub fn current(&self) -> QueryGeneration {
        QueryGeneration(self.current.load(Ordering::Acquire))
    }
}

/// Cheap cancellation token suitable for checking inside a tight provider loop.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    current: Arc<AtomicU64>,
    generation: QueryGeneration,
}

impl CancellationToken {
    pub fn generation(&self) -> QueryGeneration {
        self.generation
    }

    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.current.load(Ordering::Acquire) != self.generation.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_generation_cancels_older_token() {
        let clock = GenerationClock::new();
        let first = clock.next();
        assert!(!first.is_cancelled());
        let second = clock.next();
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());
        assert!(second.generation() > first.generation());
    }
}
