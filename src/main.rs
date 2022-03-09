/**
 * Please note that I've spent much of the time trying to make mio's Poll works
 * with tungstenite with "native-tls" feature to work together. Unfortunately,
 * up until now, I didn't find an answer yet.
 * 
 * See my question on Rust forum if you can help answering it: 
 * https://users.rust-lang.org/t/tls-websocket-how-to-make-tungstenite-works-with-mio-for-poll-and-secure-websocket-wss-via-native-tls-feature-of-tungstenite-crate/72533?u=haxpor 
 *
 * There are also choice whether we will go with async, or sync way.
 * Clearly I want to go with blocking approach, non-async, as simple as possible
 * first for this program. Although you can go with tokio-tungstenite for async
 * way but it's too overkill for me at this point. I would like it to be lightweight
 * as much as possible (for now).
 *
 */
use tungstenite::{connect, Message};
use tungstenite::stream::MaybeTlsStream;
use url::Url;
use rustelebot::*;
use chrono::{NaiveDateTime, DateTime, Utc};
use serde::{Deserialize, Deserializer};
use separator::Separatable;

use std::sync::mpsc::{sync_channel, SyncSender, TryRecvError};
use std::time::Duration;
use regex::Regex;

/// Internal used for between-thread communication through std::sync::mpsc
/// between signal thread, and main message loop in main thread.
enum MsgType {
	PingMsg,
    PongMsg
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum VariantResponse {
    Subscribe(SubscribeResponse),
    Liquidation(GenericResponse<BybitLiquidationData>),
    Trade(GenericResponse<BybitTradeData>),
}

#[derive(Debug, serde::Deserialize)]
struct SubscribeResponse {
    success: bool,
    ret_msg: Option<String>,
    conn_id: String,
    request: SubscribeRequest,
}

#[derive(Debug, serde::Deserialize)]
struct SubscribeRequest {
    op: String,
    args: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct GenericResponse<T> {
    topic: String,
    data: GenericData<T>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum GenericData<T> {
    Liquidation(T),
    Trade(Vec<T>)
}

// we don't need to process anything of this field, thus we don't need to
// convert into specific type e.g. number, but just String for displaying onto
// console, otherwise we can do it with deserialize_with="fn".
#[derive(Debug, serde::Deserialize)]
struct BybitLiquidationData {
    symbol: String,
    side: String,
    
    #[serde(deserialize_with = "de_string_to_number")]
    price: f64,

    #[serde(deserialize_with = "de_string_to_number")]
    qty: u32,   // maximum of trading qty depends on asset, but this would be suffice e.g. BTCUSD maxed at 1,000,000
    time: u64
}

// same, just for representation
// BEWARE: serde_json need "arbitrary_precision" feature in order to support i128, and u128
// see https://github.com/serde-rs/json/pull/506/commits/f69e1ffe3fb07e2e221ea45ec4f4935a86ca1953
#[derive(Debug, serde::Deserialize)]
struct BybitTradeData {
    timestamp: String,
    trade_time_ms: u64,
    symbol: String,
    side: String,
    size: u64,
    price: f64,
    tick_direction: String,
    trade_id: String,
    cross_seq: u64,
}

/// Deserializing function from `String` to numeric which can be any integer,
/// or floating-point number.
///
/// # Also see
/// Look at example at https://serde.rs/stream-array.html
pub fn de_string_to_number<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: std::str::FromStr + serde::Deserialize<'de>,
    <T as std::str::FromStr>::Err: std::fmt::Display // std::str::FromStr has `Err` type, see https://doc.rust-lang.org/std/str/trait.FromStr.html
{
    let buf = String::deserialize(deserializer)?;
    // convert into serde's custom Error type
    buf.parse::<T>().map_err(serde::de::Error::custom)
}

/// Get the base currency of the specified symbol.
///
/// # Arguments
/// * `symbol` - fully qualified symbol to get base currency from
///
/// # Returns
/// Return the reference to the string thus no allocation need as the source
/// string is still around. This is reason not to return `String`.
fn get_base_currency(symbol: &str) -> Result<&str, ()> {
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
fn is_linear_perpetual(symbol: &str) -> bool {
    symbol.match_indices("USDT").collect::<Vec<_>>().len() == 1    
}

/// Determine the specified symbol whether it is non-perpetual contract or not.
/// 
/// # Arguments
/// * `symbol` - fully qualified symbol to check whether it is a non-perpetual contract
///
/// # Returns
/// True if it is non-perpetual contract, otherwise false.
fn is_non_perpetual_contract(symbol: &str) -> bool {
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
fn get_ms_and_ns_pair(ms_timestamp: u64) -> (u64, u32) {
    let ms: u64 = ms_timestamp / 1000;
    let ns: u32 = (ms_timestamp % 1000) as u32;
    (ms, ns)
}

fn main() {
    // create bot instance for telegram
    let telegram_bot_instance = create_instance(
        &std::env::var("HX_BYBIT_SHIPREKT_TELEGRAM_BOT_TOKEN").expect("Required env variable HX_BYBIT_SHIPREKT_TELEGRAM_BOT_TOKEN to be defined"),
        &std::env::var("HX_BYBIT_SHIPREKT_TELEGRAM_CHANNEL_CHAT_ID").expect("Required env variable HX_BYBIT_SHIPREKT_TELEGRAM_CHANNEL_CHAT_ID to be defined"));

    // blocking version of connect (tungstenite::client::connect)
    // for unblocking call use client()
    let (mut socket, _response) = connect(Url::parse("wss://stream.bybit.com/realtime").unwrap()).expect("Can't connect");

    // check that underlying stream is TlsStream
    match socket.get_mut() {
        MaybeTlsStream::NativeTls(t) => {
            // instead of set to non-blocking, we set timeout so we will have
            // an effect of a slightly waiting time used in the main message loop for free
            //t.get_mut().set_nonblocking(true);
            t.get_mut().set_read_timeout(Some(Duration::from_millis(100))).expect("Error: cannot set read-timeout to underlying stream");
        },
        _ => panic!("Error: it is not TlsStream")
    }

    println!("Connected to ByBit realtime websocket");

    let subscribe_res = socket.write_message(Message::Text(r#"{"op": "subscribe", "args": ["liquidation"]}"#.into()));
    if subscribe_res.is_err() {
        panic!("Error: {}", subscribe_res.unwrap_err());
    }

    println!("Subscribed to liquidation websocket");

    // create a async channel with 1 buffer for ping message
    // NOTE: we can have multiple of senders, but only one of receiver
    // so we utilize two of `sync_channel` here for signal back/forth between
    // a spawned thread, and main thread for communication about sending a new
    // message to the websocket.
    let (sender, receiver) : (SyncSender<MsgType>, _) = sync_channel(0);
    let (rev_sender, rev_receiver) : (SyncSender<MsgType>, _) = sync_channel(0);

    std::thread::spawn(move || {
        'outer: loop {
            // sleep for some period of time before sending signal through mpsc
            let sleep_ping = std::time::Duration::from_secs(10);
            std::thread::sleep(sleep_ping);

            let mut is_ok = false;
            match sender.send(MsgType::PingMsg) {
                Ok(_) => {
                    is_ok = true;
                },
                Err(_e) => {
                    eprintln!(" - (internal) Failed in sending ping signal message");

                    // It's not point to continue as we won't receive the PongMsg
                    // back as we didn't successfully send the PingMsg.
                    // Restart the whole process
                    continue;
                }
            }

            // we will wait until the main message loop processes our PingMsg
            // signal instead of continuing sleep the thread unnecessary which
            // might result in excessed of PingMsg in the queue of channel unnecessary
            if is_ok {
                // intended to use goto but Rust doesn't support,
                // we will break the loop when things is ok
                'inner: loop {
                    match rev_receiver.recv() {
                        Ok(signal) => match signal {
                            // See "Nesting and Labels" if we need to break to ouside loop
                            // https://doc.rust-lang.org/rust-by-example/flow_control/loop/nested.html
                            //
                            // In this case we only break the inner loop.
                            MsgType::PongMsg => {
                                break;
                            }

                            // Continue waiting until we receive the right type of signal message.
                            _ => continue
                        },
                        // break to 'outer loop to restart everything
                        Err(_e) => break 'outer
                    }
                }
            }
        }
    });

    // main thread - main loop for message processing
    loop {
        match receiver.try_recv() {
            Ok(signal) => match signal {
                // sending ping msg
                MsgType::PingMsg => {
                    let ping_res = socket.write_message(Message::Ping(r#"{"op":"ping"}"#.into()));
                    match ping_res {
                        Ok(_res) => {
                            let mut is_ok = false;
                            for _ in 0..3 {
                                // send back complete PongMsg
                                match rev_sender.send(MsgType::PongMsg) {
                                    Ok(_) => {
                                        is_ok = true;
                                        break;
                                    },
                                    Err(e) => eprintln!("Error: cannot send back PongMsg; err={}", e)
                                }
                            }

                            if !is_ok {
                                panic!("internal rev_sender error after retrying for max 3 times");
                            }
                        },
                        Err(e) => eprintln!("{}", e)
                    } 
                },
                _ => ()
            },
            Err(TryRecvError::Disconnected) => eprintln!("Sending mechanism disconnected"),
            _ => ()
        }

        match socket.read_message() {
            // we don't distinguish between the type of message, just print it out
            Ok(Message::Ping(_)) => {},
            Ok(Message::Pong(_)) => {},
            Ok(Message::Text(json_str)) => {
                // better to at least we can distingquish between type of messages
                // here.
                match serde_json::from_str::<'_, VariantResponse>(&json_str) {
                    Ok(VariantResponse::Subscribe(_json_obj)) => (),
                    Ok(VariantResponse::Liquidation(json_obj)) => {
                        let inner_json_obj = match json_obj.data {
                            GenericData::Liquidation(json_obj) => json_obj,
                            _ => {
                                eprintln!("Found wrong type of JSON object to parsed for Liquidation");
                                continue;
                            }
                        };

                        let base_currency = get_base_currency(&inner_json_obj.symbol).unwrap_or("UNKNOWN");
                        let is_linear = is_linear_perpetual(&inner_json_obj.symbol);
                        let side = if inner_json_obj.side == "Buy" { "Long" } else { "Short" };

                        let (ms, ns) = get_ms_and_ns_pair(inner_json_obj.time);
                        // FIXME: dang, NaiveDateTime::from_timestamp requires i64, this means
                        // timestamp supports for 132 years further until 2102 since epoch 1970
                        let datetime: DateTime<Utc> = DateTime::from_utc(NaiveDateTime::from_timestamp(ms as i64, ns), Utc);
                        let bankruptcy_worth_str = ((inner_json_obj.price * inner_json_obj.qty as f64 * 1000.0_f64).round() / 1000.0_f64).separated_string(); 
                        let qty_str = inner_json_obj.qty.separated_string(); 
                        let price_str = inner_json_obj.price.separated_string();
                        let base_or_quote_currency_str = if is_linear { "USDT" } else { base_currency };

                        let message = format!("Bybit shiprekt a {side} position of {qty} {base_or_quote_currency} (worth ${bankruptcy_value}) on the {symbol} {perpetual_or_not} contract at ${price} - {datetime_str}",
                            side=side,
                            qty=qty_str,
                            base_or_quote_currency=base_or_quote_currency_str,
                            bankruptcy_value=bankruptcy_worth_str,
                            symbol=inner_json_obj.symbol,
                            perpetual_or_not=if is_non_perpetual_contract(&inner_json_obj.symbol) { "Futures" } else { "Perpetual futures" },
                            price=price_str,
                            datetime_str=datetime.to_string());

                        match send_message(&telegram_bot_instance, &message) {
                            Ok(_) => println!("Notified event: {side} position of {symbol} worth ${bankruptcy_value} with {qty} {base_or_quote_currency} at ${price}",
                                              symbol=inner_json_obj.symbol,
                                              side=side,
                                              bankruptcy_value=bankruptcy_worth_str,
                                              qty=qty_str,
                                              base_or_quote_currency=base_or_quote_currency_str,
                                              price=price_str),
                            // FIXME: upstream fix for rustelebot for `Display` of `ErrorResult`
                            Err(e) => eprintln!("{}", e.msg)
                        }
                    },
                    Ok(VariantResponse::Trade(json_obj)) => println!("{:#?}", json_obj),
                    Err(e) => eprintln!("-- error parsing JSON response: {} --", e),
                }
            },
            Ok(Message::Close(optional_cf)) => match optional_cf {
               // no need to pay attention to close frame, it's already closed
               _ => println!("-- Websocket closed --"),
            },
            //Err(e) => eprintln!("{:?}", e),
            _ => ()
        }
    }
}
