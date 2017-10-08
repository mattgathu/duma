extern crate clap;
extern crate rget;

use std::process;
use rget::http;
use rget::rgetftp;
use clap::{Arg, App};




fn main() {
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
    let url = args.value_of("URL").unwrap();
    let quiet_mode = args.is_present("quiet");
    let resume_download = args.is_present("continue");
    let file_name = args.value_of("FILE");
    if url.contains("ftp://") {
        match rgetftp::download(url, file_name, quiet_mode) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Got error: {}", e.description());
                process::exit(1);
            }
        }


    } else {
        match http::download(url, quiet_mode, file_name, resume_download) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Got error: {}", e.description());
                process::exit(1);
            }
        }

    }
}



// #[cfg(test)]
// mod tests {
//     use super::*;
// 
//     #[test]
//     fn parse_url_works() {
//         let error = parse_url("www.mattgathu.github.io");
//         match error {
//             Ok(_) => {}
//             Err(_) => panic!("parse_url failed to parse!"),
//         };
//     }
// }
