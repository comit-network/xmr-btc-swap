extern crate tokio_tar as async_tar;

use async_tar::Builder;
use tokio::fs::File;

fn main() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let file = File::create("foo.tar").await.unwrap();
        let mut a = Builder::new(file);

        a.append_path("README.md").await.unwrap();
        a.append_file("lib.rs", &mut File::open("src/lib.rs").await.unwrap())
            .await
            .unwrap();
    });
}
