use std::fs;
use std::io::Read;
use std::io::Write;
use std::io::Seek;
use std::thread;
use std::sync::mpsc;
use std::io::BufWriter;
use std::io::SeekFrom;
use reqwest::{Client, Url};
use reqwest::header::{Range, ByteRangeSpec, ContentLength, ContentType, AcceptRanges, RangeUnit};
use indicatif::HumanBytes;
use console::style;
use num_cpus::get as get_cpus_num;

use utils::{get_file_handle, print};
use bar::create_progress_bar;


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
    let client = Client::new().unwrap();
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
            &url.path()
                 .split('/')
                 .last()
                 .unwrap()
        }
    };

    let client = Client::new().unwrap();
    let mut resp = if resume_download {
        let req_headers = client.head(url.clone())?
            .send()?
            .headers()
            .clone();
        match req_headers.get::<AcceptRanges>() {
            Some(header) => {
                if header.contains(&RangeUnit::Bytes) {
                    let byte_count = match fs::metadata(fname) {
                        Ok(metadata) => metadata.len(),
                        Err(_) => 0u64,
                    };
                    // if byte_count is zero don't pass range header
                    match byte_count {
                        0 => {
                            client.get(url.clone())?
                                .send()?
                        }
                        _ => {
                            let byte_range = Range::Bytes(vec![ByteRangeSpec::AllFrom(byte_count)]);
                            client.get(url.clone())?
                                .header(byte_range)
                                .send()?
                        }
                    }
                } else {
                    client.get(url.clone())?
                        .send()?
                }
            }
            None => {
                client.get(url.clone())?
                    .send()?
            }
        }

        //client.get(url)?.send()?

    } else {
        client.get(url.clone())?
            .send()?
    };
    print(&format!("HTTP request sent... {}",
                   style(format!("{}", resp.status())).green()),
          quiet_mode,
          false);
    if resp.status().is_success() {

        let headers = if resume_download {
            resp.headers().clone()
        } else {
            client.get(url.clone())?
                .send()?
                .headers()
                .clone()
        };
        let ct_len = headers.get::<ContentLength>().map(|ct_len| **ct_len);

        let ct_type = headers.get::<ContentType>().unwrap();

        match ct_len {
            Some(len) => {
                print(&format!("Length: {} ({})",
                               style(len).green(),
                               style(format!("{}", HumanBytes(len))).red()),
                      quiet_mode,
                      false);
            }
            None => {
                print(&format!("Length: {}", style("unknown").red()),
                      quiet_mode,
                      false);
            }
        }

        print(&format!("Type: {}", style(ct_type).green()),
              quiet_mode,
              false);

        print(&format!("Saving to: {}", style(fname).green()),
              quiet_mode,
              false);

        let chunk_size = match ct_len {
            Some(x) => x as usize / 99,
            None => 1024usize, // default chunk size
        };

        let out_file = get_file_handle(fname, resume_download)?;
        let mut writer = BufWriter::new(out_file);

        let pbar = create_progress_bar(quiet_mode, fname, ct_len);

        // if resuming download, update progress bar
        if resume_download {
            let metadata = fs::metadata(fname)?;
            pbar.inc(metadata.len());
        }

        if ct_len.is_some() && multithread {
            let (tx, rx) = mpsc::channel();
            for offsets in get_chunk_sizes(ct_len.unwrap()) {
                let url = url.clone();
                let tx = tx.clone();
                thread::spawn(move || { download_chunk(url, offsets, tx).unwrap(); });
            }

            let mut progress_state = 0;

            loop {
                if progress_state == ct_len.unwrap() {
                    break;
                } else {
                    let (byte_count, offset, buf) = rx.recv().unwrap();
                    writer.seek(SeekFrom::Start(offset))?;
                    writer.write_all(buf.as_slice()).unwrap();
                    progress_state += byte_count;
                    pbar.inc(byte_count);
                }
            }

        } else {

            loop {
                let mut buffer = vec![0; chunk_size];
                let bcount = resp.read(&mut buffer[..]).unwrap();
                buffer.truncate(bcount);
                if !buffer.is_empty() {
                    writer.write_all(buffer.as_slice()).unwrap();
                    pbar.inc(bcount as u64);
                } else {
                    break;
                }
            }
        }

        pbar.finish();

    } else if resp.status().as_u16() == 416 {
        print(&style("\nThe file is already fully retrieved; nothing to do.\n").red(),
              quiet_mode,
              false);
    }

    Ok(())

}
