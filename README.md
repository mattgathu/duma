## duma

[![Build Status](https://travis-ci.org/mattgathu/duma.svg?branch=master)](https://travis-ci.org/mattgathu/duma)
[![Build status](https://ci.appveyor.com/api/projects/status/007cmm9c6c9onai9?svg=true)](https://ci.appveyor.com/project/mattgathu/duma)

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
    -c, --continue    resume getting a partially-downloaded file
    -h, --help        Prints help information
    -q, --quiet       quiet (no output)
    -V, --version     Prints version information

OPTIONS:
    -O, --output-document <FILE>    write documents to FILE

ARGS:
    <URL>    url to download

```

## Installation

Via cargo

```
Cargo install duma
```

## screenshot

![screenshot](screenshot.png)

## license

This project is license used the MIT license. See [LICENSE](LICENSE) for more details.
