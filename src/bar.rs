use indicatif::{ProgressBar, ProgressStyle};

static PBAR_FMT: &'static str =
    "{msg} {spinner:.green} {percent}% [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} eta: {eta}";

pub fn create_progress_bar(msg: &str, length: Option<u64>) -> ProgressBar {
    let progbar = match length {
        Some(len) => ProgressBar::new(len),
        None => ProgressBar::new_spinner(),
    };

    progbar.set_message(msg);
    if length.is_some() {
        progbar.set_style(
            ProgressStyle::default_bar()
                .template(PBAR_FMT)
                .progress_chars("=> "),
        );
    } else {
        progbar.set_style(ProgressStyle::default_spinner());
    }

    progbar
}
