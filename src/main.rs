extern crate clap;
extern crate console;
extern crate reqwest;
extern crate indicatif;

use std::fs::File;
use std::io::Read;
use std::io::copy;
use reqwest::Client;
use reqwest::header::ContentLength;
use indicatif::{ProgressBar, ProgressStyle};
use clap::{Arg, App};
use console::style;

fn download(target: &str) {

    let client = Client::new().unwrap();
    let mut resp = client.get(target)
        .unwrap()
        .send()
        .unwrap();
    if resp.status().is_success() {

        let len = resp.headers()
            .get::<ContentLength>()
            .map(|ct_len| **ct_len)
            .unwrap_or(0);

        let fname = target.split("/").last().unwrap();
        println!("Downloading: '{}' Size: {}b",
                 style(fname).bold(),
                 style(len).bold());

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

        let mut file = File::create(fname).unwrap();
        copy(&mut buf.as_slice(), &mut file).unwrap();
    }

}


fn main() {
    let matches = App::new("My Simple Downloader")
        .version("0.1.0")
        .author("Matt Gathu <mattgathu@gmail.com>")
        .about("Progressful downloader")
        .arg(Arg::with_name("url")
                 .required(true)
                 .short("u")
                 .long("url")
                 .takes_value(true)
                 .help("url to download"))
        .get_matches();
    let url = matches.value_of("url").unwrap();
    download(url);
}
