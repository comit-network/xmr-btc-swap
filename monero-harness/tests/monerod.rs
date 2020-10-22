use monero_harness::Monero;
use spectral::prelude::*;
use testcontainers::clients::Cli;

fn init_cli() -> Cli {
    Cli::default()
}

#[tokio::test]
async fn connect_to_monerod() {
    let tc = init_cli();
    let (monero, _container) = Monero::new(&tc);
    let cli = monero.monerod_rpc_client();

    let header = cli
        .get_block_header_by_height(0)
        .await
        .expect("failed to get block 0");

    assert_that!(header.height).is_equal_to(0);
}
