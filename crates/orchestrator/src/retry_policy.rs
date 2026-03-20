#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
}

impl RetryPolicy {
    pub const fn new(max_attempts: u32) -> Self {
        Self { max_attempts }
    }

    pub const fn should_retry(&self, current_attempt: u32) -> bool {
        current_attempt < self.max_attempts
    }
}

#[cfg(test)]
mod tests {
    use super::RetryPolicy;

    #[test]
    fn respects_attempt_limit() {
        let policy = RetryPolicy::new(2);

        assert!(policy.should_retry(0));
        assert!(policy.should_retry(1));
        assert!(!policy.should_retry(2));
    }
}
