//! Only what the app's `main` does first: answer git's or ssh's prompt and exit.

fn main() {
    std::process::exit(gitviber_core::askpass::helper().unwrap_or(2));
}
