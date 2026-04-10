use nexal_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde::Serialize;
use std::any::Any;
use std::fmt::Debug;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

// ── Error types ──

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPosition {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorLocation {
    pub path: String,
    pub range: TextRange,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid decision: {0}")]
    InvalidDecision(String),
    #[error("invalid pattern element: {0}")]
    InvalidPattern(String),
    #[error("invalid example: {0}")]
    InvalidExample(String),
    #[error("invalid rule: {0}")]
    InvalidRule(String),
    #[error(
        "expected every example to match at least one rule. rules: {rules:?}; unmatched examples: {examples:?}"
    )]
    ExampleDidNotMatch {
        rules: Vec<String>,
        examples: Vec<String>,
        location: Option<ErrorLocation>,
    },
    #[error("expected example to not match rule `{rule}`: {example}")]
    ExampleDidMatch {
        rule: String,
        example: String,
        location: Option<ErrorLocation>,
    },
}

impl Error {
    pub fn with_location(self, location: ErrorLocation) -> Self {
        match self {
            Error::ExampleDidNotMatch {
                rules,
                examples,
                location: None,
            } => Error::ExampleDidNotMatch {
                rules,
                examples,
                location: Some(location),
            },
            Error::ExampleDidMatch {
                rule,
                example,
                location: None,
            } => Error::ExampleDidMatch {
                rule,
                example,
                location: Some(location),
            },
            other => other,
        }
    }

    pub fn location(&self) -> Option<ErrorLocation> {
        match self {
            Error::ExampleDidNotMatch { location, .. }
            | Error::ExampleDidMatch { location, .. } => location.clone(),
            _ => None,
        }
    }
}

// ── Decision ──

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    Allow,
    Prompt,
    Forbidden,
}

impl Decision {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "allow" => Ok(Self::Allow),
            "prompt" => Ok(Self::Prompt),
            "forbidden" => Ok(Self::Forbidden),
            other => Err(Error::InvalidDecision(other.to_string())),
        }
    }
}

// ── Rule types ──

pub mod rule {
    pub use super::NetworkRule;
    pub use super::NetworkRuleProtocol;
    pub use super::PatternToken;
    pub use super::PrefixPattern;
    pub use super::PrefixRule;
    pub use super::Rule;
    pub use super::RuleRef;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PatternToken {
    Single(String),
    Alts(Vec<String>),
}

impl PatternToken {
    pub fn alternatives(&self) -> &[String] {
        match self {
            Self::Single(expected) => std::slice::from_ref(expected),
            Self::Alts(alternatives) => alternatives,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrefixPattern {
    pub first: Arc<str>,
    pub rest: Arc<[PatternToken]>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleMatch {
    PrefixRuleMatch {
        #[serde(rename = "matchedPrefix")]
        matched_prefix: Vec<String>,
        decision: Decision,
        #[serde(rename = "resolvedProgram", skip_serializing_if = "Option::is_none")]
        resolved_program: Option<AbsolutePathBuf>,
        #[serde(skip_serializing_if = "Option::is_none")]
        justification: Option<String>,
    },
    HeuristicsRuleMatch {
        command: Vec<String>,
        decision: Decision,
    },
}

impl RuleMatch {
    pub fn decision(&self) -> Decision {
        match self {
            Self::PrefixRuleMatch { decision, .. } => *decision,
            Self::HeuristicsRuleMatch { decision, .. } => *decision,
        }
    }

    pub fn with_resolved_program(self, resolved_program: &AbsolutePathBuf) -> Self {
        match self {
            Self::PrefixRuleMatch {
                matched_prefix,
                decision,
                justification,
                ..
            } => Self::PrefixRuleMatch {
                matched_prefix,
                decision,
                resolved_program: Some(resolved_program.clone()),
                justification,
            },
            other => other,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrefixRule {
    pub pattern: PrefixPattern,
    pub decision: Decision,
    pub justification: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkRuleProtocol {
    Http,
    Https,
    Socks5Tcp,
    Socks5Udp,
}

impl NetworkRuleProtocol {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "http" => Ok(Self::Http),
            "https" | "https_connect" | "http-connect" => Ok(Self::Https),
            "socks5_tcp" => Ok(Self::Socks5Tcp),
            "socks5_udp" => Ok(Self::Socks5Udp),
            other => Err(Error::InvalidRule(format!(
                "network_rule protocol must be one of http, https, socks5_tcp, socks5_udp (got {other})"
            ))),
        }
    }

    pub fn as_policy_string(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
            Self::Socks5Tcp => "socks5_tcp",
            Self::Socks5Udp => "socks5_udp",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkRule {
    pub host: String,
    pub protocol: NetworkRuleProtocol,
    pub decision: Decision,
    pub justification: Option<String>,
}

pub trait Rule: Any + Debug + Send + Sync {
    fn program(&self) -> &str;
    fn matches(&self, cmd: &[String]) -> Option<RuleMatch>;
    fn as_any(&self) -> &dyn Any;
}

pub type RuleRef = Arc<dyn Rule>;

impl Rule for PrefixRule {
    fn program(&self) -> &str {
        self.pattern.first.as_ref()
    }

    fn matches(&self, _cmd: &[String]) -> Option<RuleMatch> {
        None
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ── Policy ──

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MatchOptions {
    pub resolve_host_executables: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Policy;

impl Policy {
    pub fn new() -> Self {
        Self
    }

    pub fn empty() -> Self {
        Self
    }

    pub fn get_allowed_prefixes(&self) -> Vec<Vec<String>> {
        Vec::new()
    }

    pub fn add_prefix_rule(&mut self, _prefix: &[String], _decision: Decision) -> Result<()> {
        Ok(())
    }

    pub fn add_network_rule(
        &mut self,
        _host: &str,
        _protocol: NetworkRuleProtocol,
        _decision: Decision,
        _justification: Option<String>,
    ) -> Result<()> {
        Ok(())
    }

    pub fn merge_overlay(&self, _overlay: &Policy) -> Policy {
        Policy
    }

    pub fn compiled_network_domains(&self) -> (Vec<String>, Vec<String>) {
        (Vec::new(), Vec::new())
    }

    pub fn check<F>(&self, _cmd: &[String], _heuristics_fallback: &F) -> Evaluation
    where
        F: Fn(&[String]) -> Decision,
    {
        Evaluation::always_allow()
    }

    pub fn check_with_options<F>(
        &self,
        _cmd: &[String],
        _heuristics_fallback: &F,
        _options: &MatchOptions,
    ) -> Evaluation
    where
        F: Fn(&[String]) -> Decision,
    {
        Evaluation::always_allow()
    }

    pub fn check_multiple<Commands, F>(
        &self,
        _commands: Commands,
        _heuristics_fallback: &F,
    ) -> Evaluation
    where
        Commands: IntoIterator,
        Commands::Item: AsRef<[String]>,
        F: Fn(&[String]) -> Decision,
    {
        Evaluation::always_allow()
    }

    pub fn check_multiple_with_options<Commands, F>(
        &self,
        _commands: Commands,
        _heuristics_fallback: &F,
        _options: &MatchOptions,
    ) -> Evaluation
    where
        Commands: IntoIterator,
        Commands::Item: AsRef<[String]>,
        F: Fn(&[String]) -> Decision,
    {
        Evaluation::always_allow()
    }

    pub fn matches_for_command(
        &self,
        _cmd: &[String],
        _heuristics_fallback: Option<&dyn Fn(&[String]) -> Decision>,
    ) -> Vec<RuleMatch> {
        vec![RuleMatch::HeuristicsRuleMatch {
            command: Vec::new(),
            decision: Decision::Allow,
        }]
    }

    pub fn matches_for_command_with_options(
        &self,
        _cmd: &[String],
        _heuristics_fallback: Option<&dyn Fn(&[String]) -> Decision>,
        _options: &MatchOptions,
    ) -> Vec<RuleMatch> {
        vec![RuleMatch::HeuristicsRuleMatch {
            command: Vec::new(),
            decision: Decision::Allow,
        }]
    }

    pub fn rules(&self) -> std::collections::HashMap<String, Vec<RuleRef>> {
        std::collections::HashMap::new()
    }

    pub fn network_rules(&self) -> &[NetworkRule] {
        &[]
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evaluation {
    pub decision: Decision,
    #[serde(rename = "matchedRules")]
    pub matched_rules: Vec<RuleMatch>,
}

impl Evaluation {
    pub fn is_match(&self) -> bool {
        false
    }

    pub fn always_allow() -> Self {
        Self {
            decision: Decision::Allow,
            matched_rules: vec![RuleMatch::HeuristicsRuleMatch {
                command: Vec::new(),
                decision: Decision::Allow,
            }],
        }
    }
}

// ── Parser (stub) ──

pub struct PolicyParser;

impl Default for PolicyParser {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyParser {
    pub fn new() -> Self {
        Self
    }

    pub fn parse(&mut self, _policy_identifier: &str, _policy_file_contents: &str) -> Result<()> {
        Ok(())
    }

    pub fn build(self) -> Policy {
        Policy::empty()
    }
}

// ── Amend (no-ops) ──

#[derive(Debug, thiserror::Error)]
pub enum AmendError {
    #[error("prefix rule requires at least one token")]
    EmptyPrefix,
    #[error("invalid network rule: {0}")]
    InvalidNetworkRule(String),
    #[error("policy path has no parent: {path}")]
    MissingParent { path: PathBuf },
}

pub fn blocking_append_allow_prefix_rule(
    _policy_path: &Path,
    _prefix: &[String],
) -> std::result::Result<(), AmendError> {
    Ok(())
}

pub fn blocking_append_network_rule(
    _policy_path: &Path,
    _host: &str,
    _protocol: NetworkRuleProtocol,
    _decision: Decision,
    _justification: Option<&str>,
) -> std::result::Result<(), AmendError> {
    Ok(())
}
