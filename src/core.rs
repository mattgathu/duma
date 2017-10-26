use std::fs;
use std::env;
use std::fmt;
use std::thread;
use std::sync::mpsc;
use std::error::Error;
use std::cell::RefCell;
use std::io::Read;

use num_cpus::get as get_cpus_num;
use reqwest::{Client, Proxy, Response, StatusCode, Url};
use reqwest::header::{AcceptRanges, ByteRangeSpec, ContentLength, Headers, Range, RangeUnit};

use ftp::FtpStream;




#[allow(unused_variables)]
pub trait Events {
    fn on_resume_download(&mut self, bytes_on_disk: u64) {}

    fn on_headers(&mut self, headers: Headers) {}

    fn on_content(&mut self, content: &[u8], offset: Option<u64>) -> Result<(), Box<Error>> {
        Ok(())
    }

    fn on_content_length(&mut self, ct_len: u64) {}

    fn on_ftp_content_length(&mut self, ct_len: Option<u64>) {}

    fn on_success_status(&self) {}

    fn on_failure_status(&self, status_code: StatusCode) {}

    fn on_finish(&mut self) {}
}

pub struct FtpDownload {
    url: Url,
    hooks: Vec<RefCell<Box<Events>>>,
}

impl FtpDownload {
    pub fn new(url: Url) -> Self {
        Self {
            url: url,
            hooks: Vec::new(),
        }
    }

    pub fn download(&mut self) -> Result<(), Box<Error>> {
        let ftp_server = format!("{}:{}",
                                 self.url.host_str().unwrap(),
                                 self.url.port_or_known_default().unwrap());
        let username = if self.url.username().is_empty() {
            "anonymous"
        } else {
            self.url.username()
        };
        let password = self.url.password().unwrap_or("anonymous");

        let mut path_segments: Vec<&str> = self.url
            .path_segments()
            .unwrap()
            .collect();
        let ftp_fname = path_segments.pop().unwrap();

        let mut conn = FtpStream::connect(ftp_server)?;
        conn.login(username, password)?;
        for path in &path_segments {
            conn.cwd(path)?;
        }
        let ct_len = conn.size(ftp_fname)?;
        let mut reader = conn.get(ftp_fname)?;

        for hook in &self.hooks {
            let ct_len = if ct_len.is_some() {
                Some(ct_len.unwrap() as u64)
            } else {
                None
            };
            hook.borrow_mut().on_ftp_content_length(ct_len);
        }

        loop {
            let mut buffer = vec![0; 2048usize];
            let bcount = reader.read(&mut buffer[..]).unwrap();
            buffer.truncate(bcount);
            if !buffer.is_empty() {
                self.send_content(buffer.as_slice(), None)?;
            } else {
                break;
            }
        }

        for hook in &self.hooks {
            hook.borrow_mut().on_finish();
        }

        Ok(())


    }
    fn send_content(&self, contents: &[u8], offset: Option<u64>) -> Result<(), Box<Error>> {
        for hk in &self.hooks {
            hk.borrow_mut().on_content(contents, offset)?;
        }
        Ok(())
    }
    pub fn events_hook<E: Events + 'static>(&mut self, hk: E) -> &mut FtpDownload {
        self.hooks.push(RefCell::new(Box::new(hk)));
        self
    }
}

pub struct HttpDownload {
    url: Url,
    multithread: bool,
    resume: bool,
    chunk_sz: usize,
    hooks: Vec<RefCell<Box<Events>>>,
    fname: String,
}

impl fmt::Debug for HttpDownload {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
               "HttpDownload {{ url: {}, multithread: {} }}",
               self.url,
               self.multithread)
    }
}

impl HttpDownload {
    pub fn new(url: Url, fname: &str, multithread: bool, resume: bool) -> HttpDownload {
        HttpDownload {
            url: url,
            multithread: multithread,
            resume: resume,
            chunk_sz: 1024,
            hooks: Vec::new(),
            fname: fname.to_owned(),
        }
    }

    pub fn download(&mut self) -> Result<(), Box<Error>> {
        let client = Self::get_reqwest_client()?;
        let mut req = client.get(self.url.clone())?;
        let head_resp = client.head(self.url.clone())?
            .send()?;

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

    fn multithread_download(&mut self, resp: &Response) -> Result<(), Box<Error>> {
        let ct_len = resp.headers().get::<ContentLength>().map(|ct_len| **ct_len);
        let (tx, rx) = mpsc::channel();
        for offsets in Self::get_chunk_sizes(ct_len.unwrap()) {
            let url = self.url.clone();
            let tx = tx.clone();
            thread::spawn(move || { Self::download_chunk(url, offsets, tx).unwrap(); });
        }
        let mut bytes_recv = 0;
        loop {
            if bytes_recv == ct_len.unwrap() {
                break;
            } else {
                let (byte_count, offset, buf) = rx.recv().unwrap();
                self.send_content(buf.as_slice(), Some(offset))?;
                bytes_recv += byte_count;
            }
        }
        Ok(())
    }

    pub fn events_hook<E: Events + 'static>(&mut self, hk: E) -> &mut HttpDownload {
        self.hooks.push(RefCell::new(Box::new(hk)));
        self
    }

    fn singlethread_download(&mut self, resp: &mut Response) -> Result<(), Box<Error>> {
        loop {
            let mut buffer = vec![0; self.chunk_sz];
            let bcount = resp.read(&mut buffer[..]).unwrap();
            buffer.truncate(bcount);
            if !buffer.is_empty() {
                self.send_content(buffer.as_slice(), None)?;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn send_content(&mut self, contents: &[u8], offset: Option<u64>) -> Result<(), Box<Error>> {
        for hk in &self.hooks {
            hk.borrow_mut().on_content(contents, offset)?;
        }

        Ok(())
    }
    fn get_reqwest_client() -> Result<(Client), Box<Error>> {
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
                      -> Result<(), Box<Error>> {
        let client = Self::get_reqwest_client()?;
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
}
