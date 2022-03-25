use crate::types::OperationError;

use tungstenite::handshake::client::Response;
use tokio_tungstenite::{connect_async, WebSocketStream, MaybeTlsStream};
use tokio::net::TcpStream;
use regex::Regex;
use url::Url;

/// Get the base currency of the specified symbol.
///
/// # Arguments
/// * `symbol` - fully qualified symbol to get base currency from
///
/// # Returns
/// Return the reference to the string thus no allocation need as the source
/// string is still around. This is reason not to return `String`.
pub fn get_base_currency(symbol: &str) -> Result<&str, ()> {
    // it can be USD, USDT, USDM..., USD<numberic>..., so USD is suffice for the
    // search.
    let matches: Vec<_> = symbol.match_indices("USD").collect();
    if matches.len() == 0 {
        // FIXME: add app's level error case
        return Err(());
    }

    // don't get confused, this is to return the first half of split
    Ok(symbol.split_at(matches[0].0).0)
}

/// Determine whether the specified symbol is linear perpetual.
/// `Note`: On Bybit, there are 3 types of future contracts.
///
/// 1. Inverse Perpetual
/// 2. USDT Perpetual (linear perpetual)
/// 3. Inverse Futures
///
/// Only 2. is the linear perpetual, others are not.
/// Currently only USDT would be applied for Bybit, and thus considered
/// a linear perpetual (thus the name of USDT Perpetual).
///
/// # Arguments
/// * `symbol` - fully qualified symbol to check whether it is a linear perpetual
///
/// # Returns
/// True if it is a linear one, otherwise false.
pub fn is_linear_perpetual(symbol: &str) -> bool {
    symbol.match_indices("USDT").collect::<Vec<_>>().len() == 1    
}

/// Determine the specified symbol whether it is non-perpetual contract or not.
/// 
/// # Arguments
/// * `symbol` - fully qualified symbol to check whether it is a non-perpetual contract
///
/// # Returns
/// True if it is non-perpetual contract, otherwise false.
pub fn is_non_perpetual_contract(symbol: &str) -> bool {
    if is_linear_perpetual(symbol) {
        return false;
    }

    // it could be BTCUSDM22, ETHUSD0325, etc
    let regex = Regex::new(r"\S+USD\S\S+").unwrap();
    regex.is_match(symbol)
}

/// Get the milliseconds and nanoseconds pair from the specified timestamp in
/// milliseconds.
///
/// # Arguments
/// * `ms_timestamp` - timestamp in milliseconds
///
/// # Returns
/// Pair of seconds, and nanoseconds representing the input (ms) timestamp.
pub fn get_ms_and_ns_pair(ms_timestamp: u64) -> (u64, u32) {
    let ms: u64 = ms_timestamp / 1000;
    let ns: u32 = (ms_timestamp % 1000) as u32;
    (ms, ns)
}

/// Connect to specified websocket url.
///
/// # Arguments
/// * `wss_url` - websocket url
pub async fn connect_async_to_wss(wss_url: &str) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), OperationError> {
    let url = match Url::parse(wss_url) {
        Ok(res) => res,
        Err(e) => ret_err!(OperationError::ErrorInternalGeneric, "Url parsing error; err={}", e),
    };

    match connect_async(url).await {
        Ok((ws, response)) => Ok((ws, response)),
        Err(e) => ret_err!(OperationError::ErrorWssConnect, "cannot connect to WSS; err={}", e),
    }
}
