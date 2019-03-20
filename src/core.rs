use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::sync::mpsc;
use std::time::Duration;

use failure::{bail, format_err, Fallible};
use reqwest::header::{HeaderMap, ACCEPT_RANGES, CONTENT_LENGTH, RANGE};
use reqwest::{Client, Proxy, Response, StatusCode, Url};

use threadpool::ThreadPool;

use ftp::FtpStream;

#[derive(Debug, Clone)]
pub struct Config {
    pub user_agent: String,
    pub resume: bool,
    pub headers: HeaderMap,
    pub file: String,
    pub timeout: Option<Duration>,
    pub concurrent: bool,
    pub proxies: Option<HashMap<String, String>>,
    pub max_retries: i32,
    pub bytes_on_disk: Option<u64>,
    pub chunk_sizes: Option<Vec<(u64, u64)>>,
    pub chunk_sz: usize,
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

    fn on_failure_status(&self, status_code: StatusCode) {}

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
            self.url.host_str().ok_or(format_err!(
                "failed to parse hostname from url: {}",
                self.url
            ))?,
            self.url.port_or_known_default().ok_or(format_err!(
                "failed to parse port from url: {}",
                self.url
            ))?,
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
            .ok_or(format_err!("failed to get url path segments: {}", self.url))?
            .collect();
        let ftp_fname = path_segments.pop().ok_or(format_err!(
            "got empty path segments from url: {}",
            self.url
        ))?;

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
    opts: Config,
    retries: i32,
}

impl fmt::Debug for HttpDownload {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "HttpDownload url: {}", self.url)
    }
}

impl HttpDownload {
    pub fn new(url: Url, opts: Config) -> HttpDownload {
        HttpDownload {
            url,
            hooks: Vec::new(),
            opts,
            retries: 0,
        }
    }

    pub fn download(&mut self) -> Fallible<()> {
        let client = self.get_reqwest_client()?;
        let mut req = client.get(self.url.clone());
        let head_resp = client.head(self.url.clone()).send()?;

        let server_supports_bytes = match head_resp.headers().get(ACCEPT_RANGES) {
            Some(val) => {
                if let Ok(unit) = val.to_str() {
                    unit == "bytes"
                } else {
                    false
                }
            }
            None => false,
        };
        if server_supports_bytes {
            if let Some(range) = self.opts.headers.clone().get(RANGE) {
                if !self.opts.concurrent {
                    req = req.header(RANGE, range);
                }
                self.opts.headers.remove(RANGE);
                for hook in &self.hooks {
                    hook.borrow_mut().on_server_supports_resume();
                }
            };
        }

        req = req.headers(self.opts.headers.clone());

        let mut resp = req.send()?;

        if resp.status().is_success() {
            let headers = head_resp.headers();
            for hk in &self.hooks {
                hk.borrow_mut().on_headers(headers.clone());
            }
            if server_supports_bytes && self.opts.concurrent {
                self.concurrent_download(client, &headers)?;
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

    pub fn events_hook<E: EventsHandler + 'static>(&mut self, hk: E) -> &mut HttpDownload {
        self.hooks.push(RefCell::new(Box::new(hk)));
        self
    }

    fn singlethread_download(&mut self, resp: &mut Response) -> Fallible<()> {
        loop {
            let mut buffer = vec![0; self.opts.chunk_sz];
            let bcount = resp.read(&mut buffer[..])?;
            buffer.truncate(bcount);
            if !buffer.is_empty() {
                self.send_content(buffer.as_slice())?;
            } else {
                break;
            }
        }
        Ok(())
    }

    pub fn concurrent_download(&mut self, client: Client, headers: &HeaderMap) -> Fallible<()> {
        let (data_tx, data_rx) = mpsc::channel();
        let (errors_tx, errors_rx) = mpsc::channel();
        let ct_len = if let Some(val) = headers.get(CONTENT_LENGTH) {
            val.to_str()?.parse::<u64>()?
        } else {
            bail!("concurrent download: server did not return content-length header")
        };
        let n_workers = 8;
        let chunk_sizes = self
            .opts
            .chunk_sizes
            .clone()
            .unwrap_or_else(|| self.get_chunk_sizes(ct_len));
        let worker_pool = ThreadPool::new(n_workers);
        for offsets in chunk_sizes {
            let data_tx = data_tx.clone();
            let errors_tx = errors_tx.clone();
            let url = self.url.clone();
            let client = client.clone();
            worker_pool
                .execute(move || download_chunk(client, url, offsets, data_tx.clone(), errors_tx))
        }

        let threshold = ct_len - self.opts.bytes_on_disk.unwrap_or(0);
        let mut count = 0;
        loop {
            if count == threshold {
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
                    if self.retries > self.opts.max_retries {
                        for hk in &self.hooks {
                            hk.borrow_mut().on_max_retries();
                        }
                    }
                    self.retries += 1;
                    let data_tx = data_tx.clone();
                    let errors_tx = errors_tx.clone();
                    let url = self.url.clone();
                    let client = client.clone();
                    worker_pool
                        .execute(move || download_chunk(client, url, offsets, data_tx, errors_tx))
                }
            }
        }
        worker_pool.join();
        Ok(())
    }

    fn get_chunk_sizes(&self, ct_len: u64) -> Vec<(u64, u64)> {
        let chunk_size = 512_000;
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

    fn get_reqwest_client(&self) -> Fallible<(Client)> {
        let mut builder = Client::builder();

        if let Some(proxies) = self.opts.proxies.clone() {
            if let Some(http_proxy) = proxies.get("http_proxy") {
                builder = builder.proxy(Proxy::http(Url::parse(http_proxy)?)?);
            }

            if let Some(https_proxy) = proxies.get("https_proxy") {
                builder = builder.proxy(Proxy::https(Url::parse(https_proxy)?)?);
            }
        };

        if let Some(secs) = self.opts.timeout {
            builder = builder.timeout(secs);
        }

        Ok(builder.build()?)
    }
}

fn download_chunk(
    client: Client,
    url: Url,
    offsets: (u64, u64),
    sender: mpsc::Sender<(u64, u64, Vec<u8>)>,
    errors: mpsc::Sender<(u64, u64)>,
) {
    fn _download_chunk(
        client: Client,
        url: Url,
        offsets: (u64, u64),
        sender: mpsc::Sender<(u64, u64, Vec<u8>)>,
        start_offset: &mut u64,
    ) -> Fallible<()> {
        let byte_range = format!("bytes={}-{}", offsets.0, offsets.1);
        let mut resp = client.get(url).header(RANGE, byte_range).send()?;
        let chunk_sz = offsets.1 - offsets.0;
        loop {
            let mut buf = vec![0; chunk_sz as usize];
            let byte_count = resp.read(&mut buf[..])?;
            buf.truncate(byte_count);
            if !buf.is_empty() {
                sender.send((byte_count as u64, *start_offset, buf.clone()))?;
                *start_offset += byte_count as u64;
            } else {
                break;
            }
        }

        Ok(())
    }
    let mut start_offset = offsets.0;
    let end_offset = offsets.1;
    match _download_chunk(client, url, offsets, sender, &mut start_offset) {
        Ok(_) => {}
        Err(_) => match errors.send((start_offset, end_offset)) {
            _ => {}
        },
    }
}
