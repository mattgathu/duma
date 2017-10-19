extern crate clap;
extern crate rget;

use std::process;

use rget::http;
use rget::rftp;
use rget::utils;

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
    let args = App::new("Rget")
        .version("0.1.0")
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
        .arg(Arg::with_name("multithread")
                 .short("M")
                 .long("multithread")
                 .help("use multithreading for faster download (no resume capability)")
                 .required(false)
                 .takes_value(false))
        .arg(Arg::with_name("FILE")
                 .short("O")
                 .long("output-document")
                 .help("write documents to FILE")
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
    let resume_download = args.is_present("continue");
    let file_name = args.value_of("FILE");
    let multithread = args.is_present("multithread");

    match url.scheme() {
        "ftp" => rftp::download(url, file_name, quiet_mode),
        "http" | "https" => http::download(url, quiet_mode, file_name, resume_download, multithread),
        _ => utils::gen_error(format!("unsupported url scheme '{}'", url.scheme())),
    }
}
