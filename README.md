## duma

[![Build Status](https://travis-ci.org/mattgathu/duma.svg?branch=master)](https://travis-ci.org/mattgathu/duma)
[![Build status](https://ci.appveyor.com/api/projects/status/007cmm9c6c9onai9?svg=true)](https://ci.appveyor.com/project/mattgathu/duma)
[![Build status](https://github.com/mattgathu/duma/workflows/CI/badge.svg?branch=master)](https://github.com/mattgathu/duma/actions)

A minimal file downloader written in Rust.

## features

* support for **http** and **https** downloads
* support for **ftp** downloads
* Download **resume** capability
* download **progress bar**

## usage

```
Duma 0.1.0
Matt Gathu <mattgathu@gmail.com>
A minimal file downloader

USAGE:
    duma [FLAGS] [OPTIONS] <URL>

FLAGS:
    -c, --continue        resume getting a partially-downloaded file
    -h, --help            Prints help information
    -H, --headers         prints the headers sent by the HTTP server
    -q, --quiet           quiet (no output)
    -s, --singlethread    download using only a single thread
    -V, --version         Prints version information

OPTIONS:
    -U, --useragent <AGENT>                    identify as AGENT instead of Duma/VERSION
    -O, --output <FILE>                        write documents to FILE
    -n, --num_connections <NUM_CONNECTIONS>    maximum number of concurrent connections (default is 8)
    -T, --timeout <SECONDS>                    set all timeout values to SECONDS

ARGS:
    <URL>    url to download

```

## Installation

Via cargo

```
cargo install duma
```

## screenshot

![screenshot](screenshot.png)

## license

This project is license used the MIT license. See [LICENSE](LICENSE) for more details.
