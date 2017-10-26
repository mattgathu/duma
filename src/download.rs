use std::fs;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::error::Error;

use console::style;
use indicatif::{HumanBytes, ProgressBar};
use reqwest::{StatusCode, Url};
use reqwest::header::{ContentLength, ContentType, Headers};

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
            if !name.is_empty() { format!("{}", name) } else { "index.html".to_owned() }
        }
    }


}

pub fn ftp_download(url: Url, quiet_mode: bool, filename: Option<&str>) -> Result<(), Box<Error>> {
    let fname = gen_filename(&url, filename);

    let mut client = FtpDownload::new(url.clone());
    if !quiet_mode {
        let events_handler = DownloadEventsHandler::new(&fname);
        client.events_hook(events_handler).download()?;
    } else {
        let events_handler = QuietModeEventsHandler::new(&fname);
        client.events_hook(events_handler).download()?;
    }
    Ok(())

}

pub fn http_download(url: Url,
                     quiet_mode: bool,
                     filename: Option<&str>,
                     resume_download: bool,
                     multithread: bool)
                     -> Result<(), Box<Error>> {
    let fname = gen_filename(&url, filename);

    let mut client = HttpDownload::new(url.clone(), &fname, multithread, resume_download);
    if !quiet_mode {
        let events_handler = DownloadEventsHandler::new(&fname);
        client.events_hook(events_handler).download()?;
    } else {
        let events_handler = QuietModeEventsHandler::new(&fname);
        client.events_hook(events_handler).download()?;
    }
    Ok(())
}



pub struct DownloadEventsHandler {
    prog_bar: Option<ProgressBar>,
    bytes_on_disk: Option<u64>,
    fname: String,
    file: BufWriter<fs::File>,
}

impl DownloadEventsHandler {
    pub fn new(fname: &str) -> DownloadEventsHandler {
        DownloadEventsHandler {
            prog_bar: None,
            bytes_on_disk: None,
            fname: fname.to_owned(),
            file: BufWriter::new(get_file_handle(fname, false).unwrap()),
        }
    }

    fn create_prog_bar(&mut self, length: Option<u64>) {
        let byte_count = self.bytes_on_disk;
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

    fn on_content(&mut self, content: &[u8], offset: Option<u64>) -> Result<(), Box<Error>> {
        let byte_count = content.len() as u64;
        if offset.is_some() {
            self.file.seek(SeekFrom::Start(offset.unwrap()))?;
        }
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
    file: BufWriter<fs::File>,
}

impl QuietModeEventsHandler {
    pub fn new(fname: &str) -> Self {
        Self { file: BufWriter::new(get_file_handle(fname, false).unwrap()) }
    }
}

impl Events for QuietModeEventsHandler {
    fn on_content(&mut self, content: &[u8], offset: Option<u64>) -> Result<(), Box<Error>> {
        if offset.is_some() {
            self.file.seek(SeekFrom::Start(offset.unwrap()))?;
        }
        self.file.write_all(content)?;

        Ok(())
    }
}
