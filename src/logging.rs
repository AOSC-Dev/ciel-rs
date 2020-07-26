use fern;
use fern::colors::{Color, ColoredLevelConfig};

pub fn setup_logger() -> Result<(), fern::InitError> {
    let colors = ColoredLevelConfig::new()
        .info(Color::BrightCyan)
        .debug(Color::Green)
        .warn(Color::Yellow)
        .error(Color::BrightRed);

    fern::Dispatch::new()
        .format(move |out, msg, record| {
            out.finish(format_args!("{}: \x1b[1m{}\x1b[0m", colors.color(record.level()), msg))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .apply()?;

    Ok(())
}
