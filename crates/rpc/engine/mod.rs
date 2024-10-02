pub mod exchange_transition_config;
pub mod fork_choice;
pub mod payload;

use crate::{utils::RpcRequest, RpcErr, RpcHandler, Store};
use serde_json::{json, Value};

pub type ExchangeCapabilitiesRequest = Vec<String>;

impl Into<RpcRequest> for ExchangeCapabilitiesRequest {
    fn into(self) -> RpcRequest {
        RpcRequest {
            method: "engine_exchangeCapabilities".to_string(),
            params: Some(vec![serde_json::json!(self)]),
            ..Default::default()
        }
    }
}

impl RpcHandler for ExchangeCapabilitiesRequest {
    fn parse(params: &Option<Vec<Value>>) -> Result<Self, RpcErr> {
        params
            .as_ref()
            .ok_or(RpcErr::BadParams)?
            .first()
            .ok_or(RpcErr::BadParams)
            .and_then(|v| serde_json::from_value(v.clone()).map_err(|_| RpcErr::BadParams))
    }

    fn handle(&self, _storage: Store) -> Result<Value, RpcErr> {
        Ok(json!(*self))
    }
}
