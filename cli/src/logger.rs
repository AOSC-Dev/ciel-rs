use anyhow::Result;
use log::{Level, LevelFilter, Metadata, Record};

struct CielLogger;

impl log::Log for CielLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            match record.level() {
                Level::Error => {
                    eprint!("{} ", ::console::style("error:").red().bold());
                }
                Level::Warn => {
                    eprint!("{} ", ::console::style("warn:").yellow().bold());
                }
                Level::Info => {
                    eprint!("{} ", ::console::style("info:").cyan().bold());
                }
                Level::Debug => todo!(),
                Level::Trace => todo!(),
            }
            eprintln!("{}", record.args());
        }
    }

    fn flush(&self) {}
}

pub fn init() -> Result<()> {
    log::set_boxed_logger(Box::new(CielLogger)).map(|()| log::set_max_level(LevelFilter::Info))?;
    Ok(())
}

#[inline]
pub fn style_bool(pred: bool) -> &'static str {
    if pred {
        "\x1b[1m\x1b[32mYes\x1b[0m"
    } else {
        "\x1b[34mNo\x1b[0m"
    }
}
