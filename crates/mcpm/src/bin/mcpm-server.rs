//! mcpm-server executable entry point.

fn main() -> std::process::ExitCode {
    mcpm::run(mcpm::Program::McpmServer)
}
