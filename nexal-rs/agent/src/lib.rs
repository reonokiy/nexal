mod bot;
mod podman;
mod pool;
mod runner;

pub use bot::Bot;
pub use pool::AgentPool;

/// Split a model response into ≤4096-char chunks at blank lines,
/// suitable for sending as separate bot messages.
pub fn split_response(text: String) -> Vec<String> {
    if text.is_empty() {
        return vec!["(no response)".to_string()];
    }

    const MAX_LEN: usize = 4000;
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();

    for paragraph in text.split("\n\n") {
        let block = paragraph.trim();
        if block.is_empty() {
            continue;
        }
        // If adding this paragraph would exceed the limit, flush first.
        if !current.is_empty() && current.len() + 2 + block.len() > MAX_LEN {
            out.push(current.trim().to_string());
            current.clear();
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(block);
    }

    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }

    if out.is_empty() {
        out.push(text.trim().to_string());
    }

    out
}
