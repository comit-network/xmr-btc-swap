//! An example of listing the file names of entries in an archive.
//!
//! Takes a tarball on stdin and prints out all of the entries inside.

extern crate tokio_tar as async_tar;

use tokio::io::stdin;
use tokio_stream::*;

use async_tar::Archive;

fn main() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut ar = Archive::new(stdin());
        let mut entries = ar.entries().unwrap();
        while let Some(file) = entries.next().await {
            let f = file.unwrap();
            println!("{}", f.path().unwrap().display());
        }
    });
}
