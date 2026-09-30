// One exe for everything: without arguments it opens the window, with arguments it is the CLI.
#![windows_subsystem = "windows"]

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.len() > 1 {
        lookout::system::attach_console();
        std::process::exit(lookout::cli::main(args));
    }
    lookout::app::run();
}
