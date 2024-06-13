use anyhow::{bail, Result};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::fs::copy;
use std::path::PathBuf;
use structopt::StructOpt;
#[derive(Debug, StructOpt)]
#[structopt(
    name = "Migrate",
    about = "A tool for migration of xmr-btc-swap database."
)]
struct Args {
    /// Input file
    #[structopt(parse(from_os_str))]
    file_name: PathBuf,
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::from_args();
    let path_str = &args.file_name.display();
    let pool = SqlitePool::connect(&format!("sqlite:{}", path_str)).await?;
    println!("Copying {path_str} to {path_str}.original...");
    copy(&args.file_name, format!("{}.original", path_str))?;
    migrate(&pool).await?;
    println!("Successfully updated the database");
    Ok(())
}

async fn migrate(pool: &SqlitePool) -> Result<()> {
    let mut conn = pool.acquire().await?;

    let rows = sqlx::query!(
        r#"
       SELECT state,id,swap_id
       FROM swap_states
    "#
    )
    .fetch_all(&mut conn)
    .await?;

    for row in rows.iter() {
        let mut bob: Value = match serde_json::from_str(&row.state) {
            Ok(state) => state,
            Err(e) => {
                bail!(e);
            }
        };
        let id = &row.id;
        let swap_id = &row.swap_id;
        let state = bob
            .get_mut("Bob")
            .expect("Incorrect state json in the database");
        let Some((monero_wallet_restore_blockheight, v)) = rows.iter().find_map(|row| {
            if &row.swap_id == swap_id {
                match serde_json::from_str::<Value>(&row.state) {
                    Ok(state) => {
                        if let Some(state) = state
                            .get("Bob")
                            .expect("Incorrect state json in the database")
                            .get("BtcLocked")
                        {
                            return Some((
                                state["monero_wallet_restore_blockheight"].clone(),
                                state["state3"]["v"].clone(),
                            ));
                        }
                        None
                    }
                    Err(err) => {
                        println!("{}", err);
                        None
                    }
                }
            } else {
                None
            }
        }) else {
            bail!("Couldn't find state BtcLocked of swap {}", swap_id);
        };
        if let Some(state) = state.get_mut("BtcCancelled") {
            state["v"] = v.clone();
            state["monero_wallet_restore_blockheight"] = monero_wallet_restore_blockheight.clone();
        } else if let Some(state) = state.get_mut("CancelTimelockExpired") {
            state["v"] = v.clone();
            state["monero_wallet_restore_blockheight"] = monero_wallet_restore_blockheight.clone();
        } else if let Some(end_bob_state) = state.get_mut("Done") {
            if let Some(state) = end_bob_state.get_mut("BtcRefunded") {
                state["v"] = v.clone();
                state["monero_wallet_restore_blockheight"] =
                    monero_wallet_restore_blockheight.clone();
            } else if let Some(bob_state) = end_bob_state.get_mut("BtcPunished") {
                let tx_lock_id = bob_state["tx_lock_id"].clone();
                let Some(state) = rows.iter().find_map(|row| {
                    if &row.swap_id == swap_id {
                        serde_json::from_str::<Value>(&row.state)
                            .ok()?
                            .get("Bob")
                            .expect("Parsing data error.")
                            .get("BtcCancelled")
                            .map(|state| json!({ "BtcPunished": state.clone() }))
                    } else {
                        None
                    }
                }) else {
                    bail!("Couldn't find state BtcCancelled of swap {}", swap_id);
                };
                let bob_state = bob.get_mut("Bob").unwrap();
                *bob_state = state;
                bob_state["BtcPunished"]["v"] = v.clone();
                bob_state["BtcPunished"]["monero_wallet_restore_blockheight"] =
                    monero_wallet_restore_blockheight.clone();
                bob_state["BtcPunished"]["tx_lock_id"] = tx_lock_id;
            }
        }
        let state_string = bob.to_string();
        sqlx::query!(
            r#"
           UPDATE swap_states
           SET state = ?
           WHERE swap_id = ?
           AND id = ?
        "#,
            state_string,
            swap_id,
            id
        )
        .execute(&mut *conn)
        .await?;
        sqlx::query!("VACUUM").execute(&mut *conn).await?; // clear old values.
    }
    Ok(())
}
