//! mcpm-client executable entry point.

fn main() -> std::process::ExitCode {
    mcpm::run(mcpm::Program::McpmClient)
}
