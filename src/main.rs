extern crate clap;
extern crate console;
extern crate reqwest;
extern crate indicatif;

use std::fs::File;
use std::io::Read;
use std::io::copy;
use reqwest::{Client, Url, UrlError};
use reqwest::header::{ContentLength, ContentType};
use indicatif::{ProgressBar, ProgressStyle, HumanBytes};
use clap::{Arg, App};
use console::style;

fn parse_url(url: &str) -> Result<Url, UrlError> {
    match Url::parse(url) {
        Ok(url) => Ok(url),
        Err(error) if error == UrlError::RelativeUrlWithoutBase => {
            let url_with_base = format!("{}{}", "http://", url);
            Url::parse(url_with_base.as_str())
        }
        Err(error) => return Err(error),
    }

}

fn download(target: &str) -> Result<(), Box<::std::error::Error>> {

    // parse url
    let url = parse_url(target)?;
    let client = Client::new().unwrap();
    let mut resp = client.get(url)?
        .send()
        .unwrap();
    println!("HTTP request sent... {}",
             style(format!("{}", resp.status())).green());
    if resp.status().is_success() {

        let headers = resp.headers().clone();
        let len = headers.get::<ContentLength>().map(|ct_len| **ct_len).unwrap_or(0);

        let ct_type = headers.get::<ContentType>().unwrap();

        println!("Length: {} ({})",
                 style(len).green(),
                 style(format!("{}", HumanBytes(len))).red());
        println!("Type: {}", style(ct_type).green());

        let fname = target.split("/").last().unwrap();

        println!("Saving to: {}", style(fname).green());

        let chunk_len = len as usize / 99;
        let mut buf = Vec::new();
        let mut byte_count: Vec<usize> = Vec::new();
        let bar = ProgressBar::new(len as u64);
        bar.set_message(fname);
        bar.set_style(ProgressStyle::default_bar()
            .template("{msg} {spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} eta: {eta}")
            .progress_chars("=> "));

        loop {
            let mut buffer = vec![0; chunk_len];
            let bcount = resp.read(&mut buffer[..]).unwrap();
            byte_count.push(bcount.clone());
            buffer.truncate(bcount);
            if !buffer.is_empty() {
                buf.extend(buffer.into_boxed_slice()
                               .into_vec()
                               .iter()
                               .cloned());
                bar.inc(bcount as u64);
            } else {
                break;
            }
        }

        bar.finish();

        save_to_file(&mut buf, fname)?;
    }

    Ok(())

}

fn save_to_file(contents: &mut Vec<u8>, fname: &str) -> Result<(), std::io::Error> {
    let mut file = File::create(fname).unwrap();
    copy(&mut contents.as_slice(), &mut file).unwrap();
    Ok(())

}

fn main() {
    let matches = App::new("My Simple Downloader")
        .version("0.1.0")
        .author("Matt Gathu <mattgathu@gmail.com>")
        .about("Progressful downloader")
        .arg(Arg::with_name("URL")
                 .required(true)
                 .takes_value(true)
                 .index(1)
                 .help("url to download"))
        .get_matches();
    let url = matches.value_of("URL").unwrap();
    match download(url) {
        Ok(_) => println!("Download Successful!"),
        Err(e) => println!("Got error: {}", e.description()),
    }
}
