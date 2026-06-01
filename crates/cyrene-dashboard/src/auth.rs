//! Dashboard authentication (R26.7, R22).
//!
//! The dashboard requires authentication before granting access. A bearer
//! token is issued at startup (printed to the operator) and every request must
//! present it. Token comparison is constant-time to avoid timing attacks.

/// Guards dashboard access with a bearer token.
#[derive(Debug, Clone)]
pub struct DashboardAuth {
    token: String,
}

impl DashboardAuth {
    /// Creates an auth guard with the given bearer token.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }

    /// Returns `true` if the presented token matches (constant-time).
    #[must_use]
    pub fn authorize(&self, presented: &str) -> bool {
        constant_time_eq(self.token.as_bytes(), presented.as_bytes())
    }

    /// Extracts and validates a bearer token from an `Authorization` header
    /// value of the form `Bearer <token>`.
    #[must_use]
    pub fn authorize_header(&self, header: Option<&str>) -> bool {
        match header.and_then(|h| h.strip_prefix("Bearer ")) {
            Some(token) => self.authorize(token),
            None => false,
        }
    }
}

/// Constant-time byte comparison.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correct_token_is_authorized() {
        let auth = DashboardAuth::new("secret-token");
        assert!(auth.authorize("secret-token"));
    }

    #[test]
    fn wrong_token_is_rejected() {
        let auth = DashboardAuth::new("secret-token");
        assert!(!auth.authorize("wrong"));
        assert!(!auth.authorize(""));
    }

    #[test]
    fn bearer_header_is_parsed_and_validated() {
        let auth = DashboardAuth::new("abc123");
        assert!(auth.authorize_header(Some("Bearer abc123")));
        assert!(!auth.authorize_header(Some("Bearer wrong")));
        assert!(!auth.authorize_header(Some("abc123"))); // missing prefix
        assert!(!auth.authorize_header(None));
    }
}
