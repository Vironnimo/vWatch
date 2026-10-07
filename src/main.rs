//! The console entry point. Shells wait for the process and inherit its standard streams.

fn main() {
    std::process::exit(watchtower::cli_jobs::main(std::env::args_os().collect()));
}
