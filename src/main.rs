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
use tungstenite::error::Error as TungsError;
use url::Url;
use rustelebot::*;
use chrono::{NaiveDateTime, DateTime, Utc};
use separator::Separatable;

use std::sync::mpsc::{sync_channel, SyncSender, TryRecvError};
use std::time::Duration;

mod types;
mod deserialize;
mod impls;
mod utils;
mod macros;

use types::*;

/// Connect to the target WSS url
///
/// # Arguments
/// * `wss_url` - wss url to connect to
fn main() {
    // create bot instance for telegram
    let telegram_bot_instance = create_instance(
        &match std::env::var("HX_BYBIT_SHIPREKT_TELEGRAM_BOT_TOKEN") {
            Ok(res) => res,
            Err(e) => errprint_exit1!(OperationError::ErrorMissingRequiredEnvVar, "HX_BYBIT_SHIPREKT_TELEGRAM_BOT_TOKEN not defined; err={}", e),
        },
        &match std::env::var("HX_BYBIT_SHIPREKT_TELEGRAM_CHANNEL_CHAT_ID") {
            Ok(res) => res,
            Err(e) => errprint_exit1!(OperationError::ErrorMissingRequiredEnvVar, "HX_BYBIT_SHIPREKT_TELEGRAM_CHANNEL_CHAT_ID not defined; err={}", e),
        });

    // blocking version of connect (tungstenite::client::connect)
    // for unblocking call use client()
    let (mut socket, _response) = match connect(Url::parse("wss://stream.bybit.com/realtime").unwrap()) {
        Ok(res) => res,
        Err(e) => errprint_exit1!(OperationError::ErrorWssConnect, "cannot connect to WSS; err={}", e),
    };

    // check that underlying stream is TlsStream
    match socket.get_mut() {
        MaybeTlsStream::NativeTls(t) => {
            // instead of set to non-blocking, we set timeout so we will have
            // an effect of a slightly waiting time used in the main message loop for free
            //t.get_mut().set_nonblocking(true);
            match t.get_mut().set_read_timeout(Some(Duration::from_millis(100))) {
                Err(e) => errprint_exit1!(OperationError::ErrorInternalGeneric, "Error: cannot set read-timeout to underlying stream; err={}", e),
                _ => (),
            }
        },
        _ => panic!("Error: it is not TlsStream")
    }

    println!("Connected to ByBit realtime websocket");

    // TODO: provide filtering options through cli e.g. BTCUSD, XRPUSD, etc
    let subscribe_res = socket.write_message(Message::Text(r#"{"op": "subscribe", "args": ["liquidation"]}"#.into()));
    match subscribe_res {
        Ok(_) => println!("subscribed to liquidation topic"),
        Err(e) => errprint_exit1!(OperationError::ErrorWssTopicSubscription, "error subscribing to liquidation topic; err={}", e),
    }

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
            let sleep_ping = std::time::Duration::from_secs(30);
            std::thread::sleep(sleep_ping);

            let mut is_ok = false;
            match sender.send(MsgType::PingMsg) {
                Ok(_) => {
                    is_ok = true;
                },
                Err(_e) => {
                    eprintln!(" - (internal) Failed in sending ping signal message");

                    // There's no point to continue as we won't receive the PongMsg
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

        // it's not necessary to cover all error cases here, but for now it would
        // help to debug in case something weird happen when we actually run
        // it as a long-running process.
        match socket.read_message() {
            // we don't distinguish between the type of message, just print it out
            Ok(Message::Ping(_)) => {},
            Ok(Message::Pong(_)) => {},
            Ok(Message::Binary(bins)) => println!("Received Binbary message, content={}", std::str::from_utf8(&bins).unwrap_or("unknown")),
            Ok(Message::Frame(frame)) => println!("Received Frame message, content={:?}", frame),
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

                        let base_currency = utils::get_base_currency(&inner_json_obj.symbol).unwrap_or("UNKNOWN");
                        let is_linear = utils::is_linear_perpetual(&inner_json_obj.symbol);
                        let side = if inner_json_obj.side == "Buy" { "Long" } else { "Short" };

                        let (ms, ns) = utils::get_ms_and_ns_pair(inner_json_obj.time);
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
                            perpetual_or_not=if utils::is_non_perpetual_contract(&inner_json_obj.symbol) { "Futures" } else { "Perpetual futures" },
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
                    Err(e) => eprintln!("-- error parsing JSON response: {} --", e),
                }
            },
            Ok(Message::Close(optional_cf)) => match optional_cf {
               // no need to pay attention to close frame, it's already closed
               _ => println!("-- Websocket closed --"),
            },
            Err(TungsError::ConnectionClosed) => eprintln!("Error: connection closed"),
            Err(TungsError::AlreadyClosed) => eprintln!("Error: already closed"),
            Err(TungsError::Io(_)) => (),       // don't do anything because Io error means we don't have any incoming data to process
            Err(TungsError::Tls(e)) => eprintln!("Error:: Tls error; err={}", e),
            Err(TungsError::Capacity(e)) => {
                type CError = tungstenite::error::CapacityError;
                match e {
                    CError::TooManyHeaders => eprintln!("Error: CapacityError, too many headers"),
                    CError::MessageTooLong{ size, max_size } => eprintln!("Error: CapacityError, message too long with size={}, max_size={}", size, max_size),
                }
            },
            Err(TungsError::Protocol(e)) => eprintln!("Error: Protocol, err={}", e),
            Err(TungsError::SendQueueFull(e)) => {
                type PMsg = tungstenite::protocol::Message;

                match e {
                    PMsg::Text(text) => eprintln!("Error: SendQueueFull for Text message, content={}", text),
                    PMsg::Binary(bins) => eprintln!("Error: SendQueueFull for Binary message, content={}", std::str::from_utf8(&bins).unwrap_or("unknown")),
                    PMsg::Ping(bins) => eprintln!("Error: SendQueueFull for Ping message, content={}", std::str::from_utf8(&bins).unwrap_or("unknown")),
                    PMsg::Pong(bins) => eprintln!("Error: SendQueueFull for Pong message, content={}", std::str::from_utf8(&bins).unwrap_or("unknown")),
                    PMsg::Close(close_frame_optional) => {
                        match close_frame_optional {
                            Some(close_frame) => eprintln!("Error: SendQueueFull for Close message, content={:?}", close_frame),
                            None => eprintln!("Error: SendQueueFull for Close message, no close-frame content")
                        }
                    },
                    PMsg::Frame(frame) => eprintln!("Error: SendQueueFull for Frame messasge, content={:?}", frame)
                }
            },
            Err(TungsError::Utf8) => eprintln!("Error: Utf8 coding error"),
            Err(TungsError::Url(e)) => eprintln!("Error: Invalid Url; err={:?}", e),
            Err(TungsError::Http(e)) => eprintln!("Error: Http error; err={:?}", e),
            Err(TungsError::HttpFormat(e)) => eprintln!("Error: Http format error; err{:?}", e),
        }
    }
}
