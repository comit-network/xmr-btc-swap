use tokio::join;

pub mod testutils;

/// Run the following tests with RUST_MIN_STACK=10000000

#[tokio::test]
async fn happy_path() {
    testutils::test(|alice, bob| async move {
        join!(alice.swap(), bob.swap());

        alice.assert_btc_redeemed();
        bob.assert_btc_redeemed();
    })
    .await;
}

// #[tokio::test]
// async fn happy_path() {
//     testutils::test(|alice_node, bob_node| async move {
//         let alice_start_state = unimplemented!();
//         let bob_start_state = unimplemented!();
//
//         let (alice_end_state, bob_end_state) =
//             join!(alice_node.swap(alice_start_state), bob_node.swap(bo));
//
//         alice_node.assert_btc_redeemed(alice_end_state);
//         bob_node.assert_btc_redeemed(bob_end_state);
//     })
//     .await;
// }
