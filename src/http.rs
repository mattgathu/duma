use std::fs;
use std::env;
use std::fmt;
use std::thread;
use std::sync::mpsc;
use std::cell::RefCell;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};

use console::style;
use num_cpus::get as get_cpus_num;
use indicatif::{HumanBytes, ProgressBar};
use reqwest::{Client, Proxy, Response, StatusCode, Url};
use reqwest::header::{AcceptRanges, ByteRangeSpec, ContentLength, ContentType, Headers, Range,
                      RangeUnit};

use utils::get_file_handle;
use bar::create_progress_bar;


fn get_reqwest_client() -> Result<(Client), Box<::std::error::Error>> {
    let http_proxy = env::var("http_proxy");
    let https_proxy = env::var("https_proxy");

    let mut builder = Client::builder()?;

    if http_proxy.is_ok() {
        builder.proxy(Proxy::http(Url::parse(&http_proxy.unwrap())?)?);
    }

    if https_proxy.is_ok() {
        builder.proxy(Proxy::https(Url::parse(&https_proxy.unwrap())?)?);
    }

    Ok(builder.build()?)
}


fn get_chunk_sizes(ct_len: u64) -> Vec<(u64, u64)> {
    let cpus = get_cpus_num() as u64;
    let chunk_size = ct_len / cpus;
    let mut sizes = Vec::new();

    for core in 0..cpus {
        let bound = if core == cpus - 1 {
            ct_len
        } else {
            ((core + 1) * chunk_size) - 1
        };
        sizes.push((core * chunk_size, bound));
    }

    sizes
}

fn download_chunk(url: Url,
                  offsets: (u64, u64),
                  progress_sender: mpsc::Sender<(u64, u64, Vec<u8>)>)
                  -> Result<(), Box<::std::error::Error>> {
    let client = get_reqwest_client()?;
    let byte_range = Range::Bytes(vec![ByteRangeSpec::FromTo(offsets.0, offsets.1)]);
    let mut resp = client.get(url)?
        .header(byte_range)
        .send()?;
    let chunk_sz = offsets.1 - offsets.0;
    let mut start_offset = offsets.0;

    loop {
        let mut buf = vec![0; chunk_sz as usize];
        let byte_count = resp.read(&mut buf[..]).unwrap();
        buf.truncate(byte_count);
        if !buf.is_empty() {
            progress_sender.send((byte_count as u64, start_offset, buf.clone())).unwrap();
            start_offset += byte_count as u64;
        } else {
            break;
        }
    }

    Ok(())

}

pub fn download(url: Url,
                quiet_mode: bool,
                filename: Option<&str>,
                resume_download: bool,
                multithread: bool)
                -> Result<(), Box<::std::error::Error>> {
    let fname = match filename {
        Some(name) => name,
        None => {
            let name = &url.path()
                            .split('/')
                            .last()
                            .unwrap();
            if !name.is_empty() { name } else { "index.html" }
        }
    };


    let mut client = Downloader::new(url.clone(), fname, multithread, resume_download);
    if !quiet_mode {
        let events_handler = DownloadEventsHandler::new(fname);
        client.events_hook(events_handler).download()?;
    } else {
        client.download()?;
    }
    Ok(())
}


#[allow(unused_variables)]
pub trait Events {
    fn on_bufwrite(&mut self, byte_count: u64) {}

    fn on_resume_download(&mut self, bytes_on_disk: u64) {}

    fn on_headers(&mut self, headers: Headers) {}

    fn on_read(&mut self, bytes: Vec<u8>) {}

    fn on_content_length(&mut self, ct_len: u64) {}

    fn on_success_status(&self) {}

    fn on_failure_status(&self, status_code: StatusCode) {}

    fn on_finish(&mut self) {}
}

pub struct Downloader {
    url: Url,
    buf: Option<BufWriter<fs::File>>,
    multithread: bool,
    resume: bool,
    chunk_sz: usize,
    hooks: Vec<RefCell<Box<Events>>>,
    fname: String,
}

impl fmt::Debug for Downloader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
               "Downloader {{ url: {}, multithread: {} }}",
               self.url,
               self.multithread)
    }
}

impl Downloader {
    pub fn new(url: Url, fname: &str, multithread: bool, resume: bool) -> Downloader {
        Downloader {
            url: url,
            buf: None,
            multithread: multithread,
            resume: resume,
            chunk_sz: 1024,
            hooks: Vec::new(),
            fname: fname.to_owned(),
        }
    }

    pub fn download(&mut self) -> Result<(), Box<::std::error::Error>> {
        let client = get_reqwest_client()?;
        let mut req = client.get(self.url.clone())?;
        let head_resp = client.head(self.url.clone())?
            .send()?;
        self.buf = Some(BufWriter::new(get_file_handle(&self.fname, self.resume).unwrap()));

        if self.resume {
            let head_resp = client.head(self.url.clone())?
                .send()?;
            let supports_bytes = match head_resp.headers().get::<AcceptRanges>() {
                Some(header) => header.contains(&RangeUnit::Bytes),
                None => false,
            };
            let byte_count = if supports_bytes {
                match fs::metadata(&self.fname) {
                    Ok(metadata) => Some(metadata.len()),
                    _ => None,
                }
            } else {
                None
            };
            if byte_count.is_some() {
                req.header(Range::Bytes(vec![ByteRangeSpec::AllFrom(byte_count.unwrap())]));
                for hk in &self.hooks {
                    hk.borrow_mut().on_resume_download(byte_count.unwrap());
                }
            }
        };
        let mut resp = req.send()?;

        if resp.status().is_success() {
            let headers = head_resp.headers();
            for hk in &self.hooks {
                hk.borrow_mut().on_headers(headers.clone());
            }
            if self.multithread {
                self.multithread_download(&resp)?;
            } else {
                self.singlethread_download(&mut resp)?;
            }
        } else {
            for hk in &self.hooks {
                hk.borrow_mut().on_failure_status(resp.status());
            }
        }

        for hook in &self.hooks {
            hook.borrow_mut().on_finish();
        }

        Ok(())
    }

    fn multithread_download(&mut self, resp: &Response) -> Result<(), Box<::std::error::Error>> {
        let ct_len = resp.headers().get::<ContentLength>().map(|ct_len| **ct_len);
        let (tx, rx) = mpsc::channel();
        for offsets in get_chunk_sizes(ct_len.unwrap()) {
            let url = self.url.clone();
            let tx = tx.clone();
            thread::spawn(move || { download_chunk(url, offsets, tx).unwrap(); });
        }
        let mut bytes_recv = 0;
        loop {
            if bytes_recv == ct_len.unwrap() {
                break;
            } else {
                let (byte_count, offset, buf) = rx.recv().unwrap();
                self.write_buf(buf.as_slice(), Some(offset))?;
                bytes_recv += byte_count;
            }
        }
        Ok(())
    }

    pub fn events_hook<E: Events + 'static>(&mut self, hk: E) -> &mut Downloader {
        self.hooks.push(RefCell::new(Box::new(hk)));
        self
    }

    fn singlethread_download(&mut self,
                             resp: &mut Response)
                             -> Result<(), Box<::std::error::Error>> {
        loop {
            let mut buffer = vec![0; self.chunk_sz];
            let bcount = resp.read(&mut buffer[..]).unwrap();
            buffer.truncate(bcount);
            if !buffer.is_empty() {
                self.write_buf(buffer.as_slice(), None)?;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn write_buf(&mut self,
                 contents: &[u8],
                 offset: Option<u64>)
                 -> Result<(), Box<::std::error::Error>> {
        let byte_count = contents.len() as u64;
        if offset.is_some() {
            self.buf
                .as_mut()
                .unwrap()
                .seek(SeekFrom::Start(offset.unwrap()))?;
        }
        self.buf
            .as_mut()
            .unwrap()
            .write_all(contents)?;

        for hk in &self.hooks {
            hk.borrow_mut().on_bufwrite(byte_count);
        }

        Ok(())
    }
}


pub struct DownloadEventsHandler {
    prog_bar: Option<ProgressBar>,
    bytes_on_disk: Option<u64>,
    fname: String,
}

impl DownloadEventsHandler {
    pub fn new(fname: &str) -> DownloadEventsHandler {
        DownloadEventsHandler {
            prog_bar: None,
            bytes_on_disk: None,
            fname: fname.to_owned(),
        }
    }
}

impl Events for DownloadEventsHandler {
    fn on_headers(&mut self, headers: Headers) {
        let ct_type = headers.get::<ContentType>().unwrap();
        println!("Type: {}", style(ct_type).green());

        println!("Saving to: {}", style(&self.fname).green());
        let ct_len = headers.get::<ContentLength>().map(|ct_len| **ct_len);

        let byte_count = self.bytes_on_disk;
        match ct_len {
            Some(len) => {
                let exact = style(len - byte_count.unwrap_or(0)).green();
                let human_readable =
                    style(format!("{}", HumanBytes(len - byte_count.unwrap_or(0)))).red();

                println!("Length: {} ({})", exact, human_readable);
            }
            None => {
                println!("Length: {}", style("unknown").red());
            }
        }

        let prog_bar = create_progress_bar(false, &self.fname, ct_len);
        if byte_count.is_some() {
            prog_bar.inc(byte_count.unwrap());
        }
        self.prog_bar = Some(prog_bar);
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

    fn on_bufwrite(&mut self, byte_count: u64) {
        self.prog_bar
            .as_mut()
            .unwrap()
            .inc(byte_count);
    }

    fn on_failure_status(&self, status: StatusCode) {
        if status.as_u16() == 416 {
            println!("{}",
                     &style("\nThe file is already fully retrieved; nothing to do.\n").red());

        }
    }
}
