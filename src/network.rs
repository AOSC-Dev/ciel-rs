use failure::Error;
use lazy_static::lazy_static;
use reqwest::blocking::{Client, Response};
use std::path::PathBuf;

lazy_static! {
    static ref GIT_PROGRESS: indicatif::ProgressStyle = indicatif::ProgressStyle::default_bar()
        .template("[{bar:40.cyan/blue}] {pos}/{len} {msg} ({eta})");
}

/// Download a file from the web
pub fn download_file(url: &str) -> Result<Response, Error> {
    let client = Client::new().get(url).send()?;

    Ok(client)
}

/// Clone the Git repository to `root`
pub fn download_git(uri: &str, root: PathBuf) -> Result<(), Error> {
    let mut callbacks = git2::RemoteCallbacks::new();
    let mut co_callback = git2::build::CheckoutBuilder::new();
    let progress_dl = indicatif::ProgressBar::new(1);
    let progress_res = indicatif::ProgressBar::new(1);
    let progress_co = indicatif::ProgressBar::new(1);

    progress_dl.set_style(GIT_PROGRESS.clone());
    progress_res.set_style(GIT_PROGRESS.clone());
    progress_co.set_style(GIT_PROGRESS.clone());

    progress_dl.set_message("Waiting for server...");
    progress_dl.set_position(0);

    callbacks.transfer_progress(move |p: git2::Progress| {
        if p.received_objects() == p.total_objects() {
            progress_res.set_message("Resolving deltas...");
            progress_res.set_length(p.total_deltas() as u64);
            progress_res.set_position(p.indexed_deltas() as u64);
        } else {
            let human_bytes = indicatif::HumanBytes(p.received_bytes() as u64);
            progress_dl.set_position(p.received_objects() as u64);
            progress_dl.set_length(p.total_objects() as u64);
            progress_dl.set_message(&format!("{}", human_bytes));
        }

        true
    });

    co_callback.progress(move |_, cur, total| {
        progress_co.set_message("Checking out files...");
        progress_co.set_length(total as u64);
        progress_co.set_position(cur as u64);
    });
    let mut options = git2::FetchOptions::new();
    options.remote_callbacks(callbacks);
    git2::build::RepoBuilder::new()
        .fetch_options(options)
        .with_checkout(co_callback)
        .clone(uri, &root)?;

    Ok(())
}
