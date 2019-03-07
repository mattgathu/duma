use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::io::{BufReader, Read};
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT_RANGES, RANGE};
use reqwest::{Client, ClientBuilder, Proxy, RequestBuilder, Response, StatusCode, Url};

use ftp::FtpStream;

#[allow(unused_variables)]
pub trait Events {
    fn on_resume_download(&mut self, bytes_on_disk: u64) {}

    fn on_headers(&mut self, headers: HeaderMap) {}

    fn on_content(&mut self, content: &[u8]) -> Result<(), Box<Error>> {
        Ok(())
    }

    fn on_content_length(&mut self, ct_len: u64) {}

    fn on_ftp_content_length(&mut self, ct_len: Option<u64>) {}

    fn on_success_status(&self) {}

    fn on_failure_status(&self, status_code: StatusCode) {}

    fn on_finish(&mut self) {}

    fn on_server_supports_resume(&mut self) {}
}

pub struct FtpDownload {
    url: Url,
    hooks: Vec<RefCell<Box<Events>>>,
}

impl FtpDownload {
    pub fn new(url: Url) -> Self {
        Self {
            url,
            hooks: Vec::new(),
        }
    }

    pub fn download(&mut self) -> Result<(), Box<Error>> {
        let ftp_server = format!(
            "{}:{}",
            self.url.host_str().unwrap(),
            self.url.port_or_known_default().unwrap()
        );
        let username = if self.url.username().is_empty() {
            "anonymous"
        } else {
            self.url.username()
        };
        let password = self.url.password().unwrap_or("anonymous");

        let mut path_segments: Vec<&str> = self.url.path_segments().unwrap().collect();
        let ftp_fname = path_segments.pop().unwrap();

        let mut conn = FtpStream::connect(ftp_server)?;
        conn.login(username, password)?;
        for path in &path_segments {
            conn.cwd(path)?;
        }
        let ct_len = conn.size(ftp_fname)?;
        let mut reader: BufReader<_> = conn.get(ftp_fname)?;

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
            if buffer.is_empty() {
                break;
            }
            self.send_content(buffer.as_slice())?;
        }

        for hook in &self.hooks {
            hook.borrow_mut().on_finish();
        }

        Ok(())
    }

    fn send_content(&self, contents: &[u8]) -> Result<(), Box<Error>> {
        for hk in &self.hooks {
            hk.borrow_mut().on_content(contents)?;
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
    chunk_sz: usize,
    hooks: Vec<RefCell<Box<Events>>>,
    headers: HeaderMap,
    timeout: Option<Duration>,
    proxies: Option<HashMap<String, String>>,
}

impl fmt::Debug for HttpDownload {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "HttpDownload url: {}", self.url)
    }
}

impl HttpDownload {
    pub fn new(
        url: Url,
        headers: HeaderMap,
        timeout: Option<Duration>,
        proxies: Option<HashMap<String, String>>,
    ) -> HttpDownload {
        HttpDownload {
            url,
            chunk_sz: 1024,
            hooks: Vec::new(),
            headers,
            timeout,
            proxies,
        }
    }

    pub fn download(&mut self) -> Result<(), Box<Error>> {
        let client: Client = self.get_reqwest_client()?;
        let mut req: RequestBuilder = client.get(self.url.clone());
        let head_resp: Response = client.head(self.url.clone()).send()?;

        let accept_ranges: Option<&HeaderValue> = head_resp.headers().get(ACCEPT_RANGES);
        let server_supports_bytes = match accept_ranges {
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
            if let Some(range) = self.headers.clone().get(RANGE) {
                req = req.header(RANGE, range);
                self.headers.remove(RANGE);
                for hook in &self.hooks {
                    hook.borrow_mut().on_server_supports_resume();
                }
            }
        }

        let mut resp = req.headers(self.headers.clone()).send()?;
        if resp.status().is_success() {
            let headers = head_resp.headers();
            for hk in &self.hooks {
                hk.borrow_mut().on_headers(headers.clone());
            }
            self.singlethread_download(&mut resp)?;
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

    pub fn events_hook<E: Events + 'static>(&mut self, hk: E) -> &mut HttpDownload {
        self.hooks.push(RefCell::new(Box::new(hk)));
        self
    }

    fn singlethread_download(&mut self, resp: &mut Response) -> Result<(), Box<Error>> {
        loop {
            let mut buffer = vec![0; self.chunk_sz];
            let bcount = resp.read(&mut buffer[..]).unwrap();
            buffer.truncate(bcount);
            if buffer.is_empty() {
                break;
            }
            self.send_content(buffer.as_slice())?;
        }
        Ok(())
    }

    fn send_content(&mut self, contents: &[u8]) -> Result<(), Box<Error>> {
        for hk in &self.hooks {
            hk.borrow_mut().on_content(contents)?;
        }

        Ok(())
    }

    fn get_reqwest_client(&self) -> Result<(Client), Box<Error>> {
        let mut builder: ClientBuilder = Client::builder();

        if let Some(proxies) = self.proxies.clone() {
            if let Some(http_proxy) = proxies.get("http_proxy") {
                builder = builder.proxy(Proxy::http(Url::parse(http_proxy)?)?);
            }

            if let Some(https_proxy) = proxies.get("https_proxy") {
                builder = builder.proxy(Proxy::https(Url::parse(https_proxy)?)?);
            }
        }

        if let Some(secs) = self.timeout {
            builder = builder.timeout(secs);
        }

        Ok(builder.build()?)
    }
}
