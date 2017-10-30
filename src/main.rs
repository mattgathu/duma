#[macro_use]
extern crate clap;
extern crate duma;

use std::process;

use duma::download::{ftp_download, http_download};
use duma::utils;

use clap::{App, Arg};


fn main() {
    match run() {
        Ok(_) => {}
        Err(e) => {
            eprintln!("error: {}", e);
            process::exit(1);
        }
    }
}

fn run() -> Result<(), Box<::std::error::Error>> {
    let args = App::new("Duma")
        .version(crate_version!())
        .author("Matt Gathu <mattgathu@gmail.com>")
        .about("wget clone written in Rust")
        .arg(Arg::with_name("quiet")
                 .short("q")
                 .long("quiet")
                 .help("quiet (no output)")
                 .required(false)
                 .takes_value(false))
        .arg(Arg::with_name("continue")
                 .short("c")
                 .long("continue")
                 .help("resume getting a partially-downloaded file")
                 .required(false)
                 .takes_value(false))
        .arg(Arg::with_name("FILE")
                 .short("O")
                 .long("output-document")
                 .help("write documents to FILE")
                 .required(false)
                 .takes_value(true))
        .arg(Arg::with_name("AGENT")
                 .short("U")
                 .long("user-agent")
                 .help("identify as AGENT instead of Duma/VERSION")
                 .required(false)
                 .takes_value(true))
        .arg(Arg::with_name("SECONDS")
                 .short("T")
                 .long("timeout")
                 .help("set all timeout values to SECONDS")
                 .required(false)
                 .takes_value(true))
        .arg(Arg::with_name("URL")
                 .required(true)
                 .takes_value(true)
                 .index(1)
                 .help("url to download"))
        .get_matches();
    let url = utils::parse_url(args.value_of("URL").unwrap())?;
    let quiet_mode = args.is_present("quiet");
    let file_name = args.value_of("FILE");

    match url.scheme() {
        "ftp" => ftp_download(url, quiet_mode, file_name),
        "http" | "https" => http_download(url, &args, crate_version!()),
        _ => utils::gen_error(format!("unsupported url scheme '{}'", url.scheme())),
    }
}
