#[cfg(not(unix))]
fn main() {
    eprintln!("nexal-execve-wrapper is only implemented for UNIX");
    std::process::exit(1);
}

#[cfg(unix)]
pub use nexal_shell_escalation::main_execve_wrapper as main;
