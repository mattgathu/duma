use std::process;

use anyhow::{format_err, Result};
use clap::{clap_app, crate_version};
use duma::download::{ftp_download, http_download};
use duma::utils;
fn main() {
    match run() {
        Ok(_) => {}
        Err(e) => {
            eprintln!("error: {}", e);
            process::exit(1);
        }
    }
}

fn run() -> Result<()> {
    let args = clap_app!(Duma =>
    (version: crate_version!())
    (author: "Matt Gathu <mattgathu@gmail.com>")
    (about: "A minimal file downloader")
    (@arg quiet: -q --quiet "quiet (no output)")
    (@arg continue: -c --continue "resume getting a partially-downloaded file")
    (@arg singlethread: -s --singlethread "download using only a single thread")
    (@arg headers: -H --headers "prints the headers sent by the HTTP server")
    (@arg FILE: -O --output +takes_value "write documents to FILE")
    (@arg AGENT: -U --useragent +takes_value "identify as AGENT instead of Duma/VERSION")
    (@arg SECONDS: -T --timeout +takes_value "set all timeout values to SECONDS")
    (@arg NUM_CONNECTIONS: -n --num_connections +takes_value "maximum number of concurrent connections (default is 8)")
    (@arg URL: +required +takes_value "url to download")
    )
    .get_matches_safe().unwrap_or_else(|e| e.exit());

    let url = utils::parse_url(
        args.value_of("URL")
            .ok_or_else(|| format_err!("missing URL argument"))?,
    )?;
    let quiet_mode = args.is_present("quiet");
    let file_name = args.value_of("FILE");

    match url.scheme() {
        "ftp" => ftp_download(url, quiet_mode, file_name),
        "http" | "https" => http_download(url, &args, crate_version!()),
        _ => utils::gen_error(format!("unsupported url scheme '{}'", url.scheme())),
    }
}
