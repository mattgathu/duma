use std::env;
use std::fmt;
use std::error::Error;
use std::cell::RefCell;
use std::io::Read;

use reqwest::{Client, Proxy, Response, StatusCode, Url};
use reqwest::header::{AcceptRanges, Headers, Range, RangeUnit};

use ftp::FtpStream;


#[allow(unused_variables)]
pub trait Events {
    fn on_resume_download(&mut self, bytes_on_disk: u64) {}

    fn on_headers(&mut self, headers: Headers) {}

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
    headers: Headers,
}

impl fmt::Debug for HttpDownload {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "HttpDownload url: {}", self.url)
    }
}

impl HttpDownload {
    pub fn new(url: Url, headers: Headers) -> HttpDownload {
        HttpDownload {
            url: url,
            chunk_sz: 1024,
            hooks: Vec::new(),
            headers: headers,
        }
    }

    pub fn download(&mut self) -> Result<(), Box<Error>> {
        let client = Self::get_reqwest_client()?;
        let mut req = client.get(self.url.clone())?;
        let head_resp = client.head(self.url.clone())?
            .send()?;

        let server_supports_bytes = match head_resp.headers().get::<AcceptRanges>() {
            Some(header) => header.contains(&RangeUnit::Bytes),
            None => false,
        };
        if server_supports_bytes {
            if let Some(hdr) = self.headers.clone().get::<Range>() {
                    req.header(hdr.clone());
                    self.headers.remove::<Range>();
                    for hook in &self.hooks {
                        hook.borrow_mut().on_server_supports_resume();
                    }
            };
        }

        req.headers(self.headers.clone());

        let mut resp = req.send()?;

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
            if !buffer.is_empty() {
                self.send_content(buffer.as_slice())?;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn send_content(&mut self, contents: &[u8]) -> Result<(), Box<Error>> {
        for hk in &self.hooks {
            hk.borrow_mut().on_content(contents)?;
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
}
