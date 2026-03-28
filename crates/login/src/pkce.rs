/// PKCE codes are no longer generated; this struct is retained for type compatibility.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PkceCodes {
    pub code_verifier: String,
    pub code_challenge: String,
}
