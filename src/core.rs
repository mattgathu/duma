use std::cell::RefCell;
use std::fmt;
use std::io::Read;
use std::sync::mpsc;
use std::time::Duration;

use failure::{format_err, Fallible};
use reqwest::blocking::{Client, Request};
use reqwest::header::{self, HeaderMap, HeaderValue};
use url::Url;

use threadpool::ThreadPool;

use ftp::FtpStream;

#[derive(Debug, Clone)]
pub struct Config {
    pub user_agent: String,
    pub resume: bool,
    pub headers: HeaderMap,
    pub file: String,
    pub timeout: u64,
    pub concurrent: bool,
    pub max_retries: i32,
    pub num_workers: usize,
    pub bytes_on_disk: Option<u64>,
    pub chunk_offsets: Option<Vec<(u64, u64)>>,
    pub chunk_size: u64,
}

#[allow(unused_variables)]
pub trait EventsHandler {
    fn on_resume_download(&mut self, bytes_on_disk: u64) {}

    fn on_headers(&mut self, headers: HeaderMap) {}

    fn on_content(&mut self, content: &[u8]) -> Fallible<()> {
        Ok(())
    }

    fn on_concurrent_content(&mut self, content: (u64, u64, &[u8])) -> Fallible<()> {
        Ok(())
    }

    fn on_content_length(&mut self, ct_len: u64) {}

    fn on_ftp_content_length(&mut self, ct_len: Option<u64>) {}

    fn on_success_status(&self) {}

    fn on_failure_status(&self, status_code: i32) {}

    fn on_finish(&mut self) {}

    fn on_max_retries(&mut self) {}

    fn on_server_supports_resume(&mut self) {}
}

pub struct FtpDownload {
    url: Url,
    hooks: Vec<RefCell<Box<dyn EventsHandler>>>,
}

impl FtpDownload {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            hooks: Vec::new(),
        }
    }

    pub fn download(&mut self) -> Fallible<()> {
        let ftp_server = format!(
            "{}:{}",
            self.url
                .host_str()
                .ok_or_else(|| format_err!("failed to parse hostname from url: {}", self.url))?,
            self.url
                .port_or_known_default()
                .ok_or_else(|| format_err!("failed to parse port from url: {}", self.url))?,
        );
        let username = if self.url.username().is_empty() {
            "anonymous"
        } else {
            self.url.username()
        };
        let password = self.url.password().unwrap_or("anonymous");

        let mut path_segments: Vec<&str> = self
            .url
            .path_segments()
            .ok_or_else(|| format_err!("failed to get url path segments: {}", self.url))?
            .collect();
        let ftp_fname = path_segments
            .pop()
            .ok_or_else(|| format_err!("got empty path segments from url: {}", self.url))?;

        let mut conn = FtpStream::connect(ftp_server)?;
        conn.login(username, password)?;
        for path in &path_segments {
            conn.cwd(path)?;
        }
        let ct_len = conn.size(ftp_fname)?;
        let mut reader = conn.get(ftp_fname)?;

        for hook in &self.hooks {
            let ct_len = ct_len.map(|x| x as u64);
            hook.borrow_mut().on_ftp_content_length(ct_len);
        }

        loop {
            let mut buffer = vec![0; 2048usize];
            let bcount = reader.read(&mut buffer[..])?;
            buffer.truncate(bcount);
            if !buffer.is_empty() {
                self.send_content(buffer.as_slice())?;
            } else {
                break;
            }
        }

        for hook in &self.hooks {
            hook.borrow_mut().on_finish();
        }

        Ok(())
    }

    fn send_content(&self, contents: &[u8]) -> Fallible<()> {
        for hk in &self.hooks {
            hk.borrow_mut().on_content(contents)?;
        }
        Ok(())
    }
    pub fn events_hook<E: EventsHandler + 'static>(&mut self, hk: E) -> &mut FtpDownload {
        self.hooks.push(RefCell::new(Box::new(hk)));
        self
    }
}

pub struct HttpDownload {
    url: Url,
    hooks: Vec<RefCell<Box<dyn EventsHandler>>>,
    conf: Config,
    retries: i32,
    client: Client,
}

impl fmt::Debug for HttpDownload {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "HttpDownload url: {}", self.url)
    }
}

impl HttpDownload {
    pub fn new(url: Url, conf: Config) -> HttpDownload {
        HttpDownload {
            url,
            hooks: Vec::new(),
            conf,
            retries: 0,
            client: Client::new(),
        }
    }

    pub fn download(&mut self) -> Fallible<()> {
        let resp = self
            .client
            .get(self.url.as_ref())
            .timeout(Duration::from_secs(self.conf.timeout))
            .headers(self.conf.headers.clone())
            .header(
                header::USER_AGENT,
                HeaderValue::from_str(&self.conf.user_agent)?,
            )
            .send()?;
        let headers = resp.headers();

        let server_supports_bytes = match headers.get(header::ACCEPT_RANGES) {
            Some(val) => val == "bytes",
            None => false,
        };

        if server_supports_bytes && self.conf.headers.contains_key(header::RANGE) {
            if self.conf.concurrent {
                self.conf.headers.remove(header::RANGE);
            }
            for hook in &self.hooks {
                hook.borrow_mut().on_server_supports_resume();
            }
        }

        let req = self
            .client
            .get(self.url.as_ref())
            .timeout(Duration::from_secs(self.conf.timeout))
            .headers(self.conf.headers.clone())
            .build()?;

        for hk in &self.hooks {
            hk.borrow_mut().on_headers(headers.clone());
        }
        if server_supports_bytes
            && self.conf.concurrent
            && headers.contains_key(header::CONTENT_LENGTH)
        {
            self.concurrent_download(req, headers.get(header::CONTENT_LENGTH).unwrap())?;
        } else {
            self.singlethread_download(req)?;
        }

        for hook in &self.hooks {
            hook.borrow_mut().on_finish();
        }

        Ok(())
    }

    pub fn events_hook<E: EventsHandler + 'static>(&mut self, hk: E) -> &mut HttpDownload {
        self.hooks.push(RefCell::new(Box::new(hk)));
        self
    }

    fn singlethread_download(&mut self, req: Request) -> Fallible<()> {
        let mut resp = self.client.execute(req)?;
        let ct_len = if let Some(val) = resp.headers().get(header::CONTENT_LENGTH) {
            Some(val.to_str()?.parse::<usize>()?)
        } else {
            None
        };
        let mut cnt = 0;
        loop {
            let mut buffer = vec![0; self.conf.chunk_size as usize];
            let bcount = resp.read(&mut buffer[..])?;
            cnt += bcount;
            buffer.truncate(bcount);
            if !buffer.is_empty() {
                self.send_content(buffer.as_slice())?;
            } else {
                break;
            }
            if Some(cnt) == ct_len {
                break;
            }
        }
        Ok(())
    }

    pub fn concurrent_download(&mut self, req: Request, ct_val: &HeaderValue) -> Fallible<()> {
        let (data_tx, data_rx) = mpsc::channel();
        let (errors_tx, errors_rx) = mpsc::channel();
        let ct_len = ct_val.to_str()?.parse::<u64>()?;
        let chunk_offsets = self
            .conf
            .chunk_offsets
            .clone()
            .unwrap_or_else(|| self.get_chunk_offsets(ct_len, self.conf.chunk_size));
        let worker_pool = ThreadPool::new(self.conf.num_workers);
        for offsets in chunk_offsets {
            let data_tx = data_tx.clone();
            let errors_tx = errors_tx.clone();
            let req = req.try_clone().unwrap();
            worker_pool.execute(move || download_chunk(req, offsets, data_tx.clone(), errors_tx))
        }

        let mut count = self.conf.bytes_on_disk.unwrap_or(0);
        loop {
            if count == ct_len {
                break;
            }
            let (byte_count, offset, buf) = data_rx.recv()?;
            count += byte_count;
            for hk in &self.hooks {
                hk.borrow_mut()
                    .on_concurrent_content((byte_count, offset, &buf))?;
            }
            match errors_rx.recv_timeout(Duration::from_micros(1)) {
                Err(_) => {}
                Ok(offsets) => {
                    if self.retries > self.conf.max_retries {
                        for hk in &self.hooks {
                            hk.borrow_mut().on_max_retries();
                        }
                    }
                    self.retries += 1;
                    let data_tx = data_tx.clone();
                    let errors_tx = errors_tx.clone();
                    let req = req.try_clone().unwrap();
                    worker_pool.execute(move || download_chunk(req, offsets, data_tx, errors_tx))
                }
            }
        }
        Ok(())
    }

    fn get_chunk_offsets(&self, ct_len: u64, chunk_size: u64) -> Vec<(u64, u64)> {
        let no_of_chunks = ct_len / chunk_size;
        let mut sizes = Vec::new();

        for chunk in 0..no_of_chunks {
            let bound = if chunk == no_of_chunks - 1 {
                ct_len
            } else {
                ((chunk + 1) * chunk_size) - 1
            };
            sizes.push((chunk * chunk_size, bound));
        }
        if sizes.is_empty() {
            sizes.push((0, ct_len));
        }

        sizes
    }

    fn send_content(&mut self, contents: &[u8]) -> Fallible<()> {
        for hk in &self.hooks {
            hk.borrow_mut().on_content(contents)?;
        }

        Ok(())
    }
}

fn download_chunk(
    req: Request,
    offsets: (u64, u64),
    sender: mpsc::Sender<(u64, u64, Vec<u8>)>,
    errors: mpsc::Sender<(u64, u64)>,
) {
    fn inner(
        mut req: Request,
        offsets: (u64, u64),
        sender: mpsc::Sender<(u64, u64, Vec<u8>)>,
        start_offset: &mut u64,
    ) -> Fallible<()> {
        let byte_range = format!("bytes={}-{}", offsets.0, offsets.1);
        let headers = req.headers_mut();
        headers.insert(header::RANGE, HeaderValue::from_str(&byte_range)?);
        headers.insert(header::ACCEPT, HeaderValue::from_str("*/*")?);
        headers.insert(header::CONNECTION, HeaderValue::from_str("keep-alive")?);
        let mut resp = Client::new().execute(req)?;
        let chunk_sz = offsets.1 - offsets.0;
        let mut cnt = 0u64;
        loop {
            let mut buf = vec![0; chunk_sz as usize];
            let byte_count = resp.read(&mut buf[..])?;
            cnt += byte_count as u64;
            buf.truncate(byte_count);
            if !buf.is_empty() {
                sender.send((byte_count as u64, *start_offset, buf.clone()))?;
                *start_offset += byte_count as u64;
            } else {
                break;
            }
            if cnt == (chunk_sz + 1) {
                break;
            }
        }

        Ok(())
    }
    let mut start_offset = offsets.0;
    let end_offset = offsets.1;
    match inner(req, offsets, sender, &mut start_offset) {
        Ok(_) => {}
        Err(_) => match errors.send((start_offset, end_offset)) {
            _ => {}
        },
    }
}
