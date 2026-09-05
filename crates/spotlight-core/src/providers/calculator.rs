//! Bounded, local arithmetic. Never invokes a shell, interpreter or network.
use crate::provider::{ProviderClass, ProviderDescriptor, ProviderError};
use crate::{Action, CancellationToken, Icon, Provider, SearchQuery, SearchResult};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};

pub struct CalculatorProvider {
    descriptor: ProviderDescriptor,
    enabled: AtomicBool,
}

impl Default for CalculatorProvider {
    fn default() -> Self {
        Self {
            descriptor: ProviderDescriptor {
                id: "calculator".into(),
                display_name: "Calculator".into(),
                class: ProviderClass::Instant,
                default_priority: 100,
            },
            enabled: AtomicBool::new(true),
        }
    }
}
impl CalculatorProvider {
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }
}
impl Provider for CalculatorProvider {
    fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }
    fn search(
        &self,
        query: &SearchQuery,
        cancellation: &CancellationToken,
    ) -> Result<Vec<SearchResult>, ProviderError> {
        if cancellation.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        if !self.enabled.load(Ordering::Relaxed) {
            return Ok(vec![]);
        }
        let Some(value) = evaluate(query.normalized_text()) else {
            return Ok(vec![]);
        };
        let text = if value == 0.0 {
            "0".into()
        } else {
            value.to_string()
        };
        Ok(vec![SearchResult {
            id: "calculator:result".into(),
            title: text.clone(),
            subtitle: Some(format!("{} · Enter to copy", query.normalized_text())),
            icon: Icon::Themed("accessories-calculator-symbolic".into()),
            provider: "calculator".into(),
            score: 1_000_000,
            primary_action: Action::CopyText { text },
            secondary_actions: vec![],
            keywords: vec![],
            metadata: BTreeMap::new(),
        }])
    }
}

/// IEEE-754 arithmetic, not financial decimal arithmetic. Incomplete input,
/// excessive nesting, non-finite results and unsupported syntax yield no result.
pub fn evaluate(text: &str) -> Option<f64> {
    if text.len() > 256 || !text.is_ascii() {
        return None;
    }
    let text = text.trim().strip_prefix('=').unwrap_or(text).trim();
    if text.is_empty() || !text.contains(['+', '-', '*', '/', '^', '%', '(']) {
        return None;
    }
    let mut parser = Parser {
        input: text.as_bytes(),
        pos: 0,
    };
    let result = parser.expression(0, 0)?;
    parser.space();
    (parser.pos == parser.input.len() && result.is_finite()).then_some(result)
}
struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}
impl Parser<'_> {
    fn space(&mut self) {
        while self
            .input
            .get(self.pos)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.pos += 1;
        }
    }
    fn take(&mut self, byte: u8) -> bool {
        self.space();
        if self.input.get(self.pos) == Some(&byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expression(&mut self, minimum: u8, depth: usize) -> Option<f64> {
        if depth > 32 {
            return None;
        }
        self.space();
        let mut left = if self.take(b'-') {
            -self.expression(3, depth + 1)?
        } else if self.take(b'+') {
            self.expression(3, depth + 1)?
        } else if self.take(b'(') {
            let value = self.expression(0, depth + 1)?;
            if !self.take(b')') {
                return None;
            }
            value
        } else if self
            .input
            .get(self.pos)
            .is_some_and(u8::is_ascii_alphabetic)
        {
            let start = self.pos;
            while self
                .input
                .get(self.pos)
                .is_some_and(u8::is_ascii_alphabetic)
            {
                self.pos += 1;
            }
            let name = &self.input[start..self.pos];
            if !self.take(b'(') {
                return None;
            }
            let value = self.expression(0, depth + 1)?;
            if !self.take(b')') {
                return None;
            }
            match name {
                b"sqrt" => value.sqrt(),
                b"abs" => value.abs(),
                b"round" => value.round(),
                _ => return None,
            }
        } else {
            let start = self.pos;
            while self
                .input
                .get(self.pos)
                .is_some_and(|b| b.is_ascii_digit() || *b == b'.')
            {
                self.pos += 1;
            }
            std::str::from_utf8(&self.input[start..self.pos])
                .ok()?
                .parse::<f64>()
                .ok()?
        };
        loop {
            self.space();
            if self.input.get(self.pos) == Some(&b'%') {
                self.pos += 1;
                left /= 100.0;
                continue;
            }
            let remaining = &self.input[self.pos..];
            let (operator, precedence, length) = match remaining.first() {
                Some(b'+') => (b'+', 1, 1),
                Some(b'-') => (b'-', 1, 1),
                Some(b'*') => (b'*', 2, 1),
                Some(b'/') => (b'/', 2, 1),
                Some(b'^') => (b'^', 3, 1),
                _ if remaining.starts_with(b"of ") => (b'*', 2, 2),
                _ => break,
            };
            if precedence < minimum {
                break;
            }
            self.pos += length;
            let right = self.expression(
                if operator == b'^' {
                    precedence
                } else {
                    precedence + 1
                },
                depth + 1,
            )?;
            left = match operator {
                b'+' => left + right,
                b'-' => left - right,
                b'*' => left * right,
                b'/' => left / right,
                b'^' => left.powf(right),
                _ => return None,
            };
            if !left.is_finite() {
                return None;
            }
        }
        left.is_finite().then_some(left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arithmetic_and_percent() {
        for (query, result) in [
            ("2+2", 4.0),
            ("sqrt(144)", 12.0),
            ("15% of 850", 127.5),
            ("2+3*4", 14.0),
            ("(2+3)*4", 20.0),
            ("2^3^2", 512.0),
            ("-2^2", -4.0),
            ("abs(-4)", 4.0),
            ("round(1.6)", 2.0),
        ] {
            assert_eq!(evaluate(query), Some(result), "{query}");
        }
    }
    #[test]
    fn rejects_invalid_and_unbounded_input() {
        for query in [
            "terminal", "42", "1/0", "sqrt(-1)", "2+", "1;whoami", "$(id)", "2**4", "2+NaN",
            "2^99999",
        ] {
            assert_eq!(evaluate(query), None, "{query}");
        }
        assert_eq!(
            evaluate(&format!("{}1{}", "(".repeat(40), ")".repeat(40))),
            None
        );
        assert_eq!(evaluate(&"1+".repeat(200)), None);
    }
    #[test]
    fn disabled_and_cancelled() {
        let clock = crate::GenerationClock::new();
        let token = clock.next();
        let query = SearchQuery::new(token.generation(), "2+2", 8);
        let provider = CalculatorProvider::default();
        assert_eq!(provider.search(&query, &token).unwrap().len(), 1);
        provider.set_enabled(false);
        assert!(provider.search(&query, &token).unwrap().is_empty());
        clock.cancel_current();
        assert!(provider.search(&query, &token).is_err());
    }
}
