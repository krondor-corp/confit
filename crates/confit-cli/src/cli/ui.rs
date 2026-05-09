use colored::{ColoredString, Colorize};

pub fn success(msg: &str) {
    eprintln!("{}", format!("✓ {msg}").green());
}

pub fn progress(msg: &str) {
    eprintln!("{}", format!("ℹ {msg}").blue());
}

pub fn failure(msg: &str) {
    eprintln!("{}", format!("✗ {msg}").red());
}

pub fn warning(msg: &str) {
    eprintln!("{}", format!("⚠ {msg}").yellow());
}

pub fn highlight(s: &str) -> ColoredString {
    s.cyan()
}

pub fn bold(s: &str) -> ColoredString {
    s.bold()
}

pub fn dim(s: &str) -> ColoredString {
    s.dimmed()
}

pub fn print_error(err: &dyn std::error::Error) {
    failure(&err.to_string());
    let mut source = err.source();
    while let Some(cause) = source {
        eprintln!("  {} {}", "caused by:".dimmed(), cause);
        source = cause.source();
    }
}
