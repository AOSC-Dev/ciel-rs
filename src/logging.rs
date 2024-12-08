#[macro_export]
macro_rules! info {
    ($($arg:tt)+) => {
        eprint!("{} ", ::console::style("info:").cyan().bold());
        eprintln!($($arg)+);
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)+) => {
        eprint!("{} ", ::console::style("warning:").yellow().bold());
        eprintln!($($arg)+);
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)+) => {
        eprint!("{} ", ::console::style("error:").red().bold());
        eprintln!($($arg)+);
    };
}

#[inline]
pub fn color_bool(pred: bool) -> &'static str {
    if pred {
        "\x1b[1m\x1b[32mYes\x1b[0m"
    } else {
        "\x1b[34mNo\x1b[0m"
    }
}
