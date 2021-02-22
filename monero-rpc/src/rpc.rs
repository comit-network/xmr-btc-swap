//! JSON RPC clients for `monerd` and `monero-wallet-rpc`.
pub mod monerod;
pub mod wallet;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug, Clone)]
pub struct Request<T> {
    /// JSON RPC version, we hard cod this to 2.0.
    jsonrpc: String,
    /// Client controlled identifier, we hard code this to 1.
    id: String,
    /// The method to call.
    method: String,
    /// The method parameters.
    params: T,
}

/// JSON RPC request.
impl<T> Request<T> {
    pub fn new(method: &str, params: T) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id: "1".to_owned(),
            method: method.to_owned(),
            params,
        }
    }
}

/// JSON RPC response.
#[derive(Deserialize, Serialize, Debug, Clone)]
struct Response<T> {
    pub id: String,
    pub jsonrpc: String,
    pub result: T,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Debug, Clone)]
    struct Params {
        val: u32,
    }

    #[test]
    fn can_serialize_request_with_params() {
        // Dummy method and parameters.
        let params = Params { val: 0 };
        let method = "get_block";

        let r = Request::new(method, &params);
        let got = serde_json::to_string(&r).expect("failed to serialize request");

        let want =
            "{\"jsonrpc\":\"2.0\",\"id\":\"1\",\"method\":\"get_block\",\"params\":{\"val\":0}}"
                .to_string();

        assert_eq!(got, want);
    }
}
