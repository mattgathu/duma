use std::fs;
use std::env;
use std::error::Error;
use std::time::Duration;
use std::collections::HashMap;
use std::io::{BufWriter, Write};

use clap::ArgMatches;
use console::style;
use indicatif::{HumanBytes, ProgressBar};
use reqwest::{StatusCode, Url};
use reqwest::header::{ByteRangeSpec, ContentLength, ContentType, Headers, Range, UserAgent};

use utils::get_file_handle;
use bar::create_progress_bar;
use core::{Events, FtpDownload, HttpDownload};


fn gen_filename(url: &Url, fname: Option<&str>) -> String {
    match fname {
        Some(name) => name.to_owned(),
        None => {
            let name = &url.path()
                            .split('/')
                            .last()
                            .unwrap();
            if !name.is_empty() {
                format!("{}", name)
            } else {
                "index.html".to_owned()
            }
        }
    }


}

fn calc_bytes_on_disk(fname: &str) -> Option<u64> {
    match fs::metadata(fname) {
        Ok(metadata) => Some(metadata.len()),
        _ => None,
    }
}

fn prep_headers(fname: &str, resume: bool, user_agent: String) -> Headers {
    let bytes_on_disk = calc_bytes_on_disk(fname);
    let mut headers = Headers::new();
    if resume && bytes_on_disk.is_some() {
        let range_hdr = Range::Bytes(vec![ByteRangeSpec::AllFrom(bytes_on_disk.unwrap())]);
        headers.set(range_hdr);
    }

    headers.set(UserAgent::new(user_agent));

    headers
}

fn get_http_proxies() -> Option<HashMap<String, String>> {
    let mut proxies = HashMap::new();
    if let Ok(proxy) = env::var("http_proxy") {
        proxies.insert("http_proxy".to_owned(), proxy);
    };
    if let Ok(proxy) = env::var("https_proxy") {
        proxies.insert("https_proxy".to_owned(), proxy);
    };

    if !proxies.is_empty() {
        Some(proxies)
    } else {
        None
    }

}

pub fn ftp_download(url: Url, quiet_mode: bool, filename: Option<&str>) -> Result<(), Box<Error>> {
    let fname = gen_filename(&url, filename);

    let mut client = FtpDownload::new(url.clone());
    if !quiet_mode {
        let events_handler = DownloadEventsHandler::new(&fname, false);
        client.events_hook(events_handler).download()?;
    } else {
        let events_handler = QuietModeEventsHandler::new(&fname, false);
        client.events_hook(events_handler).download()?;
    }
    Ok(())

}

pub fn http_download(url: Url, args: &ArgMatches, version: &str) -> Result<(), Box<Error>> {
    let resume_download = args.is_present("continue");
    let user_agent = args.value_of("AGENT").unwrap_or(&format!("Duma/{}", version)).to_owned();

    let fname = gen_filename(&url, args.value_of("FILE"));
    let headers = prep_headers(&fname, resume_download, user_agent);
    let timeout = if let Some(secs) = args.value_of("SECONDS") {
        Some(Duration::new(secs.parse::<u64>()?, 0))
    } else {
        None
    };
    let proxies = get_http_proxies();

    let mut client = HttpDownload::new(url.clone(), headers, timeout, proxies);
    if !args.is_present("quiet") {
        let events_handler = DownloadEventsHandler::new(&fname, resume_download);
        client.events_hook(events_handler).download()?;
    } else {
        let events_handler = QuietModeEventsHandler::new(&fname, resume_download);
        client.events_hook(events_handler).download()?;
    }
    Ok(())
}


pub struct DownloadEventsHandler {
    prog_bar: Option<ProgressBar>,
    bytes_on_disk: Option<u64>,
    fname: String,
    file: BufWriter<Box<Write>>,
    server_supports_resume: bool,
}

impl DownloadEventsHandler {
    pub fn new(fname: &str, resume: bool) -> DownloadEventsHandler {
        DownloadEventsHandler {
            prog_bar: None,
            bytes_on_disk: calc_bytes_on_disk(fname),
            fname: fname.to_owned(),
            file: BufWriter::new(get_file_handle(fname, resume).unwrap()),
            server_supports_resume: false,
        }
    }

    fn create_prog_bar(&mut self, length: Option<u64>) {
        let byte_count = if self.server_supports_resume {
            self.bytes_on_disk
        } else {
            None
        };
        if let Some(len) = length {
            let exact = style(len - byte_count.unwrap_or(0)).green();
            let human_readable = style(format!("{}", HumanBytes(len - byte_count.unwrap_or(0))))
                .red();

            println!("Length: {} ({})", exact, human_readable);
        } else {
            println!("Length: {}", style("unknown").red());
        }

        let prog_bar = create_progress_bar(false, &self.fname, length);
        if byte_count.is_some() {
            prog_bar.inc(byte_count.unwrap());
        }
        self.prog_bar = Some(prog_bar);


    }
}

impl Events for DownloadEventsHandler {
    fn on_headers(&mut self, headers: Headers) {
        let ct_type = headers.get::<ContentType>().unwrap();
        println!("Type: {}", style(ct_type).green());

        println!("Saving to: {}", style(&self.fname).green());
        let ct_len = headers.get::<ContentLength>().map(|ct_len| **ct_len);

        self.create_prog_bar(ct_len);
    }

    fn on_ftp_content_length(&mut self, ct_len: Option<u64>) {
        self.create_prog_bar(ct_len);
    }

    fn on_server_supports_resume(&mut self) {
        self.server_supports_resume = true;
    }

    fn on_content(&mut self, content: &[u8]) -> Result<(), Box<Error>> {
        let byte_count = content.len() as u64;
        self.file.write_all(content)?;
        self.prog_bar
            .as_mut()
            .unwrap()
            .inc(byte_count);

        Ok(())
    }

    fn on_resume_download(&mut self, bytes_on_disk: u64) {
        self.bytes_on_disk = Some(bytes_on_disk);
    }

    fn on_finish(&mut self) {
        self.prog_bar
            .as_mut()
            .unwrap()
            .finish();
    }

    fn on_failure_status(&self, status: StatusCode) {
        if status.as_u16() == 416 {
            println!("{}",
                     &style("\nThe file is already fully retrieved; nothing to do.\n").red());

        }
    }
}

struct QuietModeEventsHandler {
    file: BufWriter<Box<Write>>,
}

impl QuietModeEventsHandler {
    pub fn new(fname: &str, resume: bool) -> Self {
        Self { file: BufWriter::new(get_file_handle(fname, resume).unwrap()) }
    }
}

impl Events for QuietModeEventsHandler {
    fn on_content(&mut self, content: &[u8]) -> Result<(), Box<Error>> {
        self.file.write_all(content)?;

        Ok(())
    }
}
