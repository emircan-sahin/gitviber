fn main() {
    // git and ssh run this binary as their askpass: answer and exit, before any window.
    if let Some(code) = gitviber_core::askpass::helper() {
        std::process::exit(code);
    }
    gitviber_core::malloc::return_freed_blocks();
    gitviber_app::run(|_| {});
}
