//! An example of extracting a file in an archive.
//!
//! Takes a tarball on standard input, looks for an entry with a listed file
//! name as the first argument provided, and then prints the contents of that
//! file to stdout.

extern crate tokio_tar as async_tar;

use std::{env::args_os, path::Path};
use tokio::io::{copy, stdin, stdout};
use tokio_stream::*;

use async_tar::Archive;

fn main() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let first_arg = args_os().nth(1).unwrap();
        let filename = Path::new(&first_arg);
        let mut ar = Archive::new(stdin());
        let mut entries = ar.entries().unwrap();
        while let Some(file) = entries.next().await {
            let mut f = file.unwrap();
            if f.path().unwrap() == filename {
                copy(&mut f, &mut stdout()).await.unwrap();
            }
        }
    });
}
