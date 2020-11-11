extern crate tiny_http;
use self::tiny_http::{Header, Request, Response, Server};
use std::fs::File;
use std::io::Error;
use std::sync::Arc;
use std::sync::Once;
use std::thread;
use std::time::Duration;

static INIT: Once = Once::new();

pub fn setup() {
    INIT.call_once(|| {
        let server = Arc::new(Server::http("0.0.0.0:35550").unwrap());
        for _ in 0..4 {
            let server = server.clone();

            thread::spawn(move || loop {
                let request = server.recv().unwrap();
                handle_req(request).unwrap();
            });
        }
    });
}

fn handle_req(req: Request) -> Result<(), Error> {
    match req.url() {
        "/headers" => respond_with_headers(req),
        "/timeout" => respond_with_timeout(req),
        "/file" => respond_with_file(req),
        "/content-disposition" => respond_with_content_disposition(req),
        _ => respond_with_headers(req),
    }
}

fn respond_with_headers(req: Request) -> Result<(), Error> {
    let res = Response::empty(200)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"text/plain"[..]).unwrap());

    req.respond(res)
}

fn respond_with_timeout(req: Request) -> Result<(), Error> {
    thread::sleep(Duration::from_secs(5));
    let res = Response::empty(200);
    req.respond(res)
}

fn respond_with_file(req: Request) -> Result<(), Error> {
    let mut path = std::env::current_dir()?;
    path.push("tests");
    path.push("foo.txt");
    let f = File::open(path)?;
    let len = f.metadata()?.len();
    let ctype = "Content-Type: text/plain".parse::<Header>().unwrap();
    let clength = format!("Content-Length: {}", len)
        .parse::<Header>()
        .unwrap();
    req.respond(
        Response::from_file(f)
            .with_header(ctype)
            .with_header(clength),
    )
}

fn respond_with_content_disposition(req: Request) -> Result<(), Error> {
    let mut path = std::env::current_dir()?;
    path.push("tests");
    path.push("foo.txt");
    let f = File::open(path)?;
    let len = f.metadata()?.len();
    let ctype = "Content-Type: text/plain".parse::<Header>().unwrap();
    let cdisp = "Content-Disposition: attachment; filename=\"renamed.txt\""
        .parse::<Header>()
        .unwrap();
    let clength = format!("Content-Length: {}", len)
        .parse::<Header>()
        .unwrap();
    req.respond(
        Response::from_file(f)
            .with_header(ctype)
            .with_header(cdisp)
            .with_header(clength),
    )
}
