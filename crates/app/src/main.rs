fn main() {
    // git and ssh run this binary as their askpass: answer and exit, before any window.
    if let Some(code) = gitviber_core::askpass::helper() {
        std::process::exit(code);
    }
    // `gitviber browser …` in a terminal: one short request to the running app.
    if let Some(code) = gitviber_browser::client::helper() {
        std::process::exit(code);
    }
    // Linux: a second launch hands its paths to the app running and goes.
    if let Some(code) = gitviber_app::second_launch() {
        std::process::exit(code);
    }
    gitviber_core::malloc::return_freed_blocks();
    gitviber_app::run(|_| {});
}
