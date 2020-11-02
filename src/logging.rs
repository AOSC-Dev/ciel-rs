#[macro_export]
macro_rules! info {
    ($($arg:tt)+) => {
        eprint!("{} ", style("info:").cyan().bold());
        eprintln!($($arg)+);
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)+) => {
        eprint!("{} ", style("warning:").yellow().bold());
        eprintln!($($arg)+);
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)+) => {
        eprint!("{} ", style("error:").red().bold());
        eprintln!($($arg)+);
    };
}

#[macro_export]
macro_rules! color_bool {
    ($x:expr) => {
        if $x {
            style("Yes").green().bold()
        } else {
            style("No").cyan()
        }
    };
}
